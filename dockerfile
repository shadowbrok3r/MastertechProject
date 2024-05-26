FROM rust:slim-buster
WORKDIR /server
# RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev
RUN rustup target add wasm32-unknown-unknown
RUN cargo install --locked trunk
COPY . .
COPY ./mold /usr/local/
