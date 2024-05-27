FROM rust:slim-bookworm
WORKDIR /server
RUN rustup target add wasm32-unknown-unknown
RUN rustc --version
RUN rustup update
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev
RUN cargo install trunk
COPY . .
COPY ./mold /usr/local/
