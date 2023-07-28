use std::env;

pub fn main() {
    let base_dir = "/app/x86_64-elf";
    println!("cargo:rustc-link-search={base_dir}/lib");
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rustc-link-lib=static=c++");
}
