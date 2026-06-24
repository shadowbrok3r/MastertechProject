# syntax=docker/dockerfile:1.7
# plugin_builder worker image. Build from the repo root:
#   docker build -f plugin_builder/Dockerfile -t mtech-plugin-builder .
#
# Whole-repo context + cargo-chef. The runtime keeps cargo + the wasm32-wasip1
# toolchain because the service compiles plugins on demand.

# ── chef base: toolchain, build deps, cargo-chef ──────────────────────
FROM rust:1-slim AS chef
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev gcc ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-wasip1
RUN cargo install cargo-chef --locked
WORKDIR /src

# ── planner: derive the dependency recipe from the full workspace ─────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── builder: cook deps into a cached layer, then build only this crate ──
FROM chef AS build
# database/build.rs runs during `cook`; it include!()s ../build_hash.rs and reads ../.env.
COPY build_hash.rs build_hash.rs
COPY .env .env
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release -p plugin_builder --recipe-path recipe.json
COPY . .
RUN cargo build --release -p plugin_builder --locked

# ── runtime ───────────────────────────────────────────────────────────
FROM rust:1-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add wasm32-wasip1

COPY --from=build /src/target/release/plugin_builder /usr/local/bin/plugin_builder

# Persistent caches mounted as volumes by compose; shared CARGO_HOME keeps the
# registry/git index warm across plugin builds.
ENV CARGO_HOME=/var/cache/cargo \
    BUILD_WORKER_SCRATCH_ROOT=/tmp/mtech-builder \
    BUILD_WORKER_TARGET_CACHE_ROOT=/var/cache/mtech-builder \
    RUST_LOG=plugin_builder=info

VOLUME ["/var/cache/cargo", "/var/cache/mtech-builder"]

ENTRYPOINT ["/usr/local/bin/plugin_builder"]
