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

    let base_dir = format!("{}/osbook/devenv/x86_64-elf", env::var_os("HOME").unwrap().to_str().unwrap());

// bindgen wrapper.hpp -o bindings.rs -- -I. -I$HOME/osbook/devenv/x86_64-elf -I$HOME/osbook/devenv/x86_64-elf/include/c++/v1 -I$HOME/osbook/devenv/x86_64-elf/include -I$HOME/osbook/devenv/x86_64-elf/include/freetype2 -nostdlibinc -D__ELF__ -D_LDBL_EQ_DBL -D_GNU_SOURCE -D_POSIX_TIMERS -fno-exceptions -fno-rtti -std=c++17 -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone
//  bindgen wrapper.hpp --use-core --ctypes-prefix cty -o bindings.rs -- -I. -I$HOME/osbook/devenv/x86_64-elf -I$HOME/osbook/devenv/x86_64-elf/include/c++/v1  -I$HOME/osbook/devenv/x86_64-elf/include -I$HOME/osbook/devenv/x86_64-elf/include/freetype2 -nostdlibinc -D__ELF__ -D_LDBL_EQ_DBL -D_GNU_SOURCE -D_POSIX_TIMERS -fno-exceptions -fno-rtti -std=c++17 -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone
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
            // "-I/usr/lib/llvm-7/lib/clang/7.0.1/include/",
            "-nostdlibinc", 
            "-D__ELF__", "-D_LDBL_EQ_DBL", "-D_GNU_SOURCE", "-D_POSIX_TIMERS",
            "-fno-exceptions", "-fno-rtti",
            "-std=c++17", "-O2", "-Wall", "-g", "--target=x86_64-elf", "-ffreestanding", "-mno-red-zone"
            // "-ffreestanding", "-mno-red-zone",
            // "-fno-exceptions", "-fno-rtti",  "-O2", "-Wall", "-g", "--target=x86_64-elf"
             ])
        .opaque_type("std::.*")
        .allowlist_type("usb::xhci::Controller")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .generate()
        .expect("Unable to generate bindings");

     bindings
         .write_to_file(out_dir.join("bindings.rs"))
         .expect("Couldn't write bindings!");
}

/*
TARGET = kernel.elf
OBJS = main.o graphics.o mouse.o font.o hankaku.o newlib_support.o console.o \
       pci.o asmfunc.o libcxx_support.o logger.o \
       usb/memory.o usb/device.o usb/xhci/ring.o usb/xhci/trb.o usb/xhci/xhci.o \
       usb/xhci/port.o usb/xhci/device.o usb/xhci/devmgr.o usb/xhci/registers.o \
       usb/classdriver/base.o usb/classdriver/hid.o usb/classdriver/keyboard.o \
       usb/classdriver/mouse.o
DEPENDS = $(join $(dir $(OBJS)),$(addprefix .,$(notdir $(OBJS:.o=.d))))

CPPFLAGS += -I.
CFLAGS   += -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone
CXXFLAGS += -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone \
            -fno-exceptions -fno-rtti -std=c++17
LDFLAGS  += --entry KernelMain -z norelro --image-base 0x100000 --static


.PHONY: all
all: $(TARGET)

.PHONY: clean
clean:
	rm -rf *.o

kernel.elf: $(OBJS) Makefile
	ld.lld $(LDFLAGS) -o kernel.elf $(OBJS) -lc -lc++

%.o: %.cpp Makefile
	clang++ $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@

.%.d: %.cpp
	clang++ $(CPPFLAGS) $(CXXFLAGS) -MM $< > $@
	$(eval OBJ = $(<:.cpp=.o))
	sed --in-place 's|$(notdir $(OBJ))|$(OBJ)|' $@

%.o: %.c Makefile
	clang $(CPPFLAGS) $(CFLAGS) -c $< -o $@

.%.d: %.c
	clang $(CPPFLAGS) $(CFLAGS) -MM $< > $@
	$(eval OBJ = $(<:.c=.o))
	sed --in-place 's|$(notdir $(OBJ))|$(OBJ)|' $@

%.o: %.asm Makefile
	nasm -f elf64 -o $@ $<

hankaku.bin: hankaku.txt
	../tools/makefont.py -o $@ $<

hankaku.o: hankaku.bin
	objcopy -I binary -O elf64-x86-64 -B i386:x86-64 $< $@

.%.d: %.bin
	touch $@

.PHONY: depends
depends:
	$(MAKE) $(DEPENDS)

-include $(DEPENDS)

 */