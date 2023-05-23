#  This file is a modified version of the original "kernel/Makefile" in mikanos.
TARGET = libusb.a
OBJS = newlib_support.o pure_virtual.o \
       libcxx_support.o logger.o \
       usb/memory.o usb/device.o usb/xhci/ring.o usb/xhci/trb.o usb/xhci/xhci.o \
       usb/xhci/port.o usb/xhci/device.o usb/xhci/devmgr.o usb/xhci/registers.o \
       usb/classdriver/base.o usb/classdriver/hid.o usb/classdriver/keyboard.o \
       usb/classdriver/mouse.o
DEPENDS = $(join $(dir $(OBJS)),$(addprefix .,$(notdir $(OBJS:.o=.d))))

BASEDIR = $(HOME)/osbook/devenv/x86_64-elf/
LIBS = include/c++/v1 include include/freetype2

CPPFLAGS += $(addprefix $(addprefix -I, $(BASEDIR)), $(LIBS))
CPPFLAGS += -I$(BASEDIR) -I.\
			-nostdlibinc -D__ELF__ -D_LDBL_EQ_DBL -D_GNU_SOURCE -D_POSIX_TIMERS \
			-DEFIAPI='__attribute__((ms_abi))'
CFLAGS   += -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone -fno-inline-functions
CXXFLAGS += -O2 -Wall -g --target=x86_64-elf -ffreestanding -mno-red-zone -fno-inline-functions\
            -fno-exceptions -fno-rtti -std=c++17
LDFLAGS  += -z norelro --static -L$(BASEDIR)/lib

.PHONY: all
all: $(TARGET)

.PHONY: clean
clean:
	rm -rf *.o

libusb.a: $(OBJS) Makefile
	ar rcs libusb.a $(OBJS)

%.o: %.cpp Makefile
	clang++ $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@

.%.d: %.cpp
	clang++ $(CPPFLAGS) $(CXXFLAGS) -MM $< > $@
	$(eval OBJ = $(<:.cpp=.o))
	sed --in-place 's|$(notdir $(OBJ))|$(OBJ)|' $@

%.o: %.c Makefile
	clang $(CPPFLAGS) $(CFLAGS) -c $< -o $@

.%.d: %.c
	clang $(CPPFLAGS) $(CFLAGS) -MM $< > $@
	$(eval OBJ = $(<:.c=.o))
	sed --in-place 's|$(notdir $(OBJ))|$(OBJ)|' $@

%.o: %.asm Makefile
	nasm -f elf64 -o $@ $<

.%.d: %.bin
	touch $@

.PHONY: depends
depends:
	$(MAKE) $(DEPENDS)

-include $(DEPENDS)
