use std::env;
use std::fs;
use std::os::unix;
use std::path;
use std::process::Command;

fn main() {
    let bios_path = env!("BIOS_IMAGE");

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive")
        .arg(format!("format=raw,file={bios_path}"));

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
