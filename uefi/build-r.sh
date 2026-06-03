#!/bin/sh

# Ensure we stop on the first error
set -e


# Function to display help
show_help() {
    echo "Usage: ./build-r.sh [OPTIONS]"
    echo ""
    echo "Options:"
    echo "  -f, --force    Force rebuild of all steps, even if files already exist."
    echo "  -h, --help     Show this help message."
    exit 0
}

# Check for the flags
FORCE_BUILD=false
while [ "$#" -gt 0 ]; do
    case "$1" in
        -f|--force)
            FORCE_BUILD=true
            shift
            ;;
        -h|--help)
            show_help
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help or -h to see usage."
            exit 1
            ;;
    esac
done

# Build the release
EFI_FILE="target/x86_64-unknown-uefi/release/uefi-app.efi"
if [ "$FORCE_BUILD" = true ] || [ ! -f "$EFI_FILE" ]; then
    echo "Building the release..."
    cargo +nightly build --release --target x86_64-unknown-uefi -Z build-std=std,panic_abort
else
    echo "EFI file already built: $EFI_FILE"
fi

# Copy the release into the ISO folder
ISO_EFI_FILE="iso/EFI/BOOT/BOOTx64.EFI"
if [ "$FORCE_BUILD" = true ] || [ ! -f "$ISO_EFI_FILE" ] || [ "$EFI_FILE" -nt "$ISO_EFI_FILE" ]; then
    echo "Copying EFI file to ISO folder..."
    mkdir -p iso/EFI/BOOT
    cp "$EFI_FILE" "$ISO_EFI_FILE"
else
    echo "EFI file already copied to ISO folder: $ISO_EFI_FILE"
fi

# Create a blank image file and format it as FAT32
EFI_IMG="boot/efi.img"
if [ "$FORCE_BUILD" = true ] || [ ! -f "$EFI_IMG" ]; then
    echo "Creating and formatting EFI image..."
    mkdir -p boot
    dd if=/dev/zero of="$EFI_IMG" bs=1M count=10
    mkfs.vfat "$EFI_IMG"
else
    echo "EFI image already exists: $EFI_IMG"
fi

# Mount the EFI image and copy the bootloader
MOUNT_DIR="/mnt/efi_img"
if [ "$FORCE_BUILD" = true ] || ! sudo mount | grep -q "$EFI_IMG"; then
    echo "Mounting EFI image and copying EFI file..."
    mkdir -p "$MOUNT_DIR"
    sudo mount "$EFI_IMG" "$MOUNT_DIR"
    if [ "$FORCE_BUILD" = true ] || [ ! -f "$MOUNT_DIR/EFI/BOOT/BOOTx64.EFI" ]; then
        sudo mkdir -p "$MOUNT_DIR/EFI/BOOT"
        sudo cp "$ISO_EFI_FILE" "$MOUNT_DIR/EFI/BOOT/"
    else
        echo "EFI file already present in EFI image."
    fi
    sudo umount "$MOUNT_DIR"
else
    echo "EFI image already mounted and updated."
fi


# Explanation of Options:
#     -o my-uefi-app.iso: Specifies the output ISO filename.
#     -eltorito-alt-boot: Indicates the ISO includes an EFI boot partition.
#     -efi-boot-image: Marks the EFI image as bootable.
#     -no-emul-boot: Prevents the use of emulated disk booting.
#     -volid "UEFI_BOOT": Sets the volume ID of the ISO (optional).
#     iso/: The directory containing your ISO filesystem structure.
# Generate the ISO if not already generated or if inputs are newer
ISO_FILE="iso/RustEfiApp.iso"
if [ "$FORCE_BUILD" = true ] || [ ! -f "$ISO_FILE" ] || [ "$ISO_EFI_FILE" -nt "$ISO_FILE" ]; then
    echo "Creating ISO..."
    xorriso -as mkisofs \
        -o "$ISO_FILE" \
        -eltorito-alt-boot \
        -e EFI/BOOT/BOOTx64.EFI \
        -no-emul-boot \
        -volid "UEFI_BOOT" \
        iso/
else
    echo "ISO already created: $ISO_FILE"
fi




# If i want to burn this iso to a flash drive
# sudo dd if=my-uefi-app.iso of=/dev/sdX bs=4M status=progress && sync
