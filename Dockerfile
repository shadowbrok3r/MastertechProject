# syntax=docker/dockerfile:1.7
# MtechServer2.0 WASM frontend image. Build from the repo root:
#   docker build -t mtechserver .
#
# The frontend is compiled HERE, at image build time — not at container start.
# The runtime stage carries only the generated `dist/` behind nginx, so:
#   * container start is ~1s instead of a ~25 minute trunk build
#   * pods no longer need crates.io reachable in order to boot
#   * an image digest pins the exact bytes served (BUILD_HASH freezes here)
#   * the final image holds no source tree, no toolchain and no .env
#
# Whole-repo context: every workspace member is present so `cargo metadata`
# and trunk resolve cleanly. No manifest edits. wasm build flags + build-std
# come from the committed MtechServer2.0/.cargo/config.toml.

# ---------------------------------------------------------------------------
# Build stage — compile the WASM frontend
# ---------------------------------------------------------------------------
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

# `.dockerignore` excludes `.git/`, so build_hash.rs cannot shell out to git and
# would stamp every build `nogit` — no running pod traceable to a commit. CI
# passes the real SHA; build_hash.rs falls back to BUILD_GIT_SHA when git fails.
ARG GIT_SHA=""
ENV BUILD_GIT_SHA=$GIT_SHA

WORKDIR /app/MtechServer2.0

# `--locked` makes the committed Cargo.lock authoritative. The .dockerignore
# header has always claimed Docker builds use it; now that is actually true.
#
# The cache mounts only speed up repeated builds on one machine — BuildKit does
# NOT export them via `cache-to: type=gha`. CI therefore does not use this stage
# at all; it builds the `prebuilt` target below from the `trunk` job's artifact.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/MtechServer2.0/target,sharing=locked \
    trunk build --release --locked

# Pre-compress once, here, so nginx spends no CPU per request and the 32MB wasm
# stops crossing the wire uncompressed. Shared with the CI `trunk` job so the
# from-source and prebuilt paths below emit identical bytes.
RUN /app/deploy/precompress.sh dist

# ---------------------------------------------------------------------------
# Runtime base — nginx + config, no content yet
# ---------------------------------------------------------------------------
FROM nginx:1.27-alpine AS base
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 8080

# ---------------------------------------------------------------------------
# `prebuilt` — CI target. Skips the build stage entirely and takes `dist/` from
# a named build context, because the `trunk` job has already produced exactly
# these bytes with a warm cache:
#
#   docker buildx build --target prebuilt \
#     --build-context dist=./MtechServer2.0/dist .
#
# A named context is used rather than the main one so `.dockerignore` can keep
# excluding `**/dist/` — un-ignoring it would bloat the build context of every
# other image that builds from this repo root (axum_server, plugin_builder).
# The context must already be pre-compressed; CI runs deploy/precompress.sh.
# ---------------------------------------------------------------------------
FROM base AS prebuilt
COPY --from=dist . /usr/share/nginx/html
# Fail the BUILD on a bad config rather than crash-looping pods at rollout.
RUN nginx -t

# ---------------------------------------------------------------------------
# `runtime` — default target: self-contained build from source, for anyone
# running a plain `docker build .` without CI's prebuilt dist.
# Last stage in the file, so it is what `docker build` selects by default.
# ---------------------------------------------------------------------------
FROM base AS runtime
COPY --from=build /app/MtechServer2.0/dist /usr/share/nginx/html
RUN nginx -t
