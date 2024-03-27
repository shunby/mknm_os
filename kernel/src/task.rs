use core::arch::global_asm;

use alloc::collections::VecDeque;

static mut TASKS: Option<TaskManager> = None;

pub struct TaskManager {
    ctxs: VecDeque<TaskContext>,
}

#[repr(C, align(16))]
pub struct TaskContext {
    pub cr3: u64, pub rip: u64, pub rflags: u64, pub rsvd1: u64,
    pub cs: u64, pub ss: u64, pub fs: u64, pub gs: u64,
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rdi: u64, pub rsi: u64, pub rsp: u64, pub rbp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub fxsave_area: [u32; 128]
}

pub fn init_task_manager(ctx_taskB: TaskContext) {
    let mut ctxs = VecDeque::new();
    let ctx_main = TaskContext::new();
    ctxs.push_back(ctx_main);
    ctxs.push_back(ctx_taskB);

    unsafe {TASKS = Some(TaskManager { ctxs });}
}

pub unsafe fn switch_tasks() {
    TASKS.as_mut().unwrap().switch_tasks();
}

extern "C" {
    /// 現在のレジスタの値をcurrent_ctxに退避し、next_ctxに保存されたレジスタの値をCPUに反映する
    fn switch_context(next_ctx: &TaskContext, current_ctx: &mut TaskContext);
}

impl TaskManager {
    pub unsafe fn switch_tasks(&mut self) {
        let old_task = self.ctxs.pop_front().unwrap();
        self.ctxs.push_back(old_task);

        let (front, tail) = self.ctxs.as_mut_slices();
        let (new_task, old_task) = {
            if tail.is_empty() {
                let (f, t) = front.split_at_mut(1);
                (f.first().unwrap(), t.last_mut().unwrap())
            } else {
                (front.first().unwrap(), tail.last_mut().unwrap())
            }
        };
        switch_context(new_task, old_task);
    }
}

impl TaskContext {
    pub const fn new() -> Self {
        Self { cr3: 0, rip: 0, rflags: 0, rsvd1: 0, cs: 0, ss: 0, fs: 0, gs: 0, rax: 0, rbx: 0, rcx: 0, rdx: 0, rdi: 0, rsi: 0, rsp: 0, rbp: 0, r8: 0, r9: 0, r10: 0, r11: 0, r12: 0, r13: 0, r14: 0, r15: 0, fxsave_area: [0;128] }
    }
}



global_asm!(r#"
switch_context:
    mov [rsi + 0x40], rax
    mov [rsi + 0x48], rbx
    mov [rsi + 0x50], rcx
    mov [rsi + 0x58], rdx
    mov [rsi + 0x60], rdi
    mov [rsi + 0x68], rsi

    lea rax, [rsp + 8]
    mov [rsi + 0x70], rax
    mov [rsi + 0x78], rbp

    mov [rsi + 0x80], r8
    mov [rsi + 0x88], r9
    mov [rsi + 0x90], r10
    mov [rsi + 0x98], r11
    mov [rsi + 0xa0], r12
    mov [rsi + 0xa8], r13
    mov [rsi + 0xb0], r14
    mov [rsi + 0xb8], r15

    mov rax, cr3
    mov [rsi + 0x00], rax
    mov rax, [rsp]
    mov [rsi + 0x08], rax
    pushfq
    pop qword PTR [rsi + 0x10]

    mov ax, cs
    mov [rsi + 0x20], rax
    mov bx, ss
    mov [rsi + 0x28], rbx
    mov cx, fs
    mov [rsi + 0x30], rcx
    mov dx, gs
    mov [rsi + 0x38], rdx

    fxsave [rsi + 0xc0]

    push qword PTR [rdi + 0x28]
    push qword PTR [rdi + 0x70]
    push qword PTR [rdi + 0x10]
    push qword PTR [rdi + 0x20]
    push qword PTR [rdi + 0x08]

    fxrstor [rdi + 0xc0]

    mov rax, [rdi + 0x00]
    mov cr3, rax
    mov rax, [rdi + 0x30]
    mov fs, ax
    mov rax, [rdi + 0x38]
    mov gs, ax

    mov rax, [rdi + 0x40]
    mov rbx, [rdi + 0x48]
    mov rcx, [rdi + 0x50]
    mov rdx, [rdi + 0x58]
    mov rsi, [rdi + 0x68]
    mov rbp, [rdi + 0x78]
    mov r8, [rdi + 0x80]
    mov r9, [rdi + 0x88]
    mov r10, [rdi + 0x90]
    mov r11, [rdi + 0x98]
    mov r12, [rdi + 0xa0]
    mov r13, [rdi + 0xa8]
    mov r14, [rdi + 0xb0]
    mov r15, [rdi + 0xb8]

    mov rdi, [rdi+0x60]

    iretq
"#);
