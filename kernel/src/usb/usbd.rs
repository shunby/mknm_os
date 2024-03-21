use core::{
    fmt::{self, Debug, Formatter},
    slice::from_raw_parts,
};

use alloc::{boxed::Box, vec::Vec};
use xhci::{context::EndpointType, ring::trb::{self, command::ConfigureEndpoint}};

use crate::{println, usb::{class::keyboard::KeyboardClass, device::InputContext, spawn, xhci::{push_command, with_dcbaa, with_trf_rings}}};

use super::{
    class::{keyboard::KeyReport, mouse::{MouseClass, MouseReport}}, ring::transfer::{ControlRequestType, SetupData}, runtime::Receiver, xhci::{control_request, XhciError}
};

use bitfield::bitfield;

bitfield! {
    #[derive(Clone,Copy, Debug)]
    #[repr(C)]
    struct DeviceDescriptor_ ([u8]);
    u8;
    length, _: 7,0;
    descriptor_type, _: 15,8;
    bcd_usb, _: 31, 16;
    device_class, _: 39, 32;
    device_sub_class, _: 47, 40;
    device_protocol, _: 55, 48;
    max_packet_size_0, _: 63, 56;
    id_vendor, _: 79, 64;
    id_product, _: 95, 80;
    bcd_device, _: 111, 96;
    i_manufacturer, _: 119, 112;
    i_product, _: 127, 120;
    i_serial_number, _: 135, 128;
    b_num_configurations, _: 143, 136;
}
type DeviceDescriptor = DeviceDescriptor_<[u8; 18]>;

impl Default for DeviceDescriptor {
    fn default() -> Self {
        DeviceDescriptor_([0u8; 18])
    }
}

bitfield! {
    #[derive(Clone,Copy, Debug)]
    #[repr(C)]
    pub struct ConfigurationDescriptor_ ([u8]);
    u8;
    length, _: 7,0;
    descriptor_type, _: 15,8;
    total_length, _: 31, 16;
    num_interfaces, _: 39, 32;
    configuration_value, _: 47, 40;
    i_configuration, _: 55, 48;
    bm_attributes, _: 63, 56;
    max_power, _: 71, 64;
}
pub type ConfigurationDescriptor = ConfigurationDescriptor_<[u8; 9]>;

impl Default for ConfigurationDescriptor {
    fn default() -> Self {
        ConfigurationDescriptor_([0u8; 9])
    }
}

bitfield! {
    #[derive(Clone,Copy, Debug)]
    #[repr(C)]
    pub struct InterfaceDescriptor_ ([u8]);
    u8;
    length, _: 7,0;
    descriptor_type, _: 15,8;
    interface_number, _: 23, 16;
    alternate_setting, _: 31, 24;
    num_endpoints, _: 39, 32;
    interface_class, _: 47, 40;
    interface_subclass, _: 55, 48;
    interface_protocol, _: 63, 56;
    i_interface, _: 71, 64;
}
pub type InterfaceDescriptor = InterfaceDescriptor_<[u8; 9]>;

impl Default for InterfaceDescriptor {
    fn default() -> Self {
        InterfaceDescriptor_([0u8; 9])
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    length: u8,
    descriptor_type: u8,
    endpoint_addr: u8,
    bm_attributes: u8,
    max_packet_size: u16,
    interval: u8,
}

impl EndpointDescriptor {
    pub fn calc_dci(&self) -> usize {
        let addr = self.endpoint_addr;
        (2 * (addr & 0b1111) + (addr >> 7)) as usize
    }
}

bitfield! {
    #[derive(Clone,Copy, Debug)]
    #[repr(C)]
    pub struct HidDescriptor_ ([u8]);
    u8;
    length, _: 7,0;
    descriptor_type, _: 15,8;
    bcd_hid, _: 31, 16;
    country_code, _: 39, 32;
    num_descriptors, _: 47, 40;
    class_descriptor_type, _: 55, 48;
    class_descriptor_length, _: 71, 56;
}
pub type HidDescriptor = HidDescriptor_<[u8; 9]>;

impl Default for HidDescriptor {
    fn default() -> Self {
        HidDescriptor_([0u8; 9])
    }
}

pub struct UnknownDescriptor {
    content: Vec<u8>,
}

impl UnknownDescriptor {
    unsafe fn clone_from_ptr(ptr: *const u8) -> UnknownDescriptor {
        let length = *ptr;

        let content: Vec<u8> = from_raw_parts(ptr, length as usize).to_vec();
        UnknownDescriptor { content }
    }
}

impl Clone for UnknownDescriptor {
    fn clone(&self) -> Self {
        Self {
            content: self.content.to_vec(),
        }
    }
}

impl Debug for UnknownDescriptor {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("UnknownDescriptor {:?}", self.content))
    }
}

pub struct UsbDevice {
    slot_id: usize,
    configs: Vec<UsbConfiguration>,
    config_selected: Option<usize>,
    alternates_selected: Vec<u8>,
}

impl UsbDevice {
    fn new(slot_id: usize, configs: Vec<UsbConfiguration>) -> Self {
        Self {
            slot_id,
            configs,
            config_selected: None,
            alternates_selected: Vec::new(),
        }
    }

    async fn set_configuration(&mut self, config: usize) -> Result<(), XhciError> {
        let conf = &self.configs[config];
        let setup = SetupData {
            request_type: ControlRequestType::SetConfigutation,
            value: conf.configuration_val as u16,
            index: 0,
            length: 0,
        };
        let _ = control_request(self.slot_id, setup, None)?.await.unwrap()?;

        self.config_selected = Some(config);
        self.alternates_selected
            .append(&mut vec![0; conf.interfaces.len()]); // alternate setting defaults to zero

        Ok(())
    }

    async fn set_interface(
        &mut self,
        interface: usize,
        alternate_setting: usize,
    ) -> Result<(), XhciError> {
        let config = &self.configs[self.config_selected.unwrap()];
        let intf = &config.interfaces[interface];
        assert!(intf.alternates.len() > alternate_setting);
        self.alternates_selected[interface] = alternate_setting as u8;

        let setup = SetupData {
            request_type: ControlRequestType::SetInterface,
            value: alternate_setting as u16,
            index: interface as u16,
            length: 0,
        };
        let _ = control_request(self.slot_id, setup, None)?.await.unwrap()?;

        Ok(())
    }

    async fn enable_endpoints(&mut self) -> Result<(), XhciError> {
        let mut input_ctx = InputContext::new(with_dcbaa(|d|d.ctx_size()));
        input_ctx
            .handler_mut()
            .control_mut()
            .set_add_context_flag(0);
        {
            with_dcbaa(|dcbaa| {
                let this = input_ctx.handler_mut().device_mut().slot_mut();
                let other = dcbaa.get_context_at(self.slot_id).handler().slot();
                this.set_route_string(0);
                this.set_root_hub_port_number(other.root_hub_port_number());
                this.set_interrupter_target(0);
                this.set_speed(other.speed());
            });
        }

        let intf_arr = &self.configs[self.config_selected.unwrap()].interfaces;
        let mut context_entries = 1;
        for intf in intf_arr {
            let alt =
                &intf.alternates[self.alternates_selected[intf.interface_num as usize] as usize];
            for ep in &alt.endpoints {
                if let Descriptor::Endpoint(ep) = ep {
                    // endpoint no. =  ep_addr[3..0], direction = ep_addr[7]
                    let ep_addr = ep.endpoint_addr;
                    let direction = ep_addr >> 7;
                    let dci = (2 * (ep_addr & 0b1111) + direction) as usize;

                    input_ctx
                        .handler_mut()
                        .control_mut()
                        .set_add_context_flag(dci);

                    let ep_context = input_ctx.handler_mut().device_mut().endpoint_mut(dci);
                    let transfer_type = ep.bm_attributes & 0b11;
                    ep_context.set_endpoint_type(match (direction, transfer_type) {
                        (0, 1) => EndpointType::IsochOut,
                        (0, 2) => EndpointType::BulkOut,
                        (0, 3) => EndpointType::InterruptOut,
                        (_, 0) => EndpointType::Control,
                        (1, 1) => EndpointType::IsochIn,
                        (1, 2) => EndpointType::BulkIn,
                        (1, 3) => EndpointType::InterruptIn,
                        _ => panic!("illegal endpoint type"),
                    });
                    ep_context.set_max_packet_size(ep.max_packet_size);
                    ep_context.set_max_burst_size(0);
                    let ring_ptr = with_trf_rings(|r|r.init_ring_at(self.slot_id, dci));
                    ep_context.set_tr_dequeue_pointer(ring_ptr);
                    ep_context.set_dequeue_cycle_state();
                    ep_context.set_interval(ep.interval);
                    ep_context.set_max_primary_streams(0);
                    ep_context.set_mult(0);
                    ep_context.set_error_count(3);

                    context_entries = context_entries.max(dci + 1);
                }
            }
        }

        input_ctx
            .handler_mut()
            .device_mut()
            .slot_mut()
            .set_context_entries(context_entries as u8);

        let mut cmd = ConfigureEndpoint::new();
        cmd.set_slot_id(self.slot_id as u8);
        cmd.set_input_context_pointer(input_ctx.get_address());
        println!("{:?}", input_ctx);
        push_command(trb::command::Allowed::ConfigureEndpoint(cmd))?.await.unwrap();
        Ok(())
    }
}

pub struct UsbConfiguration {
    interfaces: Vec<UsbInterface>,
    configuration_val: u8,
    i_configuration: u8,
    bm_attributes: u8,
    max_power: u8,
}

impl UsbConfiguration {
    fn new(desc: &ConfigurationDescriptor, interfaces: Vec<UsbInterface>) -> Self {
        Self {
            interfaces,
            configuration_val: desc.configuration_value(),
            i_configuration: desc.i_configuration(),
            bm_attributes: desc.bm_attributes(),
            max_power: desc.max_power(),
        }
    }
}

pub struct UsbInterfaceAlternate {
    endpoints: Vec<Descriptor>,
    interface_num: u8,
    alternate_setting_num: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    i_interface: u8,
}

impl UsbInterfaceAlternate {
    fn new(desc: InterfaceDescriptor, endpoints: Vec<Descriptor>) -> Self {
        Self {
            endpoints,
            interface_num: desc.interface_number(),
            alternate_setting_num: desc.alternate_setting(),
            class: desc.interface_class(),
            subclass: desc.interface_subclass(),
            protocol: desc.interface_protocol(),
            i_interface: desc.i_interface(),
        }
    }

    pub fn interface_num(&self) -> u8 {
        self.interface_num
    }

    pub fn endpoints(&self) -> &Vec<Descriptor> {
        &self.endpoints
    }
}

pub struct UsbInterface {
    alternates: Vec<UsbInterfaceAlternate>,
    interface_num: u8,
}

impl UsbInterface {
    fn new(interface_num: u8, alternates: Vec<UsbInterfaceAlternate>) -> Self {
        Self {
            interface_num,
            alternates,
        }
    }

    pub fn interface_num(&self) -> u8 {
        self.interface_num
    }
}

#[derive(Debug, Clone)]
pub enum Descriptor {
    Configuration(ConfigurationDescriptor),
    Interface(InterfaceDescriptor),
    Endpoint(EndpointDescriptor),
    Hid(HidDescriptor),
    Unknown(UnknownDescriptor),
}

impl TryFrom<Descriptor> for ConfigurationDescriptor {
    type Error = XhciError;
    fn try_from(value: Descriptor) -> Result<Self, Self::Error> {
        match value {
            Descriptor::Configuration(desc) => Ok(desc),
            _ => Err(XhciError::UnexpectedDescriptor),
        }
    }
}

impl TryFrom<Descriptor> for InterfaceDescriptor {
    type Error = XhciError;
    fn try_from(value: Descriptor) -> Result<Self, Self::Error> {
        match value {
            Descriptor::Interface(desc) => Ok(desc),
            _ => Err(XhciError::UnexpectedDescriptor),
        }
    }
}

pub unsafe fn read_descriptor(ptr: *const u8) -> Option<(Descriptor, *const u8)> {
    let length = *ptr as usize;

    if length == 0 {
        return None;
    }
    let desc_type = *(ptr.add(1));

    let desc = match desc_type {
        2 => Descriptor::Configuration(*(ptr as *const ConfigurationDescriptor)),
        4 => Descriptor::Interface(*(ptr as *const InterfaceDescriptor)),
        5 => Descriptor::Endpoint(*(ptr as *const EndpointDescriptor)),
        33 => Descriptor::Hid(*(ptr as *const HidDescriptor)),
        _ => Descriptor::Unknown(UnknownDescriptor::clone_from_ptr(ptr)),
    };

    Some((desc, ptr.add(length)))
}

fn construct_interface_alternate(
    desc_arr: &[Descriptor],
) -> Option<(UsbInterfaceAlternate, &[Descriptor])> {
    let alt = InterfaceDescriptor::try_from(desc_arr.get(0)?.clone()).ok()?;
    let mut eps: Vec<Descriptor> = Vec::new();

    for i in 1..desc_arr.len() {
        match &desc_arr[i] {
            Descriptor::Interface(_) => {
                let intf = UsbInterfaceAlternate::new(alt, eps);
                return Some((intf, &desc_arr[i..]));
            }
            Descriptor::Configuration(_) => return None,
            desc => {
                eps.push(desc.clone());
            }
        }
    }
    let intf = UsbInterfaceAlternate::new(alt, eps);
    Some((intf, &desc_arr[desc_arr.len()..]))
}

fn construct_interface(mut desc_arr: &[Descriptor]) -> Option<(UsbInterface, &[Descriptor])> {
    let intf_num = InterfaceDescriptor::try_from(desc_arr.get(0)?.clone())
        .ok()?
        .interface_number();
    let mut alts: Vec<UsbInterfaceAlternate> = Vec::new();

    while let Some((alt, remain)) = construct_interface_alternate(desc_arr) {
        if alt.interface_num == intf_num {
            alts.push(alt);
            desc_arr = remain;
        } else {
            let intf = UsbInterface::new(intf_num, alts);
            return Some((intf, desc_arr));
        }
    }
    let intf = UsbInterface::new(intf_num, alts);
    Some((intf, desc_arr))
}

fn construct_configuration(mut desc_arr: &[Descriptor]) -> Option<UsbConfiguration> {
    let conf_desc = ConfigurationDescriptor::try_from(desc_arr.get(0)?.clone()).ok()?;
    desc_arr = &desc_arr[1..];
    let mut intfs: Vec<UsbInterface> = Vec::new();

    while let Some((intf, remain)) = construct_interface(desc_arr) {
        intfs.push(intf);
        desc_arr = remain;
    }

    let conf = UsbConfiguration::new(&conf_desc, intfs);
    Some(conf)
}

pub struct UsbDriver {
    address_device_notifier: Receiver<usize>,
    mouse_callback: Option<Box<dyn Fn(Box<MouseReport>) + Send>>,
    keyboard_callback: Option<Box<dyn Fn(Box<KeyReport>) + Send>>,
}

impl UsbDriver {
    pub fn new(
        address_device_notifier: Receiver<usize>,
        mouse_callback: Box<dyn Fn(Box<MouseReport>) + Send>,
        keyboard_callback: Box<dyn Fn(Box<KeyReport>) + Send>,
    ) -> Self {
        Self {
            address_device_notifier,
            mouse_callback: Some(mouse_callback),
            keyboard_callback: Some(keyboard_callback)
        }
    }

    pub async fn main_loop(&mut self) -> Result<(), XhciError> {
        loop {
            let slot_id = self.address_device_notifier.receive_async().await;
            println!("device configuration: slot_id={slot_id}");

            let dev_desc = self.read_device_descriptor(slot_id).await?;

            let mut confs: Vec<Vec<Descriptor>> = Vec::new();
            for i_conf in 0..dev_desc.b_num_configurations() {
                let conf = self.read_config(slot_id, i_conf as usize, 64).await?;

                for desc in &conf {
                    println!("{desc:?}");
                }

                confs.push(conf);
            }
            let mut dev = self.construct_device(slot_id, confs).await?;

            dev.set_configuration(0).await?;
            dev.enable_endpoints().await?;

            let intf = &dev.configs[0].interfaces[0].alternates[0];

            if self.mouse_callback.is_some()
                && intf.class == 3
                && intf.subclass == 1
                && intf.protocol == 2
            {
                let callback = self.mouse_callback.take().unwrap();
                let mouse = MouseClass::new(slot_id, intf).unwrap();
                mouse.initialize().await?;

                spawn(async move {
                    loop {
                        let (recv, buf) = mouse.subscribe_once()?;
                        if recv.await.unwrap().is_ok() {
                            callback(buf);
                        }
                    }
                })
            } else if self.keyboard_callback.is_some()
                && intf.class == 3
                && intf.subclass == 1
                && intf.protocol == 1
            {
                let callback = self.keyboard_callback.take().unwrap();
                let key = KeyboardClass::new(slot_id, intf).unwrap();
                key.initialize().await?;

                spawn(async move {
                    loop {
                        let (recv, buf) = key.subscribe_once()?;
                        if recv.await.unwrap().is_ok() {
                            callback(buf);
                        }
                    }
                })
            }
        }
    }

    async fn construct_device(
        &mut self,
        slot_id: usize,
        confdesc_arr: Vec<Vec<Descriptor>>,
    ) -> Result<UsbDevice, XhciError> {
        let mut conf_arr: Vec<UsbConfiguration> = Vec::new();
        for conf in confdesc_arr {
            let conf = construct_configuration(&conf).ok_or(XhciError::UnexpectedDescriptor)?;
            conf_arr.push(conf);
        }

        let dev = UsbDevice::new(slot_id, conf_arr);
        Ok(dev)
    }

    async fn read_device_descriptor(
        &mut self,
        slot_id: usize,
    ) -> Result<DeviceDescriptor, XhciError> {
        let mut dev_desc = Box::<DeviceDescriptor>::default();

        let setup = SetupData {
            request_type: ControlRequestType::GetDescriptor,
            value: 0x0100, // Descriptor type = 1 (DEVICE), Descriptor Number = 0
            index: 0,
            length: 18,
        };

        control_request(slot_id, setup, Some(&mut dev_desc.0))?.await.unwrap()?;

        Ok(*dev_desc.as_ref())
    }

    async fn get_config_descriptor(
        slot_id: usize,
        i_conf: usize,
        buf_sz: usize,
    ) -> Result<Result<Vec<u8>, usize>, XhciError> {
        let mut buf = vec![0u8; buf_sz];

        let setup = SetupData {
            request_type: ControlRequestType::GetDescriptor,
            value: 0x0200 | i_conf as u16, // Descriptor type = 2 (CONFIGURATION), Descriptor Number = i_conf
            index: 0,
            length: buf_sz as u16,
        };

        control_request(slot_id, setup, Some(&mut buf))?.await.unwrap()?;

        let total_len = u16::from_le_bytes([buf[2], buf[3]]);
        if (total_len as usize) < buf_sz {
            return Ok(Err(total_len as usize));
        }
        Ok(Ok(buf))
    }

    async fn read_config(
        &mut self,
        slot_id: usize,
        i_conf: usize,
        buf_sz: usize,
    ) -> Result<Vec<Descriptor>, XhciError> {
        let buf = match Self::get_config_descriptor(slot_id, i_conf, buf_sz).await? {
            Ok(desc) => desc,
            Err(total_size) => Self::get_config_descriptor(slot_id, i_conf, total_size)
                .await?
                .unwrap(),
        };
        let mut ptr = buf.as_ptr();
        let mut descs: Vec<Descriptor> = Vec::new();

        let total_len = u16::from_le_bytes([buf[2], buf[3]]) as usize;
        let buf_end = unsafe { buf.as_ptr().add(total_len) };

        while ptr < buf_end {
            let Some((desc, next_ptr)) = (unsafe { read_descriptor(ptr) }) else {
                break;
            };
            println!("{:?}", desc);
            descs.push(desc);
            ptr = next_ptr;
        }

        Ok(descs)
    }
}
