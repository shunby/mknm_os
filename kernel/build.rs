use std::env;

pub fn main() {
    let base_dir = format!("{}/osbook/devenv/x86_64-elf", env::var_os("HOME").unwrap().to_str().unwrap());
    println!("cargo:rustc-link-search={base_dir}/lib");
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rustc-link-lib=static=c++");
}
