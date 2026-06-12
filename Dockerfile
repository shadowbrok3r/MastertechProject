FROM --platform=$BUILDPLATFORM rustlang/rust:nightly
WORKDIR /

RUN rustup target add wasm32-unknown-unknown && \
    rustup target add wasm32-wasip1 && \
    rustup component add rust-src && \
    apt-get update && apt-get install -y clang gcc build-essential libclang-dev openssl wget && \
    update-ca-certificates

COPY Cargo.toml Cargo.toml
COPY build_hash.rs build_hash.rs
COPY MtechServer2.0 MtechServer2.0
COPY displays displays
COPY database database
COPY tcp_protocol tcp_protocol
COPY plugin_builder plugin_builder
COPY .env .env

# Wasm-only workspace: trim members and strip native path deps that are not
# copied into this context (stress-kit, stress-runner). plugin_builder stays
# because displays references it in Cargo.toml for manifest resolution.
RUN sed -i '/^members = \[/,/^\]/c\members = ["database", "displays", "MtechServer2.0", "tcp_protocol"]' Cargo.toml && \
    sed -i '/stress-kit/d; /stress-runner/d' Cargo.toml && \
    sed -i '/stress-kit/d; /stress-runner/d; /native-telemetry/d' displays/Cargo.toml

ENV RUSTFLAGS="-C target-feature=+bulk-memory,+mutable-globals --cfg getrandom_backend=\"wasm_js\""

RUN wget -qO- https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-musl.tar.gz | tar -xzf- && \
    mv trunk /usr/local/bin/trunk

WORKDIR /MtechServer2.0
EXPOSE 8080
ENTRYPOINT ["trunk"]
CMD ["serve", "--release", "--address", "0.0.0.0"]

# Build locally, use pre-built dist folder:
#   trunk build --release
#   docker build -f MtechServer2.0/Dockerfile.prod -t mtechserver .
#
# Build in container (CI/CD):
#   docker build -t mtechserver .


