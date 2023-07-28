use std::{fs, env, path::{Path, PathBuf}, process::Command};

pub fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let from_path = Path::new("./src/c/libusb.a");
    let dest_path = out_dir.join("libusb.a");

    Command::new("make").current_dir("./src/c").status().unwrap();

    fs::copy(
        from_path,
        &dest_path
    ).unwrap();
    fs::remove_file(from_path).unwrap();

    println!("cargo:rerun-if-changed=./src/c");
    println!("cargo:rustc-link-search={}", out_dir.to_str().unwrap());
    println!("cargo:rustc-link-lib=static=usb");

    let base_dir = "/app/x86_64-elf";

    let bindings = bindgen::Builder::default()
        .use_core()
        .ctypes_prefix("cty")
        .header("src/c/wrapper.hpp")
        .clang_args([
            "-Isrc/c", 
            &format!("-I{base_dir}"), 
            &format!("-I{base_dir}/include/c++/v1"), 
            &format!("-I{base_dir}/include"), 
            &format!("-I{base_dir}/include/freetype2"), 
            "-nostdlibinc", 
            "-D__ELF__", "-D_LDBL_EQ_DBL", "-D_GNU_SOURCE", "-D_POSIX_TIMERS",
            "-fno-exceptions", "-fno-rtti",
            "-std=c++17", "-O2", "-Wall", "-g", "--target=x86_64-elf", "-ffreestanding", "-mno-red-zone"
            , "-fno-inline-functions"
             ])
        .opaque_type("std::.*")
        .generate_inline_functions(true)
        .allowlist_type("usb::xhci::.*")
        .allowlist_type("usb::HIDMouseDriver")
        .allowlist_function("usb::funcptr_to_stdfunc")
        .allowlist_function("usb::xhci::.*")
        .allowlist_function(".*SetPrintFn")
        .allowlist_function(".*SetLogLevel")
        .allowlist_function(".*set_default_mouse_observer")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

     bindings
         .write_to_file(out_dir.join("bindings.rs"))
         .expect("Couldn't write bindings!");
}
