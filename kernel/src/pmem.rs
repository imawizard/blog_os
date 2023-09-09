mod device;

pub use device::*;
pub mod ffi;
pub mod table;

use crate::nfit::Nfit;
use crate::pmem::table::Table;
use crate::vmem::{self};
use alloc::{collections::BTreeMap, vec::Vec};
use log::trace;
use spin::Mutex;
use x86_64::structures::paging::{page::PageRange, PageSize};

pub static MANAGER: Mutex<Manager> = Mutex::new(Manager::new());

pub struct Manager {
    pmems: Vec<ManagedPmem>,
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
            .find_map(|pmem| pmem.pools.allocate(name, size.max(1)))
            .map(|_| self.get_pool(name).unwrap())
    }

    pub fn get_pool(&mut self, name: &str) -> Option<(u64, u64)> {
        self.pmems.iter_mut().find_map(|pmem| {
            pmem.pools
                .entries()
                .into_iter()
                .find(|entry| entry.name() == name)
                .and_then(|entry| {
                    self.translated
                        .get(&entry.offset())
                        .map(|(_, r)| r.start.start_address().as_u64())
                        .map(|addr| (addr, entry.len()))
                        .or_else(|| {
                            vmem::MANAGER
                                .lock()
                                .get_mut()
                                .unwrap()
                                .allocate::<table::PageSize>(
                                    pmem.info.phys_addr + entry.offset(),
                                    entry.frames(),
                                )
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
                                    (r.start.start_address().as_u64(), entry.len())
                                })
                        })
                })
        })
    }

    pub fn destroy_pool(&mut self, name: &str) -> bool {
        let (handle, entry) = match self.pmems.iter().find_map(|pmem| {
            pmem.pools
                .entries()
                .into_iter()
                .find(|entry| entry.name() == name)
                .map(|entry| (pmem.info.handle, entry))
        }) {
            Some((handle, entry)) => (handle, entry),
            None => return false,
        };
        let idx = entry.index();
        let old_offset = entry.offset();

        if self
            .pmems
            .iter_mut()
            .find(|pmem| pmem.info.handle == handle)
            .map(|pmem| pmem.pools.deallocate(idx))
            .unwrap_or(false)
        {
            if let Some((_, r)) = self.translated.get(&old_offset) {
                if vmem::MANAGER
                    .lock()
                    .get_mut()
                    .unwrap()
                    .deallocate::<table::PageSize>(*r)
                {
                    self.translated.remove(&old_offset);
                }
            }

            true
        } else {
            false
        }
    }

    pub fn resize_pool(&mut self, name: &str, new_size: u64) -> Option<(u64, u64)> {
        let (handle, entry) = self.pmems.iter().find_map(|pmem| {
            pmem.pools
                .entries()
                .into_iter()
                .find(|entry| entry.name() == name)
                .map(|entry| (pmem.info.handle, entry))
        })?;
        let idx = entry.index();
        let old_offset = entry.offset();

        if self
            .pmems
            .iter_mut()
            .find(|pmem| pmem.info.handle == handle)?
            .pools
            .reallocate(idx, new_size)
        {
            if let Some((_, r)) = self.translated.get(&old_offset) {
                if vmem::MANAGER
                    .lock()
                    .get_mut()
                    .unwrap()
                    .deallocate::<table::PageSize>(*r)
                {
                    self.translated.remove(&old_offset);
                }
            }
        }

        self.get_pool(name)
    }
}
