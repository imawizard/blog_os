use std::env;
use std::fs;
use std::os::unix;
use std::path;
use std::process::Command;

fn main() {
    let bios_path = env!("BIOS_IMAGE");

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive")
        .arg(format!("format=raw,file={bios_path}"))
        .arg("-no-reboot")
        .arg("-no-shutdown")
        .arg("-serial")
        .arg("stdio");

    let size_unit = "g";
    let ram_size = 3;
    let nvdimm_size = 1;
    let nvdimm_slots = 2;
    let max_size = ram_size + nvdimm_size * nvdimm_slots;
    cmd.arg("-m")
        .arg(format!(
            "{}{},slots={},maxmem={}{}",
            ram_size,
            size_unit,
            1 + nvdimm_slots,
            max_size,
            size_unit
        ))
        .arg("-machine")
        .arg("nvdimm=on");

    for i in 1..=nvdimm_slots {
        cmd.arg("-object")
            .arg(format!(
                "memory-backend-file,id=mem{},mem-path=pmem-{}.bin,share=on,size={}{}",
                i, i, nvdimm_size, size_unit
            ))
            .arg("-device")
            .arg(format!("nvdimm,id=nvdimm{},memdev=mem{},unarmed=off", i, i));
    }

    let mut args = env::args();
    let first_arg = args.nth(1).unwrap_or_default().to_lowercase();

    match first_arg.as_str() {
        "debug" => {
            cmd.arg("-d") // log ...
                .arg("int") // interrupts/exceptions
                .arg("-S") // freeze CPU at startup
                .arg("-s"); // shorthand for -gdb tcp::1234

            let out = path::Path::new("./target/debug/kernel");
            if let Ok(true) = out.try_exists() {
                fs::remove_file(out).unwrap();
            }

            let kernel_path = env!("KERNEL_BIN");
            unix::fs::symlink(kernel_path, out).unwrap();
        }
        _ => {}
    }

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
