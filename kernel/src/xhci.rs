use core::{mem::{MaybeUninit, transmute, size_of, ManuallyDrop}, default, iter::repeat_with, alloc::{Layout, Allocator, AllocError}, future::Future, ptr::NonNull};

use alloc::{boxed::Box, vec::Vec, collections::BTreeMap, alloc::{ Global}};
use bitfield::bitfield;
use usb_bindings::raw::usb_set_default_mouse_observer;
use xhci::{accessor::Mapper, registers::{operational::{UsbStatusRegister, UsbCommandRegister}, InterrupterRegisterSet, runtime::EventRingSegmentTableSizeRegister, PortRegisterSet}, ring::trb::{self, Link}, ring::trb::{Type, command::{EnableSlot, AddressDevice}, event::{CommandCompletion, PortStatusChange, CompletionCode, TransferEvent}, transfer::{SetupStage, DataStage, StatusStage}}, Registers, context::{Device32Byte, Device64Byte, Device, Input, Input32Byte, InputHandler, Input64Byte, SlotHandler, EndpointHandler, DeviceHandler}};
use num_traits::cast::FromPrimitive;

static mut MOUSE_OBSERVER: MaybeUninit<Box<dyn Fn(i8,i8)>> = MaybeUninit::uninit();

pub unsafe fn set_default_mouse_observer(f: impl Fn(i8, i8) + 'static) {
    MOUSE_OBSERVER = MaybeUninit::new(Box::new(f));
    usb_set_default_mouse_observer(Some(observer));
}

unsafe extern "C" fn observer(x: i8, y: i8) {
    MOUSE_OBSERVER.assume_init_ref()(x,y);
}