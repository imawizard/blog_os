use std::env;
use std::process::Command;

fn main() {
    let bios_path = env!("BIOS_IMAGE");

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive")
        .arg(format!("format=raw,file={bios_path}"));

    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
