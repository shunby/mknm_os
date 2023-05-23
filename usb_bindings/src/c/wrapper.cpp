#include "wrapper.hpp"
namespace usb {
    extern "C" void set_default_mouse_observer(void (*ptr)(int8_t, int8_t)) {
        HIDMouseDriver::default_observer = ptr;
    }
}