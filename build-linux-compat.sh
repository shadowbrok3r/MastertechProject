#!/usr/bin/env bash
# Builds MasterTech inside an Ubuntu 22.04 container so the release binary's
# glibc floor is 2.35 instead of the build host's (see the Dockerfile).
#
# Usage:  ./build-linux-compat.sh [profile]     (default: release-fast)
# Output: target-linux-compat/<profile>/MasterTech
set -euo pipefail
cd "$(dirname "$0")"

ENGINE="${CONTAINER_ENGINE:-podman}"
IMAGE=mastertech-linux-builder
PROFILE="${1:-release-fast}"

"$ENGINE" build -t "$IMAGE" Mastertech4.0/docker/linux-x64

# Registry/git caches persist across runs; toolchain stays baked in the image.
mkdir -p .cache/cargo-linux-compat

# Env overrides neutralize the workspace .cargo/config.toml linker settings
# (clang + host-path mold) inside the container.
"$ENGINE" run --rm \
    -v "$PWD":/work \
    -v "$PWD/.cache/cargo-linux-compat":/cargo \
    -e CARGO_HOME=/cargo \
    -e CARGO_TARGET_DIR=/work/target-linux-compat \
    -e CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang \
    -e RUSTFLAGS="-C link-arg=-fuse-ld=lld -Awarnings" \
    "$IMAGE" \
    cargo build --profile "$PROFILE" -p MasterTech

BIN="target-linux-compat/$PROFILE/MasterTech"
FLOOR=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)
echo
echo "Built: $BIN"
echo "glibc floor: $FLOOR (runs on any distro with >= this)"
