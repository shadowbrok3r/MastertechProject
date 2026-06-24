# syntax=docker/dockerfile:1.7
# MtechServer2.0 WASM frontend image. Build from the repo root:
#   docker build -t mtechserver .
#
# Whole-repo context: every workspace member is present so `cargo metadata`
# and trunk resolve cleanly. No manifest edits. wasm build flags + build-std
# come from the committed MtechServer2.0/.cargo/config.toml.

FROM --platform=$BUILDPLATFORM rustlang/rust:nightly
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

WORKDIR /app/MtechServer2.0
EXPOSE 8080
ENTRYPOINT ["trunk"]
CMD ["serve", "--release", "--address", "0.0.0.0"]

# Build locally with a pre-built dist folder:
#   trunk build --release
#   docker build -t mtechserver .
