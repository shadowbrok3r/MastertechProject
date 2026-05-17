FROM --platform=$BUILDPLATFORM rustlang/rust:nightly
WORKDIR /
RUN rustup target add wasm32-unknown-unknown
RUN rustup target add wasm32-wasip1
RUN rustup component add rust-src
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev openssl
RUN update-ca-certificates

# Copy ONLY the workspace members MtechServer2.0 transitively needs.
# The native-only crates (Mastertech4.0, axum_server, websocket_server2,
# qc-app, database-tools, stress-kit, stress-runner) are deliberately
# absent from the build context — Docker has no reason to drag them in.
COPY MtechServer2.0 MtechServer2.0
COPY displays displays
COPY database database
COPY tcp_protocol tcp_protocol
COPY .env .env
COPY plugin_builder plugin_builder

# Bring in the workspace manifest, then immediately strip its `members`
# list down to the four crates we just COPY'd. Without this, `cargo
# metadata` errors with "failed to load manifest for workspace member
# `/axum_server`" because the root Cargo.toml still references the
# native-only crates we omitted. The substitution must be kept in sync
# with the COPY commands above — anything missing from one must be
# missing from the other.
COPY Cargo.toml Cargo.toml
RUN sed -i 's|^members = .*|members = ["database", "displays", "MtechServer2.0", "tcp_protocol"]|' Cargo.toml

RUN wget -qO- https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-musl.tar.gz | tar -xzf-

RUN mv trunk MtechServer2.0/trunk
WORKDIR /MtechServer2.0
ENTRYPOINT [ "/MtechServer2.0/trunk" ]
CMD [ "serve", "--release" ]


# Build locally, use pre-built dist folder
#   trunk build --release
#   docker build -f MtechServer2.0/Dockerfile.prod -t mtechserver .
#
# Build in container (slower, but works for CI/CD)
#   docker build -t mtechserver .
