use crate::memory::SimpleFrameAllocator;
use alloc::vec::Vec;
use alloc::{collections::BTreeMap, vec};
use core::cell::OnceCell;
use core::cmp::Ordering;
use core::fmt;
use core::ops::{DerefMut, Range};
use spin::Mutex;
use x86_64::structures::paging::page::PageRange;
use x86_64::structures::paging::Page;
use x86_64::structures::paging::PageTableFlags as Flags;
use x86_64::{
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, PageSize, PageTable, PageTableFlags, PhysFrame,
        Size1GiB, Size2MiB, Size4KiB,
    },
    PhysAddr, VirtAddr,
};

pub static MANAGER: Mutex<OnceCell<Manager<SimpleFrameAllocator>>> = Mutex::new(OnceCell::new());

#[derive(Debug)]
pub struct Manager<'a, A> {
    mapper: OffsetPageTable<'a>,
    frame_allocator: &'a Mutex<A>,
    free_regions: BTreeMap<u64, u64>,
}

impl<'a, A> ReserveRegion for Manager<'a, A> {
    fn free_regions(&mut self) -> &mut BTreeMap<u64, u64> {
        &mut self.free_regions
    }
}

impl<'a, A> Manager<'a, A>
where
    A: FrameAllocator<Size4KiB>,
{
    pub fn new(
        mapper: OffsetPageTable<'a>,
        frame_allocator: &'a Mutex<A>,
        usable_regions: impl IntoIterator<Item = Range<u64>>,
    ) -> Self {
        Manager {
            mapper,
            frame_allocator,
            free_regions: usable_regions
                .into_iter()
                .map(|r| (r.end - r.start, r.start))
                .collect(),
        }
    }

    pub fn physical_address(&mut self) -> u64 {
        self.virtual_address() - self.mapper.phys_offset().as_u64()
    }

    pub fn virtual_address(&mut self) -> u64 {
        self.mapper.level_4_table() as *const PageTable as u64
    }

    pub fn allocate<S>(&mut self, phys_start: PhysAddr, page_count: u64) -> Option<PageRange<S>>
    where
        S: PageSize + fmt::Debug,
        OffsetPageTable<'a>: Mapper<S>,
    {
        self.reserve_page_range(page_count.max(1)).map(|r| {
            self.map_page_range(r, phys_start);
            r
        })
    }

    fn reserve_page_range<S: PageSize>(&mut self, page_count: u64) -> Option<PageRange<S>> {
        let needed_size = x86_64::align_up(page_count * S::SIZE, S::SIZE);

        self.reserve_range(needed_size, S::SIZE).map(|r| {
            let addr = VirtAddr::new(r.start);
            let first = Page::<S>::from_start_address(addr).unwrap();
            let last = first + page_count;
            Page::range(first, last)
        })
    }

    fn map_page_range<S>(&mut self, pages: PageRange<S>, phys_start: PhysAddr)
    where
        S: PageSize + fmt::Debug,
        OffsetPageTable<'a>: Mapper<S>,
    {
        let first = PhysFrame::<S>::from_start_address(phys_start).unwrap();
        let last = first + (pages.end - pages.start);
        let frames = PhysFrame::range_inclusive(first, last);

        for (page, frame) in pages.zip(frames) {
            unsafe {
                self.mapper.map_to(
                    page,
                    frame,
                    Flags::PRESENT | Flags::WRITABLE,
                    self.frame_allocator.lock().deref_mut(),
                )
            }
            .unwrap()
            .flush();
        }
    }

    pub fn deallocate<S: PageSize>(&mut self, pages: PageRange<S>) -> bool
    where
        S: PageSize + fmt::Debug,
        OffsetPageTable<'a>: Mapper<S>,
    {
        if self.release_page_range(pages) {
            true
        } else {
            false
        }
    }

    fn release_page_range<S: PageSize>(&mut self, pages: PageRange<S>) -> bool
    where
        S: PageSize + fmt::Debug,
        OffsetPageTable<'a>: Mapper<S>,
    {
        let addr = pages.start.start_address().as_u64();
        let size = (pages.end - pages.start) * S::SIZE;
        if self.release_range(addr..(addr + size)) {
            self.unmap_page_range(pages);
            true
        } else {
            false
        }
    }

    fn unmap_page_range<S>(&mut self, pages: PageRange<S>)
    where
        S: PageSize + fmt::Debug,
        OffsetPageTable<'a>: Mapper<S>,
    {
        for page in pages.into_iter() {
            self.mapper.unmap(page).unwrap().1.flush();
        }
    }

    pub fn usable_regions(&self) -> Vec<Range<u64>> {
        let mut res: Vec<_> = self
            .free_regions
            .iter()
            .map(|(size, addr)| (*size, *addr))
            .collect();
        res.sort_unstable_by(|a, b| a.1.cmp(&b.1));
        res.into_iter()
            .map(|(size, addr)| addr..(addr + size))
            .collect()
    }
}

pub(crate) trait ReserveRegion {
    fn free_regions(&mut self) -> &mut BTreeMap<u64, u64>;

    fn reserve_range(&mut self, needed_size: u64, alignment: u64) -> Option<Range<u64>> {
        let free_regions = self.free_regions();
        assert!(needed_size > 0, "size must be non-zero");

        for (&size, &addr) in free_regions.iter() {
            let aligned = x86_64::align_up(addr, alignment);
            let padding = aligned - addr;

            if needed_size + padding > size {
                continue;
            }

            free_regions.remove(&size);

            let remaining = size - needed_size - padding;
            if remaining > 0 {
                *free_regions.entry(remaining).or_default() = aligned + needed_size;
            }

            if aligned != addr {
                *free_regions.entry(padding).or_default() = addr;
            }

            return Some(aligned..(aligned + needed_size));
        }
        None
    }

    fn release_range(&mut self, region: Range<u64>) -> bool {
        let free_regions = self.free_regions();
        let region_addr = region.start;
        let region_size = region.end - region.start;
        assert!(region_size > 0, "size must be non-zero");

        let mut regions: Vec<(u64, u64)> = free_regions
            .iter()
            .map(|(&size, &addr)| (addr, size))
            .collect();

        regions.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        match regions.binary_search_by(|&(addr, size)| {
            if (addr..(addr + size)).contains(&region_addr) {
                Ordering::Equal
            } else {
                addr.cmp(&region_addr)
            }
        }) {
            Ok(_) => return false,
            Err(i) => regions.insert(i, (region_addr, region_size)),
        }

        free_regions.clear();
        regions
            .into_iter()
            .map(|(addr, size)| (addr, addr + size))
            .fold(vec![], |mut acc: Vec<(u64, u64)>, (start, end)| {
                match acc.last_mut() {
                    Some(last) if last.1 == start => last.1 = end,
                    _ => acc.push((start, end)),
                }
                acc
            })
            .into_iter()
            .for_each(|(start, end)| *free_regions.entry(end - start).or_default() = start);

        true
    }
}

pub fn get_mappings(mapper: &mut OffsetPageTable) -> Vec<VirtMapping> {
    let mut res = Vec::new();

    let offset = mapper.phys_offset().as_u64();
    for (i, e) in mapper
        .level_4_table()
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.is_unused())
    {
        let virt = (i as u64) << 12 << 9 << 9 << 9;
        let phys = e.addr().as_u64();
        let page_dir_ptr_table = unsafe { &*((phys + offset) as *const u64 as *const PageTable) };

        for (i, e) in page_dir_ptr_table
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_unused())
        {
            let virt = virt | (i as u64) << 12 << 9 << 9;
            let phys = e.addr().as_u64();
            if e.flags().contains(PageTableFlags::HUGE_PAGE) {
                let virt = VirtAddr::new(virt);
                let phys = PhysAddr::new(phys);
                res.push(VirtMapping {
                    virt: Pages::Huge(Page::from_start_address(virt).unwrap()),
                    phys: PhysFrames::Huge(PhysFrame::from_start_address(phys).unwrap()),
                });
                continue;
            }

            let page_dir_table = unsafe { &*((phys + offset) as *const u64 as *const PageTable) };
            for (i, e) in page_dir_table
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.is_unused())
            {
                let virt = virt | (i as u64) << 12 << 9;
                let phys = e.addr().as_u64();
                if e.flags().contains(PageTableFlags::HUGE_PAGE) {
                    let virt = VirtAddr::new(virt);
                    let phys = PhysAddr::new(phys);
                    res.push(VirtMapping {
                        virt: Pages::Large(Page::from_start_address(virt).unwrap()),
                        phys: PhysFrames::Large(PhysFrame::from_start_address(phys).unwrap()),
                    });
                    continue;
                }

                let page_table = unsafe { &*((phys + offset) as *const u64 as *const PageTable) };
                for (i, _) in page_table
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.is_unused())
                {
                    let virt = virt | (i as u64) << 12;
                    let phys = e.addr().as_u64();

                    let virt = VirtAddr::new(virt);
                    let phys = PhysAddr::new(phys);
                    res.push(VirtMapping {
                        virt: Pages::Regular(Page::from_start_address(virt).unwrap()),
                        phys: PhysFrames::Regular(PhysFrame::from_start_address(phys).unwrap()),
                    });
                }
            }
        }
    }

    res
}

impl<T: ?Sized> MappedRegions for T where T: IntoIterator<Item = VirtMapping> {}
pub trait MappedRegions: IntoIterator<Item = VirtMapping> {
    fn into_regions(self) -> Vec<MappedRegion>
    where
        Self: Sized,
    {
        self.into_iter()
            .map(|m| {
                (
                    m.virt.start_address().as_u64(),
                    m.virt.start_address().as_u64() + m.virt.size(),
                    m.phys.start_address().as_u64(),
                    m.phys.start_address().as_u64() + m.phys.size(),
                )
            })
            .map(|(virt_start, virt_end, phys_start, phys_end)| {
                (virt_start..virt_end, phys_start..phys_end)
            })
            .fold(vec![], |mut acc: Vec<_>, (virt, phys)| {
                match acc.last_mut() {
                    Some(last) if last.virt.end == virt.start => last.virt.end = virt.end,
                    _ => acc.push(MappedRegion { virt, phys }),
                }
                acc
            })
    }
}

const FIRST_ADDRESS: u64 = 10 * Size4KiB::SIZE;
const LAST_ADDRESS: u64 = 1_u64 << 48;

impl<T: ?Sized> UsableRegions for T where T: IntoIterator<Item = MappedRegion> {}
pub trait UsableRegions: IntoIterator<Item = MappedRegion> {
    fn into_usable(self) -> Vec<Range<u64>>
    where
        Self: Sized,
    {
        let mut res = Vec::new();
        let mut current = FIRST_ADDRESS;

        for region in self.into_iter() {
            res.push(current..region.virt.start);
            current = region.virt.end;
        }

        if current < LAST_ADDRESS {
            res.push(current..LAST_ADDRESS);
        }
        res.into_iter().filter(|r| r.start < r.end).collect()
    }
}

pub struct VirtMapping {
    pub virt: Pages,
    pub phys: PhysFrames,
}

pub struct MappedRegion {
    pub virt: Range<u64>,
    pub phys: Range<u64>,
}

pub enum Pages {
    Regular(Page<Size4KiB>),
    Large(Page<Size2MiB>),
    Huge(Page<Size1GiB>),
}

pub enum PhysFrames {
    Regular(PhysFrame<Size4KiB>),
    Large(PhysFrame<Size2MiB>),
    Huge(PhysFrame<Size1GiB>),
}

impl Pages {
    fn start_address(&self) -> VirtAddr {
        match self {
            Self::Regular(p) => p.start_address(),
            Self::Large(p) => p.start_address(),
            Self::Huge(p) => p.start_address(),
        }
    }

    fn size(&self) -> u64 {
        match self {
            Self::Regular(p) => p.size(),
            Self::Large(p) => p.size(),
            Self::Huge(p) => p.size(),
        }
    }
}

impl PhysFrames {
    fn start_address(&self) -> PhysAddr {
        match self {
            Self::Regular(p) => p.start_address(),
            Self::Large(p) => p.start_address(),
            Self::Huge(p) => p.start_address(),
        }
    }

    fn size(&self) -> u64 {
        match self {
            Self::Regular(p) => p.size(),
            Self::Large(p) => p.size(),
            Self::Huge(p) => p.size(),
        }
    }
}
