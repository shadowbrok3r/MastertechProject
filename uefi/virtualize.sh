#!/bin/sh

ISO_FILE="iso/RustEfiApp.iso"
if [ -f "$ISO_FILE" ]; then
  qemu-system-x86_64 \
      -bios /usr/share/edk2/x64/OVMF_CODE.fd \
      -cdrom "$ISO_FILE" \
      -boot d \
      -d int -D qemu_debug.log
  echo "$ISO_FILE does not exist"
fi
