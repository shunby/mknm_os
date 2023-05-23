#include "usb/xhci/xhci.hpp"
#include "usb/classdriver/mouse.hpp"
#include "logger.hpp"
namespace usb {
    extern "C" void set_default_mouse_observer(void (*ptr)(int8_t, int8_t));
}