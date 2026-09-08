# syntax=docker/dockerfile:1.7
# admin-agent image. Build from the repo root:
#   docker build -f admin-agent/Dockerfile -t admin_agent .
#
# Same whole-repo context + cargo-chef pattern as axum_server. Unlike that
# crate, this one links `displays`, which on Linux pulls eframe (x11/wayland),
# rfd (GTK3) and backdrop-blur-egui (glow/GL) even though `headless::run`
# never opens a window — so both stages carry the egui system libraries.

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
      libgtk-3-dev libxkbcommon-dev libwayland-dev \
      libgl1-mesa-dev libegl1-mesa-dev \
      libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
      libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
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

# ── builder: cook deps into a cached layer, then build only admin-agent ──
FROM chef AS builder
# database/build.rs runs during `cook`; it include!()s ../build_hash.rs and reads ../.env.
COPY build_hash.rs build_hash.rs
COPY .env .env
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release -p admin-agent --recipe-path recipe.json
COPY . .
RUN cargo build --release -p admin-agent --locked
RUN ls -la /app/target/release/

# ── runtime ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates \
      libgtk-3-0 libxkbcommon0 libwayland-client0 \
      libgl1 libegl1 \
      libx11-6 libxcursor1 libxrandr2 libxi6 libxcb1 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/admin-agent /admin-agent
# Plugin MCP (Streamable HTTP) and the zeroclaw channel reply listener.
EXPOSE 9004 9015
ENTRYPOINT ["/admin-agent"]
