#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader_api::{config::BootloaderConfig, config::Mapping, entry_point, BootInfo};
use core::panic::PanicInfo;
use kernel::task::{executor::Executor, keyboard, Task};
use kernel::{logger, println};

use bootloader_api::info::MemoryRegionKind;
use core::ops::DerefMut;
use kernel::acpi::{self, sdt, AcpiError};
use kernel::nfit;

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use kernel::allocator;
    use kernel::memory;
    use x86_64::VirtAddr;

    logger::init(boot_info.framebuffer.as_mut().expect("no framebuffer"));
    log::set_max_level(log::LevelFilter::Trace);
    println!("Hello World{}", "!");
    kernel::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset.into_option().unwrap());
    let mut mapper = unsafe { memory::init(phys_mem_offset) };

    unsafe {
        memory::FRAMES.lock().init(
            boot_info
                .memory_regions
                .iter()
                .filter(|r| r.kind == MemoryRegionKind::Usable)
                .max_by(|a, b| (a.end - a.start).cmp(&(b.end - b.start)))
                .copied()
                .unwrap(),
        );
    }

    allocator::init_heap(&mut mapper, memory::FRAMES.lock().deref_mut())
        .expect("heap initialization failed");

    let acpi_tables = acpi::get_tables(
        boot_info.rsdp_addr.into_option().expect("no rsdp set"),
        phys_mem_offset,
    );

    let nfit = unsafe {
        acpi_tables
            .get_sdt::<nfit::Nfit>(sdt::Signature::NFIT)
            .unwrap()
            .ok_or(AcpiError::TableMissing(sdt::Signature::NFIT))
            .unwrap()
    };

    for (i, e) in nfit.entries().enumerate() {
        use kernel::println as p;
        use nfit::NfitEntry as E;
        match e {
            E::SpaRange(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::NvdimmRegionMapping(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::Interleave(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::SmbiosManagementInfo(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::NvdimmControlRegion(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::NvdimmBlockDataWindowRegion(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::FlushHintAddress(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
            E::PlatformCapabilities(e) => p!("{}. NFIT Entry: {:#?}", i + 1, e),
        }
    }

    #[cfg(test)]
    test_main();

    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(keyboard::print_keypresses()));
    executor.run();
}

pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    kernel::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel::test_panic_handler(info)
}

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    println!("async number: {}", number);
}

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
