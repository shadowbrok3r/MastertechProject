#!/usr/bin/env bash
# One-time: fetch the iPXE boot binaries + wimboot into the pxe-bench roots.
# Run on the Ubuntu bench host. WinPE media (boot.wim etc.) is built separately
# on Windows and copied into $HTTP/media — see UBUNTU-SETUP.md.
#
#   ./stage-payload.sh [PXE_ROOT]   (default /srv/mtech/pxe)
set -euo pipefail

PXE_ROOT="${1:-/srv/mtech/pxe}"
TFTP="$PXE_ROOT/tftp"
HTTP="$PXE_ROOT/http"
mkdir -p "$TFTP" "$HTTP/media"

# UEFI binaries are under x86_64-efi/ — NOT the site root (root has only the
# BIOS images + undionly.kpxe + signed shims).
echo "==> iPXE binaries -> $TFTP"
curl -fSL https://boot.ipxe.org/undionly.kpxe          -o "$TFTP/undionly.kpxe"  # BIOS PXE chainload
curl -fSL https://boot.ipxe.org/x86_64-efi/snponly.efi -o "$TFTP/snponly.efi"    # x64 UEFI (firmware SNP — default)
curl -fSL https://boot.ipxe.org/x86_64-efi/ipxe.efi    -o "$TFTP/ipxe.efi"       # x64 UEFI (iPXE drivers — fallback)

# wimboot's canonical download is the GitHub latest-release asset (boot.ipxe.org/wimboot 404s).
echo "==> wimboot -> $HTTP"
curl -fSL https://github.com/ipxe/wimboot/releases/latest/download/wimboot -o "$HTTP/wimboot"

echo
echo "Staged. Remaining step: copy your WinPE media tree into:"
echo "  $HTTP/media/Boot/BCD"
echo "  $HTTP/media/Boot/boot.sdi"
echo "  $HTTP/media/sources/boot.wim"
echo "Then: sudo systemctl restart pxe-bench"
