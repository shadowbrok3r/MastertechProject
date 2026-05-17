# syntax=docker/dockerfile:1.6
#
# `plugin_builder` worker image. Built from the repo root:
#     docker build -f plugin_builder/Dockerfile -t mtech-plugin-builder .
#
# Two-stage so the runtime image is just `rust:1-slim` (we still need
# cargo + the wasm32-wasip1 toolchain at runtime — the *point* of this
# service is to compile plugins on demand).

# ── build stage ─────────────────────────────────────────────────────
FROM rust:1-slim AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-wasip1

WORKDIR /src
# Copy the whole workspace so `cargo build -p plugin_builder` can resolve
# `[workspace.dependencies]` from the root Cargo.toml. The plugin_builder
# crate itself has a tiny dep tree, so the build is fast despite the
# big workspace.
COPY . .

RUN cargo build -p plugin_builder --release

# ── runtime stage ──────────────────────────────────────────────────
FROM rust:1-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add wasm32-wasip1

COPY --from=build /src/target/release/plugin_builder /usr/local/bin/plugin_builder

# Persistent caches mounted as volumes by compose. CARGO_TARGET_DIR is
# set per-build inside the worker, but a shared $CARGO_HOME means the
# registry/git index cache survives across builds and plugins.
ENV CARGO_HOME=/var/cache/cargo \
    BUILD_WORKER_SCRATCH_ROOT=/tmp/mtech-builder \
    BUILD_WORKER_TARGET_CACHE_ROOT=/var/cache/mtech-builder \
    RUST_LOG=plugin_builder=info

VOLUME ["/var/cache/cargo", "/var/cache/mtech-builder"]

ENTRYPOINT ["/usr/local/bin/plugin_builder"]
