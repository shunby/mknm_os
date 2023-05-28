use core::arch::global_asm;

const PAGESIZE_4K: u64 = 4096;
const PAGESIZE_2M: u64 = 512 * PAGESIZE_4K;
const PAGESIZE_1G: u64 = 512 * PAGESIZE_2M;

#[repr(align(4096))]
struct  PageMapLv4Table ([u64;512]);

#[repr(align(4096))]
struct PageDirectoryPointerTable ([u64; 512]);

#[repr(align(4096))]
struct PageDirectory ([[u64;512];64]);

static mut PML4_TABLE: PageMapLv4Table = PageMapLv4Table([0;512]);
static mut PDP_TABLE: PageDirectoryPointerTable = PageDirectoryPointerTable([0;512]);
static mut PAGE_DIRS: PageDirectory = PageDirectory([[0u64;512];64]);

pub fn setup_identity_page_table() {
    unsafe {
        PML4_TABLE.0[0] = (&PDP_TABLE.0[0] as *const _ as u64) | 0x003;
        for i_pdpt in 0..PAGE_DIRS.0.len() {
            PDP_TABLE.0[i_pdpt] = (&PAGE_DIRS.0[i_pdpt] as *const _ as u64) | 0x003;
            for i_pd in 0..512 {
                PAGE_DIRS.0[i_pdpt][i_pd] = (i_pdpt as u64 * PAGESIZE_1G + i_pd as u64 * PAGESIZE_2M) | 0x083;
            }
        }
        set_cr3(&PML4_TABLE.0[0] as *const _ as u64);
    }
}

extern "sysv64" {
    fn set_cr3(val: u64);
}
global_asm!(r#"
set_cr3:
    mov cr3, rdi
    ret
"#);

