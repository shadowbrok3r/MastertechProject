# syntax=docker/dockerfile:1.7
# websocket_server2 image. Build from the repo root:
#   docker build -f websocket_server2/Dockerfile -t websocket_server2 .
#
# Whole-repo context + cargo-chef. `cargo build -p websocket_server2` compiles
# only this crate's closure; other workspace members are parsed, never compiled.

# ── mold linker ───────────────────────────────────────────────────────
FROM ubuntu:24.04 AS mold
RUN apt-get update \
 && apt-get install -y --no-install-recommends wget ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN wget -q https://github.com/rui314/mold/releases/download/v2.40.4/mold-2.40.4-x86_64-linux.tar.gz \
 && tar -xzf mold-2.40.4-x86_64-linux.tar.gz \
 && mv mold-2.40.4-x86_64-linux/bin/* /usr/local/bin/

# ── chef base: toolchain, build deps, cargo-chef, mold, cargo config ──
FROM rust:bookworm AS chef
RUN rustup install nightly && rustup default nightly
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      clang gcc build-essential libclang-dev pkg-config libssl-dev openssl perl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && update-ca-certificates
RUN cargo install cargo-chef --locked
COPY --from=mold /usr/local/bin/mold /usr/local/bin/mold
COPY --from=mold /usr/local/bin/ld.mold /usr/local/bin/ld.mold
WORKDIR /app
RUN mkdir -p /app/.cargo \
 && printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=/usr/local/bin/mold"]\n' > /app/.cargo/config.toml

# ── planner: derive the dependency recipe from the full workspace ─────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── builder: cook deps into a cached layer, then build only this crate ──
FROM chef AS builder
COPY build_hash.rs build_hash.rs
COPY .env .env
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p websocket_server2 --recipe-path recipe.json
COPY . .
RUN cargo build --release -p websocket_server2 --locked
RUN ls -la /app/target/release/

# ── runtime ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/websocket_server2 /websocket_server2
EXPOSE 8081
ENTRYPOINT ["/websocket_server2"]
