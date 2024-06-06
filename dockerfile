FROM rust:latest
# as builder
WORKDIR /
RUN rustup target add wasm32-unknown-unknown
RUN rustc --version
RUN rustup update
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev 
RUN cargo install trunk
COPY ./dist /dist
COPY Trunk.toml Trunk.toml
COPY ./frontend ./frontend
COPY ./database ./database
COPY Cargo.toml Cargo.toml
COPY frontend/index.html frontend/index.html 



RUN rustup target add wasm32-wasi