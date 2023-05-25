#[repr(C)]
pub struct MemoryMapRaw {
    pub buffer: *const u8,
    pub map_size: u64,
    pub map_key: u64,
    pub descriptor_size: u64
}