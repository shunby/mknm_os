#![no_main]
#![no_std]

mod elf; 
mod frame_buffer;

extern crate alloc;

use core::{arch::asm, mem::{transmute}};

use frame_buffer::{FrameBufferConfig, PixelFormat};
use uefi::{prelude::*, table::{boot::{AllocateType, MemoryType, SearchType, ScopedProtocol, OpenProtocolParams, OpenProtocolAttributes}}, proto::{console::gop::GraphicsOutput}, Result, data_types::PhysicalAddress};

use crate::elf::{read_elf, Elf64_PhdrType, calc_load_address_range};


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

type EntryPointFn = extern "sysv64" fn(FrameBufferConfig);
fn load_kernel(boot_services: &BootServices, image_handle: Handle) -> *const EntryPointFn {
    let mut fs = boot_services.get_image_file_system(image_handle).expect("failed to get file system");
    let kernel_file = fs.read(cstr16!("\\kernel.elf")).expect("failed to read '\\kernel.elf'");
    
    let (ehdr, phdrs) = read_elf(&kernel_file);
    let loads = phdrs.iter().filter(|h|h.p_type == Elf64_PhdrType::PT_LOAD);
    
    let (first, last) = calc_load_address_range(phdrs);
    uefi_services::println!("Kernel: 0x{:0x} - 0x{:0x} ({} bytes)", first, last, last - first);

    boot_services.allocate_pages(
        AllocateType::Address(first), 
        MemoryType::LOADER_DATA,
        ((last - first) as usize + 0xfff) / 0x1000
    ).expect("failed to allocate pages");

    for phdr in loads {
        let (sec_begin, sec_end, sec_len) = (phdr.p_vaddr as usize, (phdr.p_vaddr + phdr.p_memsz) as usize, phdr.p_memsz as usize);
        let buffer = unsafe {
            core::slice::from_raw_parts_mut(sec_begin as *mut u8, sec_len)
        };
        
        let (hdr_begin, hdr_end, hdr_len) = (phdr.p_offset as usize, (phdr.p_offset + phdr.p_filesz) as usize, phdr.p_filesz as usize);
        buffer[..hdr_len].copy_from_slice(&kernel_file[hdr_begin..hdr_end]);
        buffer[hdr_len..sec_len].fill(0);
        uefi_services::println!("Loaded section: 0x{:0x} - 0x{:0x} ({} bytes)", sec_begin, sec_end, sec_len);
    }
    
    uefi_services::println!("Entry point: 0x{:0x}", ehdr.e_entry);

    ehdr.e_entry as *const EntryPointFn
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

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();

    uefi_services::print!("Hello, Mikanami World!\n");

    let boot_services = system_table.boot_services();

    let entry_addr = load_kernel(boot_services, image_handle);
    let entry_point: EntryPointFn = unsafe {transmute(entry_addr)};
    
    let frame_buffer_config = construct_frame_buffer(boot_services)
        .expect("failed to construct frame buffer config");

    let (_, _) = system_table.exit_boot_services();

    //FIXME: GOPのFrameBufferをboot_servicesの外に持ち出してしまっている
    entry_point(frame_buffer_config);

    halt();
}

fn halt() -> ! {
    loop {
        unsafe {
            asm!("hlt");
        }
    }
}