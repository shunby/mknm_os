use core::ptr::{write_volatile, read_volatile};

use alloc::collections::BinaryHeap;
use x86_64::instructions::interrupts::without_interrupts;

use crate::{acpi, interrupt, memory_manager::LazyInit, EVENTS};

const DIVIDE_CONF_ADDR: *mut u32 = 0xfee003e0 as *mut u32;
const LVT_TIMER_ADDR: *mut u32 = 0xfee00320 as *mut u32;
const INITIAL_COUNT_ADDR: *mut u32 = 0xfee00380 as *mut u32;
const CURRENT_COUNT_ADDR: *mut u32 = 0xfee00390 as *mut u32;

const COUNT_MAX: u32 = 0xffffffff;
const TIMER_FREQ: u32 = 100;
static mut LAPIC_TIMER_FREQ: u32 = 0;

static TIMER: LazyInit<TimerManager> = LazyInit::new();
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    timeout: u64,
    value: u64
}

impl Ord for Timer {
    // self <= other :=: self.timeout >= other.timeout
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        other.timeout.cmp(&self.timeout)
    }
}

impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct TimerManager {
    tick: u64,
    timers: BinaryHeap<Timer>
}

impl TimerManager {
    pub fn new() -> Self {
        let timers = BinaryHeap::new();
        Self {tick: 0, timers}
    }

    pub fn tick(&mut self, elapsed: u64) {
        unsafe {
            let t = read_volatile(&self.tick as *const u64);
            write_volatile(&mut self.tick as *mut u64, t+elapsed);
        }
        
        while self.timers.peek().map_or(false, |top|top.timeout <= self.tick) {
            let top = self.timers.pop().unwrap();
            let _ = EVENTS.lock().push(crate::Message::TimerTimeout(top.value));
        }

    }

    pub fn add_timer(&mut self, timeout: u64, value: u64) {
        self.timers.push(Timer {timeout, value});
    }
}

fn initialize_lapic_timer() {
    unsafe {
        write_volatile(DIVIDE_CONF_ADDR, 0b1011); // divide 1:1
        write_volatile(LVT_TIMER_ADDR, 0b001 << 16); // masked, one-shot

        start_lapic_timer();
        acpi::wait_millis(100);
        let elapsed = lapic_timer_elapsed();
        stop_lapic_timer();
        
        LAPIC_TIMER_FREQ = elapsed * 10;
        write_volatile(LVT_TIMER_ADDR, (0b010 << 16) | (interrupt::IVIndex::LapicTimer as u32)); // not-masked, periodic
        write_volatile(INITIAL_COUNT_ADDR, LAPIC_TIMER_FREQ / TIMER_FREQ);
    }
}

fn start_lapic_timer() {
    unsafe {
        write_volatile(INITIAL_COUNT_ADDR, COUNT_MAX);
    }
}

fn lapic_timer_elapsed() -> u32 {
    unsafe {
        COUNT_MAX - read_volatile(CURRENT_COUNT_ADDR)
    }
}

fn stop_lapic_timer() {
    unsafe {
        write_volatile(INITIAL_COUNT_ADDR, 0);
    }
}

pub fn initialize_timer() {
    initialize_lapic_timer();
    TIMER.lock().init(TimerManager::new());
}

pub fn on_lapic_interrupt(elapsed: u64) {
    TIMER.lock().tick(elapsed);
}

pub fn get_current_tick() -> u64 {
    without_interrupts(||{
        TIMER.lock().tick
    })
}

pub fn add_timer(timeout: u64, value: u64) {
    without_interrupts(||{
        TIMER.lock().add_timer(timeout, value);
    });
}
