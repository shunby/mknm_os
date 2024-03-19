/// Peripheral Component Interconnect (PCI) デバイス

use core::{mem::{MaybeUninit, transmute, transmute_copy}};
use crate::{asm, println};
use bitfield::bitfield;

fn make_address(bus: u8, device: u8, function: u8, reg_addr: u8) -> u32 {
    let (bus, device, function, reg_addr) =
        (bus as u32, device as u32, function as u32, reg_addr as u32);
    1 << 31 | bus << 16 | device << 11 | function << 8 | (reg_addr & 0xfc)
}

const CONFIG_ADDRESS: u16 = 0x0cf8;
const CONFIG_DATA: u16 = 0x0cfc;


unsafe fn read_confreg(address: u32) -> u32 {
    asm::io_out_32(CONFIG_ADDRESS, address);
    asm::io_in_32(CONFIG_DATA)
}

unsafe fn write_confreg(address: u32, value: u32) {
    asm::io_out_32(CONFIG_ADDRESS, address);
    asm::io_out_32(CONFIG_DATA, value);
}


pub struct PCIController {
    devices: [MaybeUninit<PCIDevice>; 32],
    num_devices: usize,
}

#[derive(Debug, Clone)]
pub struct PCIDevice {
    bus: u8,
    device: u8,
    function: u8,
}

#[derive(Debug, Clone)]
pub struct ClassCode {
    pub base: u8,
    pub sub: u8,
    pub interface: u8,
}

impl PCIController {
    pub fn new() -> Self {
        Self {
            devices: unsafe { MaybeUninit::uninit().assume_init() },
            num_devices: 0,
        }
    }

    /// 全てのPCIバスをスキャンし、接続されたデバイスを記憶する
    pub unsafe fn scan_all_bus(&mut self) -> Result<(), PCIError> {
        let host_bridge = PCIDevice::new(0, 0, 0);

        if host_bridge.is_single_function_device() {
            self.scan_bus(0)?;
        } else {
            for function in 0..8 {
                if PCIDevice::new(0, 0, function).read_vendor_id() != 0xffff {
                    self.scan_bus(function)?;
                }
            }
        }
        Ok(())
    }

    /// 現在記憶しているデバイスを返す
    pub fn get_devices(&self) -> &[PCIDevice] {
        unsafe {transmute(&self.devices[..self.num_devices])}
    }

    pub fn num_devices(&self) -> usize {
        self.num_devices
    }

    fn add_device(&mut self, device: PCIDevice) -> Result<(), PCIError> {
        if self.num_devices == self.devices.len() {
            return Err(PCIError::DevicesAreFull);
        }
        self.devices[self.num_devices] = MaybeUninit::new(device);
        self.num_devices += 1;
        Ok(())
    }

    unsafe fn scan_bus(&mut self, bus: u8) -> Result<(), PCIError> {
        for device in 0..32 {
            if PCIDevice::new(bus, device, 0).is_valid() {
                self.scan_device(bus, device)?;
            }
        }
        Ok(())
    }

    unsafe fn scan_device(&mut self, bus: u8, device: u8) -> Result<(), PCIError> {
        let device_zero = self.scan_function(bus, device, 0)?;

        if device_zero.is_single_function_device() {
            return Ok(());
        }

        for function in 1..8 {
            if PCIDevice::new(bus, device, function).is_valid() {
                self.scan_function(bus, device, function)?;
            }
        }

        Ok(())
    }

    unsafe fn scan_function(
        &mut self,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<PCIDevice, PCIError> {
        let device = PCIDevice::new(bus, device, function);
        self.add_device(device.clone())?;

        let class_code = device.read_class_code();
        if class_code.base == 0x06 && class_code.sub == 0x04 {
            // standard PCI-PCI bridge
            let bus_numbers = device.read_bus_numbers();
            let secondary_bus = ((bus_numbers >> 8) & 0xff) as u8;
            self.scan_bus(secondary_bus)?;
        }
        Ok(device)
    }
}

impl PCIDevice {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        Self {
            bus,
            device,
            function,
        }
    }

    pub fn get_index(&self) -> (u8, u8, u8) {
        (self.bus, self.device, self.function)
    }

    pub unsafe fn read_confreg(&self, reg_addr: u8) -> u32 {
        read_confreg(make_address(self.bus, self.device, self.function, reg_addr))
    }
    
    pub unsafe fn write_confreg(&self, reg_addr: u8, value: u32) {
        write_confreg(make_address(self.bus, self.device, self.function, reg_addr), value);
    }

    pub unsafe fn read_header_type(&self) -> u8 {
        let data = self.read_confreg(0x0c);
        ((data >> 16) & 0x00ff) as u8
    }

    pub unsafe fn read_vendor_id(&self) -> u16 {
        let data = self.read_confreg(0x0);
        (data & 0xffff) as u16
    }

    pub unsafe fn is_single_function_device(&self) -> bool {
        let header_type = self.read_header_type();
        (header_type & 0x80) == 0
    }

    pub unsafe fn read_class_code(&self) -> ClassCode {
        let reg = self.read_confreg(0x08);
        ClassCode {
            base: ((reg >> 24) & 0xff) as u8,
            sub: ((reg >> 16) & 0xff) as u8,
            interface: ((reg >> 8) & 0xff) as u8,
        }
    }

    pub unsafe fn read_bus_numbers(&self) -> u32 {
        self.read_confreg(0x18)
    }

    pub unsafe fn read_bar(&self, index: u8) -> u64 {
        if index >= 6 {panic!()}
        let bar = self.read_confreg(0x10 + 0x04 * index) as u64;

        // 32bit address
        if (bar & 4) == 0 {
            return bar;
        }

        // 64bit address: use 2 BAR slots
        if index == 5 {panic!()}

        let bar_upper = self.read_confreg(0x10 + 0x04 * (index+1)) as u64;
        (bar_upper << 32) | bar
    }

    pub unsafe fn read_cap_ptr(&self) -> u8 {
        (self.read_confreg(0x34) & 0xff) as u8
    }

    pub unsafe fn is_valid(&self) -> bool {
        self.read_vendor_id() != 0xffff
    }
}

impl ClassCode {
    pub fn matches(&self, base: u8, sub: u8, interface: u8) -> bool {
        (self.base, self.sub, self.interface) == (base, sub, interface)
    }
}

#[derive(Debug)]
pub enum PCIError {
    DevicesAreFull,
}


#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum PCICapabilityId {
    MSI = 0x05,
}

#[repr(packed)]
#[repr(C)]
pub struct PCICapabilityHeader {
    cap_id: u8,
    next_cap_ptr: u8,
    _a: u16,
}

#[repr(u8)]
pub enum MSIDestinationMode {
    Fixed = 0b000,
}

bitfield!{
    struct MSICapabilityHeader (u32);
    u8;
    cap_id, _: 7,0;
    next_cap_ptr, _: 15,8;
    msi_enable, set_msi_enable: 16;
    multi_msg_capable, _: 19,17;
    multi_msg_enable, set_multi_msg_enable: 22,20;
    addr_64_capable, _: 23;
    per_vector_mask_capable, _: 24;
}

bitfield!{
    struct MSIMessageAddr (u32);
    u16;
    destination_mode, set_destination_mode: 2;
    redirection_hint, set_redirection_hint: 3;
    destination_id, set_destination_id: 19,12;
    fee, set_FEE: 31, 20;
}

bitfield!{
    struct MSIMessageData (u32);
    u8;
    vector, set_vector: 7,0;
    delivery_mode, set_delivery_mode: 10,8;
    trigger_level, set_trigger_level: 14;
    trigger_mode, set_trigger_mode: 15;
}

fn configure_msi_register(dev: &PCIDevice, cap_addr: u8, apic_id: u8, vector: u8) {
    unsafe {
        let mut header: MSICapabilityHeader = transmute(dev.read_confreg(cap_addr));
        let mut msg_addr: MSIMessageAddr = transmute(dev.read_confreg(cap_addr+4));
        let msg_data_addr = 
            if header.addr_64_capable() {cap_addr + 12} else {cap_addr + 8};
        let mut msg_data: MSIMessageData = transmute(dev.read_confreg(msg_data_addr));
        
        println!("header: addr {}, msi_enable {}, 64bit {}", cap_addr, header.msi_enable() as u8, header.addr_64_capable() as u8);
        println!("msg_addr: destination id {}", msg_addr.destination_id() as u8);
        println!("header: {}", transmute_copy::<_,u32>(&header));
        println!("msg_addr: {}", transmute_copy::<_,u32>(&msg_addr));
        println!("msg_data: addr {}, {}", msg_data_addr, transmute_copy::<_,u32>(&msg_data));
        println!("----------------------------------------------------");
        header.set_msi_enable(true);
        msg_addr.set_destination_id(apic_id as u16);
        msg_addr.set_FEE(0xfee);
        msg_data.set_delivery_mode(0);
        msg_data.set_trigger_mode(true);
        msg_data.set_trigger_level(true);
        msg_data.set_vector(vector);
        msg_addr.set_redirection_hint(false);
        
        dev.write_confreg(cap_addr, transmute(header));
        dev.write_confreg(cap_addr + 4, transmute(msg_addr));
        dev.write_confreg(msg_data_addr, transmute(msg_data));

        
        let header: MSICapabilityHeader = transmute(dev.read_confreg(cap_addr));
        let msg_addr: MSIMessageAddr = transmute(dev.read_confreg(cap_addr+4));
        let msg_data: MSIMessageData = transmute(dev.read_confreg(msg_data_addr));
        
        println!("header: addr {}, msi_enable {}, 64bit {}", cap_addr, header.msi_enable() as u8, header.addr_64_capable() as u8);
        println!("msg_addr: destination id {}", msg_addr.destination_id() as u8);
        println!("header: {}", transmute_copy::<_,u32>(&header));
        println!("msg_addr: {}", transmute_copy::<_,u32>(&msg_addr));
        println!("msg_data: {}", transmute_copy::<_,u32>(&msg_data));
    }
}

pub fn configure_msi_fixed_destination(
        dev: &PCIDevice, apic_id: u8, vector: u8) {
    unsafe {
        let mut cap_addr = dev.read_cap_ptr();
        while cap_addr != 0 {
            let header: PCICapabilityHeader = transmute(dev.read_confreg(cap_addr));
            
            println!("!header: addr {}, cap_id: {}", cap_addr, header.cap_id);
            println!("!header: {}", transmute_copy::<_,u32>(&header));
       
            if header.cap_id == PCICapabilityId::MSI as u8 {
                configure_msi_register(dev, cap_addr, apic_id, vector);
                return;
            }
            cap_addr = header.next_cap_ptr;
        }
    }
}
