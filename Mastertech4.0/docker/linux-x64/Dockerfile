
# Linux release builder for MasterTech.
#
# Binaries inherit the glibc symbol versions of the machine that LINKS them,
# so building on a rolling-release host (Manjaro, glibc 2.43+) produces
# executables that refuse to start on older distros:
#   version 'GLIBC_2.43' not found (required by ./MasterTech-linux)
# glibc is forward-compatible only: link against an old one and the binary
# runs on everything newer. Ubuntu 22.04 = glibc 2.35, which covers
# Ubuntu 22.04+, Debian 12+, Mint 21+ — anything from 2022 onward.
#
# Built and run by ../../..//build-linux-compat.sh (workspace root).
FROM docker.io/library/ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# clang/lld: linker (the workspace mold config is overridden by the script).
# libclang-dev: bindgen (pipewire-sys/libspa-sys).
# nasm: rav1e x86 asm. perl: vendored openssl-src.
# libxcb*/libxkbcommon/libwayland: winit/xcap/x11rb link deps.
# libgbm/libdrm: wgpu KMS path. libpipewire: xcap wayland screen capture.
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl ca-certificates git pkg-config cmake nasm perl python3 \
    clang lld libclang-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxcb-randr0-dev libxcb-shm0-dev libxcb-composite0-dev libxcb-dri3-dev \
    libxcb-present-dev libxcb-ewmh-dev libxcb-icccm4-dev libxcb-xkb-dev \
    libxcb-xinput-dev libxcb-image0-dev libxcb-keysyms1-dev libxcb-util-dev \
    libxcb-cursor-dev libxcb-xtest0-dev libxcb-xinerama0-dev \
    libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libegl-dev libgl-dev libgles-dev \
    libgbm-dev libdrm-dev libdbus-1-dev libglib2.0-dev ninja-build python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Jammy's pipewire (0.3.48) headers are too old for the libspa/pipewire Rust
# crates (bindgen misses newer struct fields), so build 1.2 from source.
# Link-time only — the lib itself is not shipped with the binary.
RUN pip3 install --no-cache-dir meson \
    && curl -fsSL https://gitlab.freedesktop.org/pipewire/pipewire/-/archive/1.2.7/pipewire-1.2.7.tar.gz | tar xz -C /tmp \
    && cd /tmp/pipewire-1.2.7 \
    && meson setup build --prefix=/usr --libdir=lib/x86_64-linux-gnu \
        -Dauto_features=disabled -Dsession-managers=[] \
        -Dtests=disabled -Dexamples=disabled -Ddocs=disabled -Dman=disabled \
    && ninja -C build install && ldconfig \
    && rm -rf /tmp/pipewire-1.2.7

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH

# Mounted workspace is uid-mapped; keep git usable for build_hash.rs.
RUN git config --global safe.directory '*'

WORKDIR /work
