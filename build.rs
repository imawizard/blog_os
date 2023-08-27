use std::env;
use std::path::PathBuf;

use bootloader::DiskImageBuilder;

fn main() {
    // set by cargo for the kernel artifact dependency
    let kernel_path = PathBuf::from(env::var("CARGO_BIN_FILE_KERNEL").unwrap());
    let disk_builder = DiskImageBuilder::new(kernel_path.clone());

    // specify output paths
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bios_path = out_dir.join("blog_os-bios.img");

    // create the disk images
    disk_builder.create_bios_image(&bios_path).unwrap();

    // pass the disk image paths via environment variables
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());

    // also pass the path to the compiled kernel
    println!("cargo:rustc-env=KERNEL_BIN={}", kernel_path.display());
}
