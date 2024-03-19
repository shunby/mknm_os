use crate::{graphic::font::write_string, task::{switch_context, TaskContext}, graphic::window::{self, Window}, LAYERS, TASK_A_CTX, TASK_B_CTX};
use crate::graphic::graphics::{PixelWriter};
use crate::println;

fn initialize_taskB_window() -> window::LayerHandle {
    let mut win = Window::new(160, 52);
    win.move_to((100,200).into());
    crate::draw_window(&mut win, "taskB!".as_bytes());
    let handle = LAYERS.lock().new_layer(win);
    LAYERS.lock().up_down(handle.layer_id(), 2);
    handle
}

#[allow(unused)]
pub fn taskB() {
    let mut cnt = 0;
    let win = initialize_taskB_window();
    loop {
        cnt += 1;
        let a = format!("{:010}", cnt);
        win.window().lock().fill_rect((24,28).into(), (80,16).into(), (0xc6,0xc6,0xc6));
        write_string(&mut *win.window().lock(), 24, 28, a.as_bytes(), (0,0,0));
        println!("task B speaking!");
        LAYERS.lock().draw();
        let t =unsafe{ &TASK_A_CTX};
        unsafe {switch_context(&TASK_A_CTX, &mut TASK_B_CTX);}
    }
}