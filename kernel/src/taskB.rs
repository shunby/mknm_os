use crate::graphic::{font::write_string, window::{self, Window}, with_layers};
use crate::graphic::graphics::PixelWriter;
use crate::println;

fn initialize_taskB_window() -> window::LayerHandle {
    let mut win = Window::new(160, 52);
    win.move_to((100,200).into());
    win.buffer().write_with(|back|{
        crate::draw_window(back, "taskB!".as_bytes());
    });
    win.buffer().flush();

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
        let win = win.window().read();
        win.buffer().write_with(|back|{
            crate::draw_window(back, "taskB!".as_bytes());
            back.fill_rect((24,28).into(), (80,16).into(), (0xc6,0xc6,0xc6));
            write_string(back, 24, 28, a.as_bytes(), (0,0,0));
        });
        win.buffer().flush();
    }
}