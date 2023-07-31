use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    mem::{transmute, MaybeUninit},
    ptr::null_mut,
    slice::from_raw_parts_mut,
};

use bitfield::size_of;
use lock_api::{GuardNoSend, MutexGuard, RawMutex};

use crate::memory_map::MemoryMap;

/**
 * シングルプロセス専用のMutex
 * ロックされた状態でさらにロックを獲得しようと試みた場合、panicする
 */
pub struct SingleMutex {
    locked: UnsafeCell<bool>,
}

unsafe impl RawMutex for SingleMutex {
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self {
        locked: UnsafeCell::new(false),
    };
    type GuardMarker = GuardNoSend;
    fn lock(&self) {
        unsafe {
            let locked = self.locked.get();
            assert!(!*locked);
            *locked = true;
        }
    }

    fn try_lock(&self) -> bool {
        self.lock();
        true
    }

    unsafe fn unlock(&self) {
        *self.locked.get() = false;
    }
}

unsafe impl Sync for SingleMutex {}

pub type Mutex<T> = lock_api::Mutex<SingleMutex, T>;

type FrameId = usize;

const KB: usize = 1024;
const GB: usize = 1024 * 1024 * 1024;
const BYTES_PER_FRAME: usize = 4 * KB;
const UEFI_PAGE_SIZE: usize = 4 * KB;
const MAX_PHYSICAL_MEMORY_BYTES: usize = 128 * GB;
const FRAME_COUNT: usize = MAX_PHYSICAL_MEMORY_BYTES / BYTES_PER_FRAME;
struct BitMapMemoryManager {
    // 1bit per frame, 1 representing "in use"
    alloc_map: [u8; FRAME_COUNT / 8],
    // the (first, last + 1) frame number to be managed
    available_range: (usize, usize),
}

impl BitMapMemoryManager {
    unsafe fn new_at(ptr: *mut u8, map: &MemoryMap) {
        let manager = ptr as *mut BitMapMemoryManager;

        (*manager).alloc_map.fill(0);

        let mut available_end = 0usize;
        for desc in map.entries() {
            if available_end < desc.physical_start as usize {
                (*manager).mark_allocated(
                    available_end / BYTES_PER_FRAME,
                    (desc.physical_start as usize - available_end) / BYTES_PER_FRAME,
                );
            }
            let physical_end =
                desc.physical_start as usize + desc.num_pages as usize * UEFI_PAGE_SIZE;
            if desc.is_available() {
                available_end = physical_end;
            } else {
                (*manager).mark_allocated(
                    desc.physical_start as usize / BYTES_PER_FRAME,
                    desc.num_pages as usize * UEFI_PAGE_SIZE / BYTES_PER_FRAME,
                );
            }
        }
        (*manager).available_range = (1, available_end / BYTES_PER_FRAME);
    }

    fn set_bit(&mut self, frame: FrameId, allocated: bool) {
        let line_index = frame / 8;
        let bit_index = frame % 8;
        if allocated {
            self.alloc_map[line_index] |= 1 << bit_index;
        } else {
            self.alloc_map[line_index] &= !(1 << bit_index);
        }
    }

    fn get_bit(&self, frame: FrameId) -> bool {
        let line_index = frame / 8;
        let bit_index = frame % 8;
        self.alloc_map[line_index] & (1 << bit_index) != 0
    }

    fn mark_allocated(&mut self, from: FrameId, nframes: usize) {
        for frame in from..from + nframes {
            self.set_bit(frame, true)
        }
    }

    pub fn allocate(&mut self, nframes: usize) -> Option<FrameId> {
        let range = self.available_range;
        let mut start = range.0;

        while start < range.1 - nframes {
            let mut nfree = 0;
            while nfree < nframes && start + nfree < range.1 && !self.get_bit(start + nfree) {
                nfree += 1;
            }
            if nfree == nframes {
                self.mark_allocated(start, nframes);
                return Some(start);
            } else {
                start += nfree + 1;
            }
        }
        None
    }

    pub fn free(&mut self, start: FrameId, nframes: usize) {
        for frame in start..start + nframes {
            self.set_bit(frame, false);
        }
    }

    pub fn get_frame_start(&self, frame: FrameId) -> *mut u8 {
        (frame * BYTES_PER_FRAME) as *mut u8
    }
}

pub struct LazyInitVal<T> {
    init: bool,
    // 制約: init=trueなら初期化されている
    inner: MaybeUninit<T>,
}

impl<T> LazyInitVal<T> {
    pub const fn new() -> Self {
        LazyInitVal {
            inner: MaybeUninit::uninit(),
            init: false,
        }
    }

    pub unsafe fn init_inplace(&mut self, initializer: &dyn Fn(&mut MaybeUninit<T>)) {
        assert!(!self.init);
        initializer(&mut self.inner);
        self.init = true;
    }

    pub fn init(&mut self, content: T) {
        assert!(!self.init);
        self.inner = MaybeUninit::new(content);
        self.init = true;
    }

    pub fn get(&self) -> &T {
        assert!(self.init);
        unsafe { self.inner.assume_init_ref() }
    }

    pub fn get_mut(&mut self) -> &mut T {
        assert!(self.init);
        unsafe { self.inner.assume_init_mut() }
    }
}

pub struct LazyInit<T> {
    // in-placeに初期化したいので、Mutex<Option<T>>は使えない(おそらく)
    inner: Mutex<LazyInitVal<T>>,
}

impl<T> LazyInit<T> {
    pub const fn new() -> Self {
        LazyInit {
            inner: Mutex::new(LazyInitVal::new()),
        }
    }

    pub fn get(&self) -> MutexGuard<'_, SingleMutex, LazyInitVal<T>> {
        self.inner.lock()
    }
}

static MEM: LazyInit<BitMapMemoryManager> = LazyInit::new();

/**
 * cache_page:
 *  32: ..
 *  64: ..
 *  128: ..
 *  ...
 * page: [ PageHeader ] Obj Obj Obj ...
 *
 */

struct FreeList {
    head: Option<&'static Mutex<ObjectHeader>>,
}

impl FreeList {
    unsafe fn push_front(&mut self, obj: *mut u8) {
        let obj = obj as *mut Mutex<ObjectHeader>;
        *obj = Mutex::new(ObjectHeader {
            next_free: self.head,
        });
        self.head = Some(&*obj);
    }

    fn pop_front(&mut self) -> *mut u8 {
        match self.head {
            None => null_mut(),
            Some(head) => {
                // println!("{:x} -> {:x}", head.data_ptr() as usize, head.lock().next_free.map_or(null_mut(), |x|x.data_ptr()) as usize);
                self.head = head.lock().next_free;
                head as *const Mutex<ObjectHeader> as *mut u8
            }
        }
    }

    fn pop_filter(&mut self, predicate: &dyn Fn(*mut u8) -> bool) -> *mut u8 {
        match self.head {
            None => null_mut(),
            Some(head) => {
                if predicate(head as *const Mutex<ObjectHeader> as *mut u8) {
                    self.pop_front()
                } else {
                    let mut prev = head.lock();
                    while let Some(obj) = prev.next_free {
                        if predicate(obj as *const Mutex<ObjectHeader> as *mut u8) {
                            prev.next_free = obj.lock().next_free;
                            return obj as *const Mutex<ObjectHeader> as *mut u8;
                        }
                        prev = obj.lock();
                    }
                    null_mut()
                }
            }
        }
    }
}

#[repr(C)]
struct PageHeader {
    next: *mut PageHeader,
    free_list: FreeList,
    n_objs: usize,
    obj_sz: usize,
}

impl PageHeader {
    pub unsafe fn new_at(page_head: *mut u8, obj_sz: usize) -> &'static Mutex<PageHeader> {
        let page = unsafe {
            let ptr = page_head as *mut Mutex<PageHeader>;
            *ptr = Mutex::new(PageHeader {
                next: null_mut(),
                free_list: FreeList { head: None },
                n_objs: 0,
                obj_sz,
            });
            &*ptr
        };

        let mut page_lock = page.lock();

        // initialize object headers
        let objs_start =
            page_head as usize + (size_of::<PageHeader>() + obj_sz - 1) / obj_sz * obj_sz;
        let mut ptr = page_head as usize + BYTES_PER_FRAME - obj_sz;
        while ptr >= objs_start {
            page_lock.free_list.push_front(ptr as *mut u8);
            // let head = page_lock.free_list.head.unwrap();
            // println!("{:x} -> {:x}", head.data_ptr() as usize, head.lock().next_free.map_or(null_mut(), |x|x.data_ptr()) as usize);
            ptr -= obj_sz;
        }

        page
    }
}

#[repr(C)]
struct ObjectHeader {
    next_free: Option<&'static Mutex<ObjectHeader>>,
}

pub struct ObjectAllocator {
    pages: [&'static Mutex<PageHeader>; ObjectAllocator::N_BLOCK_SIZES],
}
impl ObjectAllocator {
    const N_BLOCK_SIZES: usize = 6;
    const BLOCK_SZ: [usize; ObjectAllocator::N_BLOCK_SIZES] = [64, 128, 256, 512, 1024, 2048];

    pub fn new() -> Self {
        let mut pages: [MaybeUninit<&'static Mutex<PageHeader>>; ObjectAllocator::N_BLOCK_SIZES] =
            unsafe { MaybeUninit::uninit().assume_init() };
        for (i, size) in ObjectAllocator::BLOCK_SZ.iter().enumerate() {
            let ptr = (MEM.get().get_mut().allocate(1).unwrap() * BYTES_PER_FRAME) as *mut u8;
            let page = unsafe { PageHeader::new_at(ptr, *size) };
            pages[i] = MaybeUninit::new(page);
        }
        ObjectAllocator {
            pages: unsafe { transmute(pages) },
        }
    }

    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = ObjectAllocator::BLOCK_SZ
            .iter()
            .enumerate()
            .find(|(_, sz)| **sz > layout.size());
        if size.is_none() {
            return null_mut();
        }
        let (index, _) = size.unwrap();
        let mut page = self.pages[index].lock();

        let is_aligned_fn = |ptr| ptr as usize % layout.align() == 0;
        page.free_list.pop_filter(&is_aligned_fn)
    }

    pub unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let (index, _) = ObjectAllocator::BLOCK_SZ
            .iter()
            .enumerate()
            .find(|(_, sz)| **sz > layout.size())
            .unwrap();
        let mut page = self.pages[index].lock();

        page.free_list.push_front(ptr)
    }
}

unsafe impl GlobalAlloc for LazyInit<ObjectAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.get().get_mut().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.get().get_mut().dealloc(ptr, layout);
    }
}

unsafe impl<T> Sync for LazyInit<T> {}

#[global_allocator]
static GLOBAL_ALLOCATOR: LazyInit<ObjectAllocator> = LazyInit::new();

pub fn init_allocators(map: &MemoryMap) {
    unsafe {
        let mem_init = |inner: &mut MaybeUninit<BitMapMemoryManager>| {
            BitMapMemoryManager::new_at(inner.as_mut_ptr() as *mut u8, map)
        };
        MEM.get().init_inplace(&mem_init);
    }
    GLOBAL_ALLOCATOR.get().init(ObjectAllocator::new());
    run_allocator_tests();
}

pub fn run_allocator_tests() {
    let aligns = [1, 2, 4, 8, 16, 32, 64, 128];
    let sizes = [1, 2, 4, 8, 16, 32, 64, 128];
    for align in aligns {
        for size in sizes {
            let mut ptrs = [null_mut(); 10];
            for i in 0..10 {
                unsafe {
                    let ptr = GLOBAL_ALLOCATOR.alloc(Layout::from_size_align(size, align).unwrap());
                    ptrs[i] = ptr;
                    let alloc = from_raw_parts_mut(ptr, size);

                    let mut x = i;
                    for y in alloc {
                        *y = x as u8;
                        x = (x * 129 + 111) as u8 as usize;
                    }
                }
            }
            // println!("allocated: size = {size}, align = {align}");
            for (i, ptr) in ptrs.iter().enumerate() {
                let alloc = unsafe { from_raw_parts_mut(*ptr, size) };

                let mut x = i;
                for y in alloc {
                    assert!(*y == x as u8);
                    x = (x * 129 + 111) as u8 as usize;
                }

                unsafe {
                    GLOBAL_ALLOCATOR.dealloc(*ptr, Layout::from_size_align(size, align).unwrap())
                };
            }
            // println!("ok: size = {size}, align = {align}");
        }
    }
    println!("run_allocator_tests: finished");
}
