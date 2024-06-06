FROM --platform=$BUILDPLATFORM rust:latest
WORKDIR /
RUN rustup target add wasm32-unknown-unknown
RUN rustup target add wasm32-wasi
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev 
RUN cargo install trunk
COPY ./dist /dist
COPY Trunk.toml Trunk.toml
COPY ./frontend ./frontend
COPY ./database ./database
COPY Cargo.toml Cargo.toml
COPY frontend/index.html frontend/index.html 

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
# COPY frontend/index.html frontend/index.html 
# 
# FROM scratch
# ENTRYPOINT [ "mtechserver-bb39a05226a8fb4c_bg.wasm" ]
# COPY --link --from=build dist/mtechserver-bb39a05226a8fb4c_bg.wasm /mtechserver-bb39a05226a8fb4c_bg.wasm