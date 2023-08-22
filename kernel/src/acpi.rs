pub use acpi::*;

use crate::println;
use core::ptr::NonNull;
use x86_64::VirtAddr;

pub fn get_tables(rsdp: u64, physical_memory_offset: VirtAddr) -> AcpiTables<impl AcpiHandler> {
    let mapping = OffsetMapped(physical_memory_offset.as_u64());
    let tables;
    unsafe {
        tables = AcpiTables::from_rsdp(mapping, rsdp as usize).expect("failed to read acpi tables");
    }
    println!("acpi revision: {:?}", tables.revision);
    tables
}

#[derive(Clone)]
pub struct OffsetMapped(pub u64);

impl AcpiHandler for OffsetMapped {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        PhysicalMapping::new(
            physical_address,
            NonNull::new((physical_address + self.0 as usize) as *mut _).unwrap(),
            size,
            size,
            Self(self.0),
        )
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}
}
