#!/usr/bin/env bash
#
# build-iso.sh — build the Mastertech UEFI app and package it as a
# UEFI-bootable ISO suitable for dropping onto a Ventoy drive.
#
# Pipeline:
#   1. (optional) install build deps: mtools + xorriso (libisoburn)
#   2. cargo +nightly build --release  (x86_64-unknown-uefi, build-std)
#   3. pack a FAT16 EFI System Partition image containing
#      /EFI/BOOT/BOOTX64.EFI  (via mtools — no root, no mounting)
#   4. wrap it in an El-Torito EFI ISO via xorriso
#
# Output: dist/MastertechUEFI.iso  — copy this onto your Ventoy drive.
#
# Usage:
#   ./build-iso.sh            build the ISO
#   ./build-iso.sh --run      build, then smoke-test the ISO in QEMU+OVMF
#   ./build-iso.sh --clean    remove build/ and dist/ first
#   ./build-iso.sh -h|--help  show this help

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

TARGET="x86_64-unknown-uefi"
PKG="uefi-app"
VOLID="MTECH_UEFI"
# Cargo target dir: this crate is a member of the parent workspace, so the
# artifact lands in the workspace target/, not ./target.
EFI_BIN="$(cd .. && pwd)/target/${TARGET}/release/${PKG}.efi"

BUILD_DIR="$SCRIPT_DIR/build"
STAGE="$BUILD_DIR/isoroot"
EFI_IMG="$STAGE/efi.img"
DIST_DIR="$SCRIPT_DIR/dist"
OUT_ISO="$DIST_DIR/MastertechUEFI.iso"

c_info()  { printf '\033[1;35m[*]\033[0m %s\n' "$*"; }   # magenta, on-brand
c_ok()    { printf '\033[1;36m[+]\033[0m %s\n' "$*"; }   # cyan
c_err()   { printf '\033[1;31m[!]\033[0m %s\n' "$*" >&2; }

RUN_QEMU=false
DO_CLEAN=false
for arg in "$@"; do
  case "$arg" in
    --run)        RUN_QEMU=true ;;
    --clean)      DO_CLEAN=true ;;
    -h|--help)    grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) c_err "unknown option: $arg (try --help)"; exit 1 ;;
  esac
done

# --- 1. dependencies -------------------------------------------------------
ensure_deps() {
  local missing_pkgs=()
  command -v mformat >/dev/null 2>&1 || missing_pkgs+=("mtools")
  command -v xorriso >/dev/null 2>&1 || missing_pkgs+=("libisoburn")

  if [ "${#missing_pkgs[@]}" -ne 0 ]; then
    c_info "Installing missing build deps: ${missing_pkgs[*]}"
    if command -v pacman >/dev/null 2>&1; then
      sudo pacman -S --needed --noconfirm "${missing_pkgs[@]}"
    else
      c_err "Please install these packages manually: ${missing_pkgs[*]}"
      c_err "  (mtools provides mformat/mcopy; xorriso is in 'libisoburn' on Arch/Manjaro)"
      exit 1
    fi
  fi

  rustup toolchain list 2>/dev/null | grep -q '^nightly' || {
    c_info "Installing rust nightly toolchain"
    rustup toolchain install nightly
  }
  rustup component list --toolchain nightly --installed 2>/dev/null | grep -q '^rust-src' || {
    c_info "Adding rust-src component to nightly"
    rustup component add rust-src --toolchain nightly
  }
}

# --- main ------------------------------------------------------------------
if $DO_CLEAN; then
  c_info "Cleaning build/ and dist/"
  rm -rf "$BUILD_DIR" "$DIST_DIR"
fi

ensure_deps

c_info "Building release .efi ($TARGET)"
cargo +nightly build --release --target "$TARGET" -Z build-std=std,panic_abort

[ -f "$EFI_BIN" ] || { c_err "expected EFI binary not found: $EFI_BIN"; exit 1; }
c_ok "built $(du -h "$EFI_BIN" | cut -f1)  ->  $EFI_BIN"

# --- 3. pack the FAT EFI System Partition image ----------------------------
c_info "Packing FAT EFI image"
rm -rf "$BUILD_DIR"
mkdir -p "$STAGE/EFI/BOOT" "$DIST_DIR"

# Size the image to the binary + 6 MiB slack, minimum 16 MiB (valid FAT16).
efi_bytes="$(stat -c%s "$EFI_BIN")"
img_mb=$(( (efi_bytes + 1048575) / 1048576 + 6 ))
[ "$img_mb" -lt 16 ] && img_mb=16

truncate -s "${img_mb}M" "$EFI_IMG"
mkfs.vfat -F 16 -n "$VOLID" "$EFI_IMG" >/dev/null
mmd   -i "$EFI_IMG" ::/EFI ::/EFI/BOOT
mcopy -i "$EFI_IMG" "$EFI_BIN" ::/EFI/BOOT/BOOTX64.EFI

# Also place loose files in the ISO9660 tree so the disc is browsable.
cp "$EFI_BIN" "$STAGE/EFI/BOOT/BOOTX64.EFI"
c_ok "EFI image: ${img_mb} MiB FAT16, /EFI/BOOT/BOOTX64.EFI"

# --- 4. build the bootable ISO ---------------------------------------------
c_info "Building ISO via xorriso"
xorriso -as mkisofs \
  -volid "$VOLID" \
  -J -joliet-long -R \
  -e efi.img -no-emul-boot \
  -append_partition 2 0xef "$EFI_IMG" \
  -appended_part_as_gpt \
  -o "$OUT_ISO" \
  "$STAGE" 2>&1 | sed 's/^/    /'

c_ok "ISO ready: $OUT_ISO ($(du -h "$OUT_ISO" | cut -f1))"
echo
c_info "Copy it to your Ventoy drive's data partition, e.g.:"
echo "    cp '$OUT_ISO' /run/media/\$USER/Ventoy/"

# --- 5. optional smoke test ------------------------------------------------
if $RUN_QEMU; then
  OVMF_CODE="/usr/share/edk2/x64/OVMF_CODE.4m.fd"
  OVMF_VARS_SRC="/usr/share/edk2/x64/OVMF_VARS.4m.fd"
  if [ ! -f "$OVMF_CODE" ]; then
    c_err "OVMF not found at $OVMF_CODE — skipping QEMU test (install edk2-ovmf)"
    exit 0
  fi
  c_info "Smoke-testing ISO in QEMU+OVMF (window opens; close it or press q in-app)"
  vars="$(mktemp)"; cp "$OVMF_VARS_SRC" "$vars"
  qemu-system-x86_64 -machine q35 -m 2048 \
    -drive if=pflash,format=raw,unit=0,readonly=on,file="$OVMF_CODE" \
    -drive if=pflash,format=raw,unit=1,file="$vars" \
    -cdrom "$OUT_ISO" -boot d \
    -net none -rtc base=utc || true
  rm -f "$vars"
fi
