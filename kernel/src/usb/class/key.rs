use alloc::vec::Vec;

pub enum Modifier {
    LCtrl, LShift, LAlt, LGui, 
    RCtrl, RShift, RAlt, RGui, 
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ModifierSet(u8);

impl ModifierSet {
    pub fn get(&self) -> Vec<Modifier>{
        let mut v = Vec::with_capacity(2);
        if self.l_ctrl() {
            v.push(Modifier::LCtrl);
        }
        if self.l_shift() {
            v.push(Modifier::LShift);
        }
        if self.l_alt() {
            v.push(Modifier::LAlt);
        }
        if self.l_gui() {
            v.push(Modifier::LGui);
        }
        if self.r_ctrl() {
            v.push(Modifier::RCtrl);
        }
        if self.r_shift() {
            v.push(Modifier::RShift);
        }
        if self.r_alt() {
            v.push(Modifier::RAlt);
        }
        if self.r_gui() {
            v.push(Modifier::RGui);
        }
        v
    }

    pub fn l_ctrl(&self) -> bool {
        self.0 & (1) == 1
    }
    pub fn l_shift(&self) -> bool {
        self.0 >> 1 & 1 == 1
    }
    pub fn l_alt(&self) -> bool {
        self.0 >> 2 & 1 == 1
    }
    pub fn l_gui(&self) -> bool {
        self.0 >> 3 & 1 == 1
    }
    pub fn r_ctrl(&self) -> bool {
        self.0 >> 4 & 1 == 1
    }
    pub fn r_shift(&self) -> bool {
        self.0 >> 5 & 1 == 1
    }
    pub fn r_alt(&self) -> bool {
        self.0 >> 6 & 1 == 1
    }
    pub fn r_gui(&self) -> bool {
        self.0 >> 7 & 1 == 1
    }
}