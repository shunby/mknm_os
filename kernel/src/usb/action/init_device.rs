use alloc::vec::Vec;

use xhci::{context::{EndpointHandler, SlotHandler}, ring::trb::{command::{AddressDevice, Allowed, EnableSlot}, event::{CompletionCode, PortStatusChange}}, Registers};

use crate::usb::{device::{ContextSize, InputContext}, runtime::{Receiver, Sender}, xhci::{push_command, with_dcbaa, with_regs, with_trf_rings, LinearMapper, XhciError}};

pub struct DeviceInitAction {
    current_port: Option<usize>,
    waiting_port: Vec<usize>,
    status_change: Receiver<PortStatusChange>,
    address_device_listener: Sender<usize>
}

impl DeviceInitAction {
    pub fn new(status_change: Receiver<PortStatusChange>, address_device_listener: Sender<usize>) -> Self {
        Self { current_port: None, waiting_port: Vec::new(), status_change, address_device_listener }
    }

    pub async fn main_loop(&mut self) {
        let num_ports = with_regs(|r|r.capability.hcsparams1.read_volatile().number_of_ports()) as usize;
        for port_id in 0..num_ports {
            if with_regs(|r|r.port_register_set.read_volatile_at(port_id).portsc.current_connect_status()) {
                with_regs(|r|r.port_register_set.update_volatile_at(port_id, |p|{
                    p.portsc.clear_connect_status_change();
                }));
                self.init_device_async(port_id as usize).await;
            }
        }

        loop {
            let event = self.status_change.receive_async().await;
            let port_id = (event.port_id() - 1) as usize;

            let portsc = with_regs(|r|r.port_register_set.read_volatile_at(port_id).portsc);

            if portsc.connect_status_change() && portsc.current_connect_status() {
                clear_csc(port_id);
                self.waiting_port.push(port_id);
            } else if portsc.port_reset_change() {
                clear_port_reset(port_id);
                if self.current_port.is_some_and(|p|p == port_id) {
                    self.init_device_async(port_id).await;
                }
            }

            if self.current_port.is_none() && !self.waiting_port.is_empty() {
                self.current_port = Some(self.waiting_port.remove(0));
                self.reset_port(self.current_port.unwrap());
            }
        }
    }

    fn reset_port(&self, port_id: usize) {
        let portsc = with_regs(|r|r.port_register_set.read_volatile_at(port_id).portsc);
        println!(
            "resetting port {port_id}(CCS={}, CSC={})",
            portsc.current_connect_status(),
            portsc.connect_status_change()
        );
        set_port_reset(port_id);
    }
    
    async fn init_device_async(&mut self, port_id: usize) -> Result<(), XhciError> {
        println!("Addressing device at port={port_id}");
        let slot_id = self.enable_slot_async().await?;

        self.address_device_async(port_id, slot_id, false).await?;
        // wait_for(200);

        println!("Addressing finished: port={port_id}, slot={slot_id}");

        self.address_device_listener.send(slot_id);

        Ok(())
    }

    async fn enable_slot_async(&self) -> Result<usize, XhciError> {
        let recv = push_command(Allowed::EnableSlot(EnableSlot::new()))?;
        Ok(recv.await.unwrap().slot_id() as usize)
    }

    
    async fn address_device_async(
        &self,
        port_id: usize,
        slot_id: usize,
        bsr: bool,
    ) -> Result<(), XhciError> {
        with_dcbaa(|d|d.init_context_at(slot_id));
        let trf_ring_ptr = with_trf_rings(|r|r.init_ring_at(slot_id, 1));

        let input_ctx = with_regs(|r|{
            prepare_input_ctx_for_address_device(port_id, slot_id, trf_ring_ptr, with_dcbaa(|d|d.ctx_size()), r)
        });

        let mut trb = AddressDevice::new();
        trb.set_input_context_pointer(input_ctx.get_address())
            .set_slot_id(slot_id as u8);
        if bsr {
            trb.set_block_set_address_request();
        }

        let result = push_command(Allowed::AddressDevice(trb))?.await.unwrap();

        let success = result
            .completion_code()
            .map_or(false, |code| matches!(code, CompletionCode::Success));

        if success {
            drop(input_ctx);
            Ok(())
        } else {
            Err(XhciError::AddressDeviceCommandFailed(result))
        }
    }

}

fn clear_csc(port_id: usize) {
    with_regs(|r|r.port_register_set.update_volatile_at(port_id, |p|{
        let p = &mut p.portsc;
        p.set_0_connect_status_change();
        p.set_0_over_current_change();
        p.set_0_port_config_error_change();
        p.set_0_port_enabled_disabled();
        p.set_0_port_enabled_disabled_change();
        p.set_0_port_link_state_change();
        p.set_0_port_reset_change();
        p.set_0_warm_port_reset_change();

        p.clear_connect_status_change();
    }));
}

fn clear_port_reset(port_id: usize) {
    with_regs(|r|r.port_register_set.update_volatile_at(port_id, |p|{
        let p = &mut p.portsc;
        p.set_0_connect_status_change();
        p.set_0_over_current_change();
        p.set_0_port_config_error_change();
        p.set_0_port_enabled_disabled();
        p.set_0_port_enabled_disabled_change();
        p.set_0_port_link_state_change();
        p.set_0_port_reset_change();
        p.set_0_warm_port_reset_change();

        p.clear_port_reset_change();
    }));
}

fn set_port_reset(port_id: usize) {
    with_regs(|r|r.port_register_set.update_volatile_at(port_id, |p|{
        let p = &mut p.portsc;
        p.set_0_connect_status_change();
        p.set_0_over_current_change();
        p.set_0_port_config_error_change();
        p.set_0_port_enabled_disabled();
        p.set_0_port_enabled_disabled_change();
        p.set_0_port_link_state_change();
        p.set_0_port_reset_change();
        p.set_0_warm_port_reset_change();

        p.set_port_reset();
    }));
}

fn prepare_input_ctx_for_address_device(
    port_id: usize,
    slot_id: usize,
    deque_ptr: u64,
    ctx_size: ContextSize, 
    regs: &mut Registers<LinearMapper>
) -> InputContext {
    /* 4.3.3 Device Slot Initialization */
    let mut input_ctx = InputContext::new(ctx_size);

    {
        let control = input_ctx.handler_mut().control_mut();
        control.set_add_context_flag(0);
        control.set_add_context_flag(1);
    }
    config_slot_context(input_ctx.handler_mut().device_mut().slot_mut(), port_id, regs);
    config_default_control_pipe(
        input_ctx.handler_mut().device_mut().endpoint_mut(1),
        port_id,
        deque_ptr,
        regs
    );

    input_ctx
}


fn config_slot_context(slot: &mut dyn SlotHandler, port_id: usize, regs: &mut Registers<LinearMapper>) {
    let speed = regs.port_register_set.read_volatile_at(port_id).portsc.port_speed();
    slot.set_root_hub_port_number(port_id as u8 + 1);
    slot.set_route_string(0);
    slot.set_context_entries(1);
    slot.set_speed(speed);
}

fn config_default_control_pipe(
    pipe: &mut dyn EndpointHandler,
    port_id: usize,
    tr_deque_ptr: u64,
    regs: &mut Registers<LinearMapper>
) {
    let speed = regs.port_register_set.read_volatile_at(port_id).portsc.port_speed();
    let max_packet_size = match speed {
        1 => 64,  // full-speed
        2 => 8,   // Low-speed
        3 => 64,  // High-speed
        4 => 512, //SuperSpeed
        _ => 8,
    };

    pipe.set_endpoint_type(xhci::context::EndpointType::Control);
    pipe.set_max_packet_size(max_packet_size);
    pipe.set_max_burst_size(0);

    // xhci crate の仕様上、tr_deque_pointer を deque_cycle_state より先に設定する必要がある
    pipe.set_tr_dequeue_pointer(tr_deque_ptr);
    pipe.set_dequeue_cycle_state();

    pipe.set_interval(0);
    pipe.set_max_primary_streams(0);
    pipe.set_mult(0);
    pipe.set_error_count(3);
}
