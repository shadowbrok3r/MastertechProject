# syntax=docker/dockerfile:1.7
# MtechServer2.0 WASM frontend image. Build from the repo root:
#   docker build -t mtechserver .
#
# Whole-repo context: every workspace member is present so `cargo metadata`
# and trunk resolve cleanly. No manifest edits. wasm build flags + build-std
# come from the committed MtechServer2.0/.cargo/config.toml.

# Build stage: compiles the WASM frontend into dist/.
FROM --platform=$BUILDPLATFORM rustlang/rust:nightly AS build
WORKDIR /app

RUN rustup target add wasm32-unknown-unknown \
 && rustup target add wasm32-wasip1 \
 && rustup component add rust-src \
 && apt-get update \
 && apt-get install -y --no-install-recommends clang gcc build-essential libclang-dev openssl wget ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && update-ca-certificates

# Trunk version pinned to match the CI `trunk` job.
RUN wget -qO- https://github.com/trunk-rs/trunk/releases/download/v0.22.0-beta.1/trunk-x86_64-unknown-linux-musl.tar.gz | tar -xzf- \
 && mv trunk /usr/local/bin/trunk

COPY . .

# Commit stamp for build_hash.rs; .git/ is not in the build context.
ARG GIT_SHA=""
ENV BUILD_GIT_SHA=$GIT_SHA

WORKDIR /app/MtechServer2.0

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/MtechServer2.0/target,sharing=locked \
    trunk build --release --locked

RUN /app/deploy/precompress.sh dist

# nginx with the site config and no content.
FROM nginx:1.27-alpine AS base
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080

# CI target: serves a precompressed dist from `--build-context dist=./MtechServer2.0/dist`.
FROM base AS prebuilt
COPY --from=dist . /usr/share/nginx/html
RUN nginx -t

# Default target: serves the dist built from source.
FROM base AS runtime
COPY --from=build /app/MtechServer2.0/dist /usr/share/nginx/html
RUN nginx -t
