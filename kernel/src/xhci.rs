use core::mem::MaybeUninit;

use alloc::boxed::Box;
use usb_bindings::raw::usb_set_default_mouse_observer;


static mut MOUSE_OBSERVER: MaybeUninit<Box<dyn Fn(i8,i8)>> = MaybeUninit::uninit();

pub unsafe fn set_default_mouse_observer(f: impl Fn(i8, i8) + 'static) {
    MOUSE_OBSERVER = MaybeUninit::new(Box::new(f));
    usb_set_default_mouse_observer(Some(observer));
}

unsafe extern "C" fn observer(x: i8, y: i8) {
    MOUSE_OBSERVER.assume_init_ref()(x,y);
}