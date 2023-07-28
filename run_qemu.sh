WORK_DIR=/app
SRC_DIR=/app/workspace
IMG_FILE=$WORK_DIR/disk2.img

cd $SRC_DIR/kernel && cargo build
cd $SRC_DIR/bootloader && cargo build

cd $WORK_DIR
qemu-img create -f raw $IMG_FILE 200M
mkfs.fat -n 'Mikanami OS' -s 2 -f 2 -R 32 -F 32 $IMG_FILE

mmd -i $IMG_FILE EFI
mmd -i $IMG_FILE EFI/BOOT
cp $SRC_DIR/bootloader/target/x86_64-unknown-uefi/debug/Loader.efi $WORK_DIR/BOOTX64.EFI
mcopy -i $IMG_FILE $WORK_DIR/BOOTX64.EFI ::EFI/BOOT
mcopy -i $IMG_FILE $SRC_DIR/kernel/kernel.elf ::/

DEVENV_DIR=$WORK_DIR/mikanos-build/devenv

echo "http://localhost:5090/vnc.html"

qemu-system-x86_64 \
    -m 1G \
    -drive if=pflash,format=raw,readonly,file=$DEVENV_DIR/OVMF_CODE.fd \
    -drive if=pflash,format=raw,file=$DEVENV_DIR/OVMF_VARS.fd \
    -drive if=ide,index=0,media=disk,format=raw,file=$IMG_FILE \
    -device nec-usb-xhci,id=xhci \
    -device usb-mouse -device usb-kbd \
    -monitor stdio \
    -vnc :0

