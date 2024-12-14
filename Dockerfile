FROM --platform=$BUILDPLATFORM rustlang/rust:nightly
# 
# 
WORKDIR /
RUN rustup default nightly
RUN rustup target add wasm32-unknown-unknown

# RUN rustup target add wasm32-wasi
RUN rustup component add rust-src
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev openssl
RUN update-ca-certificates 
# COPY MtechServer2.0 MtechServer2.0
# COPY displays displays
# COPY database database
RUN wget -qO- https://github.com/trunk-rs/trunk/releases/download/v0.21.4/trunk-x86_64-unknown-linux-musl.tar.gz | tar -xzf-
# COPY Cargo.toml Cargo.toml
# COPY rust-toolchain.toml rust-toolchain.toml
RUN mv trunk MtechServer2.0/trunk
WORKDIR /MtechServer2.0
ENTRYPOINT [ "/MtechServer2.0/trunk" ]
CMD [ "serve"] 
# , "--release"








# FROM --platform=$BUILDPLATFORM rust:latest AS buildbase
# WORKDIR /src
# RUN rustup target add wasm32-unknown-unknown
# RUN rustup target add wasm32-wasi
# RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev 
# RUN wget unzip
#
# FROM buildbase AS build
# COPY server.js .
# RUN wget https://github.com/second-state/wasmedge-quickjs/releases/download/v0.5.0-alpha/wasmedge_quickjs.wasm
# RUN wget https://github.com/second-state/wasmedge-quickjs/releases/download/v0.5.0-alpha/modules.zip
# RUN unzip modules.zip
# 
# RUN cargo install trunk
# COPY ./dist /dist
# COPY Trunk.toml Trunk.toml
# COPY ./frontend ./frontend
# COPY ./database ./database
# COPY Cargo.toml Cargo.toml
# COPY index.html index.html 
# 
# FROM scratch
# ENTRYPOINT [ "mtechserver-bb39a05226a8fb4c_bg.wasm" ]
# COPY --link --from=build dist/mtechserver-bb39a05226a8fb4c_bg.wasm /mtechserver-bb39a05226a8fb4c_bg.wasmd
