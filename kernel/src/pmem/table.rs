use super::NfitDevice;
use crate::vmem::ReserveRegion;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::ffi::CStr;
use core::mem;
use core::ops;
use core::str;
use corundum::ll;
use log::trace;
use x86_64::structures::paging::page::PageRange;
use x86_64::structures::paging::{PageSize as PageSizeTrait, Size4KiB};

pub const MAGIC_NUMBER: u16 = 0x9898;
/// Size of mapped pages and the corresponding frames.
pub type PageSize = Size4KiB;

const ENTRY_SPACE: usize = PageSize::SIZE as usize - 2;
const ENTRY_COUNT: usize = ENTRY_SPACE / mem::size_of::<Entry>();
const NAME_LEN: usize = 30;

const _: () = assert!(
    mem::size_of::<Inner>() as u64 == PageSize::SIZE,
    "The pool table should fill an entire page"
);

pub struct Table {
    inner: &'static mut Inner,
    free_regions: BTreeMap<u64, u64>,
}

impl Table {
    /// # Safety
    ///
    /// Caller must ensure that there are no other references made from the
    /// passed address.
    pub unsafe fn new(device: &NfitDevice, pages: PageRange<PageSize>) -> Self {
        let address = pages.start.start_address().as_u64();
        let inner = Inner::new(address);
        let free_regions: BTreeMap<u64, u64>;

        trace!(
            "Validate pmem table at 0x{:012x}-0x{:012x})",
            address,
            address + device.size
        );

        if inner.is_valid() {
            let mut taken: Vec<_> = inner
                .entries()
                .into_iter()
                .inspect(|entry| {
                    trace!(
                        "Found pool '{}' at offset 0x{:012x} (size: {} MiB, real size: {} MiB)",
                        entry.name(),
                        entry.offset(),
                        entry.len() as f64 / 1024_f64 / 1024_f64,
                        entry.real_len() as f64 / 1024_f64 / 1024_f64,
                    )
                })
                .map(|entry| entry.offset()..(entry.offset() + entry.real_len()))
                .collect();

            taken.sort_unstable_by(|a, b| a.start.cmp(&b.start));

            let mut usable = Vec::new();
            let mut current = PageSize::SIZE;

            for region in taken.into_iter() {
                usable.push(current..region.start);
                current = region.end;
            }

            if current < device.size {
                usable.push(current..device.size);
            }

            free_regions = usable
                .into_iter()
                .map(|r| (r.end - r.start, r.start))
                .filter(|(size, _)| *size > 0)
                .collect();
        } else {
            trace!("Write empty table");

            inner.init();
            free_regions = [(device.size - PageSize::SIZE, PageSize::SIZE)]
                .into_iter()
                .filter(|(size, _)| *size > 0)
                .collect();
        }

        Table {
            inner,
            free_regions,
        }
    }

    pub fn allocate(&mut self, name: &str, size: u64) -> Option<u64> {
        let needed_size = size.max(PageSize::SIZE);
        if name.len() > NAME_LEN || self.inner.entries().into_iter().count() == ENTRY_COUNT {
            return None;
        }

        let r = self.reserve_range(needed_size, PageSize::SIZE)?;
        self.inner.insert(name, r.start, size);

        Some(r.start)
    }

    pub fn deallocate(&mut self, index: usize) -> bool {
        let Some(entry) = self.inner.entries.get(index) else {
            return false;
        };

        let offset = entry.offset();
        let len = entry.real_len();

        if self.release_range(offset..(offset + len)) {
            self.inner.remove(index)
        } else {
            false
        }
    }

    pub fn reallocate(&mut self, index: usize, new_size: u64) -> bool {
        let needed_size = new_size.max(PageSize::SIZE);
        let Some(entry) = self.inner.entries.get(index) else {
            return false;
        };
        if entry.real_len() >= needed_size {
            return false;
        }

        let old_range = entry.offset..(entry.offset + entry.real_len());
        let Some(new_range) = self.reserve_range(needed_size, PageSize::SIZE) else {
            return false;
        };
        self.release_range(old_range.clone());

        let Some(entry) = self.inner.entries.get_mut(index) else {
            return false;
        };
        entry.offset = new_range.start;
        entry.length = needed_size;

        ll::persist_obj(self, true);

        true
    }

    pub fn entries(&self) -> impl IntoIterator<Item = IterEntry> {
        self.inner.entries()
    }
}

impl ReserveRegion for Table {
    fn free_regions(&mut self) -> &mut BTreeMap<u64, u64> {
        &mut self.free_regions
    }
}

#[repr(C, packed)]
struct Inner {
    magic_number: u16,
    entries: [Entry; ENTRY_COUNT],
}

impl Inner {
    unsafe fn new(address: u64) -> &'static mut Self {
        &mut *(address as *mut Inner)
    }

    fn is_valid(&self) -> bool {
        self.magic_number == MAGIC_NUMBER
    }

    fn init(&mut self) {
        self.magic_number = MAGIC_NUMBER;
        self.entries.fill(Default::default());

        ll::persist_obj(self, true);
    }

    fn insert(&mut self, name: &str, offset: u64, length: u64) -> Option<usize> {
        let (i, entry) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .enumerate()
            .find(|(_, entry)| entry.name().is_empty())?;
        let n = entry.name.len().min(name.len());

        entry.name.fill(0);
        entry.name[..n].copy_from_slice(&name.as_bytes()[..n]);
        entry.offset = offset;
        entry.length = length;

        ll::persist_obj(entry, true);
        Some(i)
    }

    fn remove(&mut self, index: usize) -> bool {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.name.fill(0);
            entry.offset = 0;
            entry.length = 0;

            ll::persist_obj(entry, true);
            true
        } else {
            false
        }
    }

    fn entries(&self) -> impl IntoIterator<Item = IterEntry> {
        self.entries
            .as_slice()
            .iter()
            .filter(|entry| !entry.name().is_empty())
            .enumerate()
            .map(|(i, e)| IterEntry { index: i, inner: e })
    }
}

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Entry {
    offset: u64,
    length: u64,
    name: [u8; NAME_LEN],
}

impl Entry {
    pub fn name(&self) -> &str {
        CStr::from_bytes_until_nul(self.name.as_slice())
            .map(|s| s.to_str().unwrap())
            .or_else(|_| str::from_utf8(self.name.as_slice()))
            .unwrap()
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn real_len(&self) -> u64 {
        x86_64::align_up(self.len(), PageSize::SIZE)
    }

    pub fn frames(&self) -> u64 {
        self.real_len() / PageSize::SIZE
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub struct IterEntry<'a> {
    index: usize,
    inner: &'a Entry,
}

impl<'a> IterEntry<'a> {
    pub fn index(&self) -> usize {
        self.index
    }
}

impl<'a> ops::Deref for IterEntry<'a> {
    type Target = Entry;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}
