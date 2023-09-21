mod device;

pub use device::*;
pub mod ffi;
pub mod table;

use crate::nfit::Nfit;
use crate::pmem::table::Table;
use crate::vmem::{self};
use alloc::alloc::{alloc, dealloc, Layout};
use alloc::{collections::BTreeMap, vec::Vec};
use core::mem::MaybeUninit;
use core::slice;
use corundum::ll;
use log::trace;
use spin::Mutex;
use x86_64::structures::paging::{page::PageRange, Page, PageSize};
use x86_64::VirtAddr;

pub static MANAGER: Mutex<Manager> = Mutex::new(Manager::new());

const USE_HEAP_INSTEAD_OF_PMEM: bool = false;

pub struct Manager {
    pmems: Vec<ManagedPmem>,
    // FIXME: Put handle into key not value, two pools on different dimms might have the same offset
    translated: BTreeMap<u64, (u32, PageRange<table::PageSize>)>,
}

pub struct ManagedPmem {
    info: NfitDevice,
    pools: Table,
}

impl Manager {
    pub const fn new() -> Self {
        Manager {
            pmems: Vec::new(),
            translated: BTreeMap::new(),
        }
    }

    /// # Safety
    ///
    /// Maps the persistent memory's frames and creates mutable references to it.
    /// This function must not be called more than once.
    pub unsafe fn init(&mut self, nfit: &Nfit) {
        let mut locked = vmem::MANAGER.lock();
        let page_allocator = locked.get_mut().unwrap();

        for device in get_devices(nfit).iter() {
            trace!("Found nvdimm {:#?}", device);

            let mapped = page_allocator
                .allocate::<table::PageSize>(device.phys_addr, 1)
                .unwrap();

            self.pmems.push(ManagedPmem {
                info: device.clone(),
                pools: Table::new(device, mapped),
            });
        }
    }

    pub fn create_pool(&mut self, name: &str, size: u64) -> Option<(u64, u64)> {
        if self.get_pool(name).is_some() {
            return None;
        }

        self.pmems
            .iter_mut()
            .find_map(|pmem| pmem.pools.allocate(name, size))
            .map(|_| self.get_pool(name).unwrap())
    }

    pub fn get_pool(&mut self, name: &str) -> Option<(u64, u64)> {
        self.ensure_pool_is_mapped_if_existent(name)
            .and_then(|(handle, index)| {
                let pmem = self.pmems.iter().find(|p| p.info.handle == handle)?;
                let entry = pmem.pools.get(index)?;

                self.translated
                    .get(&entry.offset())
                    .map(|(_, r)| r.start.start_address().as_u64())
                    .map(|addr| (addr, entry.len()))
            })
    }

    pub fn destroy_pool(&mut self, name: &str) -> bool {
        let Some((handle, entry)) = self.pmems.iter().find_map(|pmem| {
            pmem.pools
                .entries()
                .into_iter()
                .find(|entry| entry.name() == name)
                .map(|entry| (pmem.info.handle, entry))
        }) else {
            return false;
        };
        let index = entry.index();
        let offset = entry.offset();
        let real_len = entry.real_len();

        if self
            .pmems
            .iter_mut()
            .find(|pmem| pmem.info.handle == handle)
            .map(|pmem| pmem.pools.deallocate(index))
            .unwrap_or(false)
        {
            if let Some((_, r)) = self.translated.get(&offset) {
                if !USE_HEAP_INSTEAD_OF_PMEM
                    && vmem::MANAGER
                        .lock()
                        .get_mut()
                        .unwrap()
                        .deallocate::<table::PageSize>(*r)
                    || USE_HEAP_INSTEAD_OF_PMEM && {
                        unsafe {
                            dealloc(
                                r.start.start_address().as_u64() as *mut u8,
                                Layout::from_size_align(
                                    real_len as usize,
                                    table::PageSize::SIZE as usize,
                                )
                                .unwrap(),
                            )
                        };
                        true
                    }
                {
                    self.translated.remove(&offset);
                }
            }
            true
        } else {
            false
        }
    }

    pub fn resize_pool(&mut self, name: &str, new_size: u64) -> Option<(u64, u64, u64)> {
        let (handle, index) = self.ensure_pool_is_mapped_if_existent(name)?;
        let pmem = self.pmems.iter_mut().find(|p| p.info.handle == handle)?;
        let entry = pmem.pools.get(index)?;

        let old_offset = entry.offset();
        let old_len = entry.len();
        let old_real_len = entry.real_len();

        trace!(
            "Try to resize pool '{}' from 0x{} (0x{}) to 0x{} bytes",
            entry.name(),
            old_len,
            old_real_len,
            new_size,
        );

        let mut new_offset = None;

        if old_real_len < new_size {
            if !pmem.pools.reallocate(index, new_size) {
                return None;
            }

            let (_, old_pages) = self.translated.remove(&old_offset)?;

            let (handle, index) = self.ensure_pool_is_mapped_if_existent(name)?;
            let pmem = self.pmems.iter_mut().find(|p| p.info.handle == handle)?;
            let entry = pmem.pools.get(index)?;

            let (_, new_pages) = self.translated.get(&entry.offset())?;
            new_offset = Some(entry.offset());

            unsafe {
                let from = slice::from_raw_parts(
                    old_pages.start.start_address().as_ptr::<MaybeUninit<u8>>(),
                    old_real_len as usize,
                );
                let to = slice::from_raw_parts_mut(
                    new_pages.start.start_address().as_mut_ptr(),
                    old_real_len as usize,
                );

                to.copy_from_slice(from);
                ll::persist_obj(&to, true);

                trace!(
                    "Copied 0x{:x} bytes from 0x{:012x} (old) to 0x{:012x} (new)",
                    old_real_len,
                    from.as_ptr() as u64,
                    to.as_ptr() as u64,
                );
            }

            if !USE_HEAP_INSTEAD_OF_PMEM {
                vmem::MANAGER
                    .lock()
                    .get_mut()
                    .unwrap()
                    .deallocate::<table::PageSize>(old_pages);
            } else {
                unsafe {
                    dealloc(
                        old_pages.start.start_address().as_u64() as *mut u8,
                        Layout::from_size_align(
                            old_real_len as usize,
                            table::PageSize::SIZE as usize,
                        )
                        .unwrap(),
                    )
                };
            }
        }

        self.translated
            .get(&new_offset.unwrap_or(old_offset))
            .map(|(_, r)| r.start.start_address().as_u64())
            .map(|addr| (addr, old_len.max(new_size), old_len))
    }

    fn ensure_pool_is_mapped_if_existent(&mut self, name: &str) -> Option<(u32, usize)> {
        self.pmems.iter_mut().find_map(|pmem| {
            pmem.pools
                .entries()
                .into_iter()
                .find(|entry| entry.name() == name)
                .and_then(|entry| {
                    self.translated
                        .contains_key(&entry.offset())
                        .then_some(())
                        .or_else(|| {
                            if !USE_HEAP_INSTEAD_OF_PMEM {
                                vmem::MANAGER
                                    .lock()
                                    .get_mut()
                                    .unwrap()
                                    .allocate::<table::PageSize>(
                                        pmem.info.phys_addr + entry.offset(),
                                        entry.frames(),
                                    )
                            } else {
                                let ptr = unsafe {
                                    alloc(
                                        Layout::from_size_align(
                                            entry.real_len() as usize,
                                            table::PageSize::SIZE as usize,
                                        )
                                        .unwrap(),
                                    )
                                };
                                let first =
                                    Page::from_start_address(VirtAddr::new(ptr as u64)).unwrap();
                                Some(Page::range(first, first + entry.frames()))
                            }
                            .map(|r| {
                                self.translated
                                    .entry(entry.offset())
                                    .or_insert((pmem.info.handle, r));
                                trace!(
                                    "Mapped pool '{}' to 0x{:012x}-0x{:012x}",
                                    entry.name(),
                                    r.start.start_address().as_u64(),
                                    r.start.start_address().as_u64()
                                        + (r.end - r.start) * table::PageSize::SIZE,
                                );
                            })
                        })
                        .map(|_| (pmem.info.handle, entry.index()))
                })
        })
    }
}
