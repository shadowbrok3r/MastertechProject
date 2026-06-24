# Stage 1: Download and extract mold linker
FROM ubuntu:24.04 AS mold
RUN apt-get update && \
    apt-get install -y wget && \
    rm -rf /var/lib/apt/lists/*
RUN wget -q https://github.com/rui314/mold/releases/download/v2.40.4/mold-2.40.4-x86_64-linux.tar.gz && \
    tar -xzf mold-2.40.4-x86_64-linux.tar.gz && \
    mv mold-2.40.4-x86_64-linux/bin/* /usr/local/bin/

# Stage 2: Build the application (use bookworm to match runtime)
FROM rust:bookworm AS builder
ENV COMPOSE_BAKE=true
WORKDIR /app

# Copy mold linker from previous stage
COPY --from=mold /usr/local/bin/mold /usr/local/bin/mold
COPY --from=mold /usr/local/bin/ld.mold /usr/local/bin/ld.mold

# Install build dependencies (perl is required for vendored OpenSSL build)
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    clang \
    gcc \
    build-essential \
    libclang-dev \
    openssl \
    pkg-config \
    libssl-dev \
    perl \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    update-ca-certificates

# Install Rust nightly (required for surrealdb's unstable features like let chains)
RUN rustup install nightly && rustup default nightly

# Configure cargo to use mold linker
RUN mkdir -p /app/.cargo && \
    printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=/usr/local/bin/mold"]\n' > /app/.cargo/config.toml

# Copy workspace Cargo.toml and modify it to exclude GUI crates
COPY Cargo.toml .
COPY build_hash.rs build_hash.rs
COPY .env .env
RUN sed -i '/^members = \[/,/^\]/c\members = [ "database", "websocket_server2" ]' Cargo.toml

# Copy workspace members needed for build
COPY database/ ./database/
COPY websocket_server2/ ./websocket_server2/

# Build only the websocket_server2 package
RUN cargo build --release --package websocket_server2
RUN ls -la /app/target/release/

# Stage 3: Runtime - use slim image
FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/websocket_server2 /websocket_server2
EXPOSE 8081
ENTRYPOINT ["/websocket_server2"]
