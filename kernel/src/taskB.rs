use crate::{graphic::{font::write_string, window::{self, Window}, with_layers}, task::{switch_context, TaskContext}, TASK_A_CTX, TASK_B_CTX};
use crate::graphic::graphics::{PixelWriter};
use crate::println;

fn initialize_taskB_window() -> window::LayerHandle {
    let mut win = Window::new(160, 52);
    win.move_to((100,200).into());
    crate::draw_window(&mut win, "taskB!".as_bytes());
    let handle = with_layers(|l|{
        let h = l.new_layer(win);
        l.up_down(h.layer_id(), 2);
        h
    });
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
        let t =unsafe{ &TASK_A_CTX};
        unsafe {switch_context(&TASK_A_CTX, &mut TASK_B_CTX);}
    }
}