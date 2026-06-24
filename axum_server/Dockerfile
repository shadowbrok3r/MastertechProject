# Stage 1: Download and extract mold linker
FROM ubuntu:24.04 AS mold
RUN apt-get update && \
    apt-get install -y wget && \
    rm -rf /var/lib/apt/lists/*
RUN wget -q https://github.com/rui314/mold/releases/download/v2.40.4/mold-2.40.4-x86_64-linux.tar.gz && \
    tar -xzf mold-2.40.4-x86_64-linux.tar.gz && \
    mv mold-2.40.4-x86_64-linux/bin/* /usr/local/bin/

# Stage 2: Build the application
FROM rust:bookworm AS builder
ENV COMPOSE_BAKE=true
WORKDIR /app

COPY --from=mold /usr/local/bin/mold /usr/local/bin/mold
COPY --from=mold /usr/local/bin/ld.mold /usr/local/bin/ld.mold

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

RUN rustup install nightly && rustup default nightly

RUN mkdir -p /app/.cargo && \
    printf '[target.x86_64-unknown-linux-gnu]\nlinker = "clang"\nrustflags = ["-C", "link-arg=-fuse-ld=/usr/local/bin/mold"]\n' > /app/.cargo/config.toml

# Slim the workspace to the two crates we need so cargo metadata doesn't
# error on missing native-only members (Mastertech4.0, displays, etc.).
COPY Cargo.toml .
COPY build_hash.rs build_hash.rs
COPY .env .env
RUN sed -i '/^members = \[/,/^\]/c\members = [ "database", "axum_server" ]' Cargo.toml

COPY database/ ./database/
COPY axum_server/ ./axum_server/

RUN cargo build --release --package axum_server
RUN ls -la /app/target/release/

# Stage 3: Minimal runtime image
FROM debian:bookworm-slim
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/axum_server /axum_server
EXPOSE 8082
ENTRYPOINT ["/axum_server"]
