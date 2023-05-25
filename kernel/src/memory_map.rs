use core::{mem::transmute, ptr::slice_from_raw_parts};

#[repr(C)]
pub struct MemoryMapRaw {
    pub buffer: *const u8,
    pub map_size: u64,
    pub map_key: u64,
    pub descriptor_size: u64,
}

impl<'a> Into<MemoryMap<'a>> for &'a MemoryMapRaw {
    fn into(self) -> MemoryMap<'a> {
        print!("map_size: ", self.map_size, "\n");
        unsafe {
            MemoryMap {
                buffer: &*slice_from_raw_parts(self.buffer, self.map_size as usize),
                map_key: self.map_key,
                descriptor_size: self.descriptor_size as usize,
            }
        }
    }
}

pub struct MemoryMap<'a> {
    buffer: &'a [u8],
    pub map_key: u64,
    pub descriptor_size: usize,
}

impl<'a> MemoryMap<'a> {
    pub fn entries(&self) -> MemoryMapIter {
        MemoryMapIter {
            memmap: self,
            seek: 0,
        }
    }
}

pub struct MemoryMapIter<'a> {
    memmap: &'a MemoryMap<'a>,
    seek: usize,
}

impl<'a> Iterator for MemoryMapIter<'a> {
    type Item = &'a MemoryDescriptor;
    fn next(&mut self) -> Option<Self::Item> {
        if self.seek + self.memmap.descriptor_size > self.memmap.buffer.len(){
            None
        } else {
            unsafe {
                let desc: &MemoryDescriptor = transmute(&self.memmap.buffer[self.seek]);
                if desc.attribute == 0 {
                    return None; /* The may be zero-filled entries at the end. */
                }
                
                self.seek += self.memmap.descriptor_size;
                Some(desc)
            }
        }
    }
}

#[repr(C)]
pub struct MemoryDescriptor {
    pub type_: MemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub num_pages: u64,
    pub attribute: u64,
}

impl MemoryDescriptor {
    pub fn is_available(&self) -> bool {
        match self.type_ {
            MemoryType::EfiBootServicesCode | MemoryType::EfiBootServicesData | MemoryType::EfiConventionalMemory => true,
            _ => false
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum MemoryType {
    EfiReservedMemoryType,
    EfiLoaderCode,
    EfiLoaderData,
    EfiBootServicesCode,
    EfiBootServicesData,
    EfiRuntimeServicesCode,
    EfiRuntimeServicesData,
    EfiConventionalMemory,
    EfiUnusableMemory,
    EfiACPIReclaimMemory,
    EfiACPIMemoryNVS,
    EfiMemoryMappedIO,
    EfiMemoryMappedIOPortSpace,
    EfiPalCode,
    EfiPersistentMemory,
    EfiMaxMemoryType,
}

impl MemoryType {
    pub fn to_str(&self) -> &'static str {
        match *self {
            MemoryType::EfiReservedMemoryType => "EfiReservedMemoryType",
            MemoryType::EfiLoaderCode => "EfiLoaderCode",
            MemoryType::EfiLoaderData => "EfiLoaderData",
            MemoryType::EfiBootServicesCode => "EfiBootServicesCode",
            MemoryType::EfiBootServicesData => "EfiBootServicesData",
            MemoryType::EfiRuntimeServicesCode => "EfiRuntimeServicesCode",
            MemoryType::EfiRuntimeServicesData => "EfiRuntimeServicesData",
            MemoryType::EfiConventionalMemory => "EfiConventionalMemory",
            MemoryType::EfiUnusableMemory => "EfiUnusableMemory",
            MemoryType::EfiACPIReclaimMemory => "EfiACPIReclaimMemory",
            MemoryType::EfiACPIMemoryNVS => "EfiACPIMemoryNVS",
            MemoryType::EfiMemoryMappedIO => "EfiMemoryMappedIO",
            MemoryType::EfiMemoryMappedIOPortSpace => "EfiMemoryMappedIOPortSpace",
            MemoryType::EfiPalCode => "EfiPalCode",
            MemoryType::EfiPersistentMemory => "EfiPersistentMemory",
            MemoryType::EfiMaxMemoryType => "EfiMaxMemoryType",
        }
    }
}
