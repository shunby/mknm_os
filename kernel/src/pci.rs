use core::{arch::global_asm, mem::{MaybeUninit, transmute}};

fn make_address(bus: u8, device: u8, function: u8, reg_addr: u8) -> u32 {
    let (bus, device, function, reg_addr) =
        (bus as u32, device as u32, function as u32, reg_addr as u32);
    1 << 31 | bus << 16 | device << 11 | function << 8 | (reg_addr & 0xfc)
}

const CONFIG_ADDRESS: u16 = 0x0cf8;
const CONFIG_DATA: u16 = 0x0cfc;

extern "sysv64" {
    // Read from IO address space
    fn io_in_32(addr: u16) -> u32;
    // Write to IO address space
    fn io_out_32(addr: u16, data: u32);
}
global_asm!(r#" 
io_out_32:
    mov dx, di
    mov eax, esi
    out dx, eax
    ret
io_in_32:
    mov dx, di
    in eax, dx
    ret
"#
);

unsafe fn write_config_address(address: u32) {
    io_out_32(CONFIG_ADDRESS, address);
}

unsafe fn write_config_data(value: u32) {
    io_out_32(CONFIG_DATA, value);
}

unsafe fn read_config_data() -> u32 {
    io_in_32(CONFIG_DATA)
}

#[derive(Debug, Clone)]
pub struct ClassCode {
    pub base: u8,
    pub sub: u8,
    pub interface: u8,
}

impl ClassCode {
    pub fn matches(&self, base: u8, sub: u8, interface: u8) -> bool {
        (self.base, self.sub, self.interface) == (base, sub, interface)
    }
}

#[derive(Debug, Clone)]
pub struct PCIDevice {
    bus: u8,
    device: u8,
    function: u8,
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

    pub unsafe fn read_header_type(&self) -> u8 {
        write_config_address(make_address(self.bus, self.device, self.function, 0x0c));
        ((read_config_data() >> 16) & 0x00ff) as u8
    }

    pub unsafe fn read_vendor_id(&self) -> u16 {
        write_config_address(make_address(self.bus, self.device, self.function, 0x0));
        (read_config_data() & 0xffff) as u16
    }

    pub unsafe fn is_single_function_device(&self) -> bool {
        let header_type = self.read_header_type();
        (header_type & 0x80) == 0
    }

    pub unsafe fn read_class_code(&self) -> ClassCode {
        write_config_address(make_address(self.bus, self.device, self.function, 0x08));
        let reg = read_config_data();
        ClassCode {
            base: ((reg >> 24) & 0xff) as u8,
            sub: ((reg >> 16) & 0xff) as u8,
            interface: ((reg >> 8) & 0xff) as u8,
        }
    }

    pub unsafe fn read_bus_numbers(&self) -> u32 {
        write_config_address(make_address(self.bus, self.device, self.function, 0x18));
        read_config_data()
    }

    pub unsafe fn read_bar(&self, index: u8) -> u64 {
        if index >= 6 {panic!()}
        write_config_address(make_address(self.bus, self.device, self.function, 0x10 + 0x04 * index));
        let bar = read_config_data() as u64;

        // 32bit address
        if (bar & 4) == 0 {
            return bar;
        }

        // 64bit address: use 2 BAR slots
        if index == 5 {panic!()}

        write_config_address(make_address(self.bus, self.device, self.function, 0x10 + 0x04 * (index+1)));
        let bar_upper = read_config_data() as u64;
        (bar_upper << 32) | bar
    }

    pub unsafe fn is_valid(&self) -> bool {
        self.read_vendor_id() != 0xffff
    }
}

pub struct PCIController {
    devices: [MaybeUninit<PCIDevice>; 32],
    num_devices: usize,
}

#[derive(Debug)]
pub enum PCIError {
    DevicesAreFull,
}

impl PCIController {
    pub fn new() -> Self {
        Self {
            devices: unsafe { MaybeUninit::uninit().assume_init() },
            num_devices: 0,
        }
    }

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
