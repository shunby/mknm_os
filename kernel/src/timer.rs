use core::ptr::{write_volatile, read_volatile};

const DIVIDE_CONF_ADDR: *mut u32 = 0xfee003e0 as *mut u32;
const LVT_TIMER_ADDR: *mut u32 = 0xfee00320 as *mut u32;
const INITIAL_COUNT_ADDR: *mut u32 = 0xfee00380 as *mut u32;
const CURRENT_COUNT_ADDR: *mut u32 = 0xfee00390 as *mut u32;

const COUNT_MAX: u32 = 0xffffffff;

pub fn initialize_lapic_timer() {
    unsafe {
        write_volatile(DIVIDE_CONF_ADDR, 0b1011); // divide 1:1
        write_volatile(LVT_TIMER_ADDR, 0b001 << 16 | 32); // masked, one_shot
    }
}

pub fn start_lapic_timer() {
    unsafe {
        write_volatile(INITIAL_COUNT_ADDR, COUNT_MAX);
    }
}

pub fn lapic_timer_elapsed() -> u32 {
    unsafe {
        COUNT_MAX - read_volatile(CURRENT_COUNT_ADDR)
    }
}

pub fn stop_lapic_timer() {
    unsafe {
        write_volatile(INITIAL_COUNT_ADDR, 0);
    }
}
