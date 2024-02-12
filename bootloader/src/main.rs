#![no_main]
#![no_std]

mod elf; 
mod frame_buffer;
mod memory_map;

extern crate alloc;

use core::{arch::asm, ffi::c_void, mem::transmute};

use frame_buffer::{FrameBufferConfig, PixelFormat};
use memory_map::MemoryMapRaw;
use uefi::{data_types::PhysicalAddress, prelude::*, proto::console::gop::GraphicsOutput, table::{boot::{AllocateType, MemoryType, OpenProtocolParams, ScopedProtocol, SearchType}, cfg::{ACPI2_GUID, ACPI_GUID}}, Result};

use crate::elf::{ElfFile, Elf64_PhdrType};


fn open_gop(boot_services: &BootServices, image_handle: Handle) -> Result<ScopedProtocol<GraphicsOutput>>{
    let gop_handles = boot_services.locate_handle_buffer(SearchType::from_proto::<GraphicsOutput>())?;
    let gop = unsafe {boot_services.open_protocol::<GraphicsOutput>(OpenProtocolParams {handle: gop_handles[0], agent: image_handle, controller: None} , transmute(1u32))};
    gop
}

const KERNEL_BASE_ADDR: PhysicalAddress = 0x100000;

fn print_gop_info(gop: &mut ScopedProtocol<GraphicsOutput>) {
    uefi_services::println!("Resolution: {}x{}, Pixel Format: {:?}, {} pixels/line",
        gop.current_mode_info().resolution().0,
        gop.current_mode_info().resolution().1,
        gop.current_mode_info().pixel_format(),
        gop.current_mode_info().stride()
        );
    
    uefi_services::println!("Frame Buffer Size: {} bytes",
        gop.frame_buffer().size()
        );
}

fn get_memory_map(boot_services: &BootServices, buf: &mut [u8]) -> MemoryMapRaw{
    let size = boot_services.memory_map_size();
    assert!(size.map_size <= buf.len());
    let buf_ptr = &buf[0] as *const u8;
    let memmap = boot_services.memory_map(buf).expect("failed to get memory map");
    unsafe {
        MemoryMapRaw {
            buffer: buf_ptr,
            map_size: size.map_size as u64,
            map_key: transmute(memmap.key()), 
            descriptor_size: size.entry_size as u64, 
        }
    }
}

fn copy_slice_pad(to: &mut [u8], from: &[u8]) {
    assert!(to.len() >= from.len());
    to[..from.len()].copy_from_slice(from);
    to[from.len()..].fill(0);
}

type EntryPointFn = extern "sysv64" fn(*const FrameBufferConfig, *const MemoryMapRaw, *const c_void);
unsafe fn load_kernel(boot_services: &BootServices, image_handle: Handle) -> EntryPointFn {
    let mut fs = boot_services.get_image_file_system(image_handle).expect("failed to get file system");
    let kernel_file = fs.read(cstr16!("\\kernel.elf")).expect("failed to read '\\kernel.elf'");
    
    let elf_file = ElfFile::from_buffer(&kernel_file);
    let loads = elf_file.prog_headers.iter().filter(|h|h.p_type == Elf64_PhdrType::PT_LOAD);
    
    let (first, last) = elf_file.calc_load_address_range();
    uefi_services::println!("Kernel: 0x{:0x} - 0x{:0x} ({} bytes)", first, last, last - first);

    boot_services.allocate_pages(
        AllocateType::Address(first), 
        MemoryType::LOADER_DATA,
        ((last - first) as usize + 0xfff) / 0x1000
    ).expect("failed to allocate pages");

    // copy LOAD sections from kernel file to memory
    for phdr in loads {
        let buffer = core::slice::from_raw_parts_mut(phdr.inmem_range().0 as *mut u8, phdr.inmem_size() as usize);
        let file = &kernel_file[phdr.infile_range().0 as usize .. phdr.infile_range().1 as usize];
        copy_slice_pad(buffer, file);
        uefi_services::println!("Loaded section: 0x{:0x} - 0x{:0x} ({} bytes)", phdr.inmem_range().0 , phdr.inmem_range().1, phdr.inmem_size());
    }
    
    uefi_services::println!("Entry point: 0x{:0x}", elf_file.elf_header.e_entry);

    unsafe { transmute(elf_file.elf_header.e_entry) }
}

fn construct_frame_buffer(boot_services: &BootServices) -> Result<FrameBufferConfig> {
    let gop_handle = boot_services.get_handle_for_protocol::<GraphicsOutput>()?;
    let mut gop = boot_services.open_protocol_exclusive::<GraphicsOutput>(gop_handle)?;
    Ok(FrameBufferConfig {
        frame_buffer: gop.frame_buffer().as_mut_ptr(),
        horizontal_resolution: gop.current_mode_info().resolution().0 as u32,
        vertical_resolution: gop.current_mode_info().resolution().1 as u32,
        pixels_per_scanline: gop.current_mode_info().stride() as u32,
        pixel_format: match gop.current_mode_info().pixel_format() {
            uefi::proto::console::gop::PixelFormat::Rgb => PixelFormat::PixelRGBResv8BitPerColor,
            uefi::proto::console::gop::PixelFormat::Bgr => PixelFormat::PixelBGRResv8BitPerColor,
            _ => panic!("unsupported pixel format")
        }
    })
}

fn find_acpi_table(system_table: &SystemTable<Boot>) -> *const c_void{
    system_table.config_table().iter().find(|table|table.guid == ACPI2_GUID)
        .expect("failed to get ACPI table").address
}

#[entry]
unsafe fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    uefi_services::print!("Hello, Mikanami World!\n");

    let boot_services = system_table.boot_services();

    let entry_point = load_kernel(boot_services, image_handle);
    
    let acpi_table_address = find_acpi_table(&system_table);
    
    let frame_buffer_config = construct_frame_buffer(boot_services)
        .expect("failed to construct frame buffer config");

    let mut memmap_buf = [0u8; 4096*4];
    let memmap = get_memory_map(boot_services, &mut memmap_buf);


    let (_, _) = system_table.exit_boot_services();

    entry_point(&frame_buffer_config as _, &memmap as _, acpi_table_address);

    halt();
}

fn halt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}