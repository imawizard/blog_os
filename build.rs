use std::env;
use std::fs;
use std::os::unix;
use std::path::PathBuf;

use bootloader::DiskImageBuilder;

const IMAGE_NAME: &str = "blog_os-bios.img";

fn main() {
    // set by cargo for the kernel artifact dependency
    let kernel_path = PathBuf::from(env::var("CARGO_BIN_FILE_KERNEL").unwrap());
    let disk_builder = DiskImageBuilder::new(kernel_path.clone());

    // specify output paths
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bios_path = out_dir.join(IMAGE_NAME);

    // create the disk images
    disk_builder.create_bios_image(&bios_path).unwrap();

    // symlink the disk image
    let out = PathBuf::from("./target/debug/").join(IMAGE_NAME);
    if let Ok(true) = out.try_exists() {
        fs::remove_file(&out).unwrap();
    }
    unix::fs::symlink(&bios_path, &out).unwrap();

    // pass the disk image paths via environment variables
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());

    // also pass the path to the compiled kernel
    println!("cargo:rustc-env=KERNEL_BIN={}", kernel_path.display());
}
