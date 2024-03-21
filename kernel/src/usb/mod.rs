use alloc::boxed::Box;
use futures::Future;

use crate::{memory_manager::LazyInit, pci::PCIDevice};

use self::{runtime::{new_channel, new_executor_and_spawner, Executor, Spawner}, xhci::{initialize_xhci, XhciError}};

pub mod usbd;
pub mod xhci;
mod runtime;
mod ring;
mod class;
mod device;
mod util;
mod action;

static EXECUTOR: LazyInit<Executor<'static, Result<(), XhciError>>> = LazyInit::new();
pub static SPAWNER: LazyInit<Spawner<'static, Result<(), XhciError>>> = LazyInit::new();

pub unsafe fn init_usb(
    xhc: PCIDevice, 
    intel_ehci_found: bool, 
    mouse_callback: Box<dyn Fn(Box<class::mouse::MouseReport>) + Send>,
    key_callback: Box<dyn Fn(Box<class::keyboard::KeyReport>) + Send>
) {
    let (executor, spawner) = new_executor_and_spawner::<Result<(), XhciError>>();
    EXECUTOR.lock().init(executor);
    SPAWNER.lock().init(spawner);

    let (addr_send, addr_recv) = new_channel();
    initialize_xhci(xhc, intel_ehci_found, &mut SPAWNER.lock(), addr_send);
    let mut usbd = usbd::UsbDriver::new(addr_recv, mouse_callback, key_callback);
    SPAWNER.lock().spawn(async move {
        usbd.main_loop().await
    });

}

pub fn on_xhc_interrupt() {
    xhci::on_xhc_interrupt();
    let mut executor = EXECUTOR.lock();
    while executor.has_next_task() {
        if let Some(Err(e)) = executor.process_next_task().unwrap() {
            println!("Error while running xHCI tasks: {e:?}");
        }
    }
}

fn spawn(future: impl Future<Output = Result<(), XhciError>> + Send + 'static) {
    SPAWNER.lock().spawn(future);
}
