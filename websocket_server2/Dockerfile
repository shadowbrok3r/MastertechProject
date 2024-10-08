FROM messense/rust-musl-cross:x86_64-musl AS planner
WORKDIR /server
RUN rustup update
COPY . .
RUN cargo install cargo-chef
RUN cargo chef prepare --recipe-path recipe.json

## STAGE 2
FROM messense/rust-musl-cross:x86_64-musl AS cacher
RUN rustup update
WORKDIR /server
RUN cargo install cargo-chef
COPY --from=planner /server/recipe.json recipe.json
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json

FROM ubuntu:latest AS mold
RUN  apt-get update \
  && apt-get install -y wget \
  && rm -rf /var/lib/apt/lists/*
RUN wget https://github.com/rui314/mold/releases/download/v2.32.1/mold-2.32.1-x86_64-linux.tar.gz
RUN tar -xzf mold-2.32.1-x86_64-linux.tar.gz
RUN mv mold-2.32.1-x86_64-linux/bin/* /usr/bin/

## STAGE 3
FROM messense/rust-musl-cross:x86_64-musl as builder
COPY --from=mold /usr/bin/*mold* /usr/bin/mold

COPY . /server
WORKDIR /server
# copy deps
COPY --from=cacher /server/target target
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev
RUN cargo build --release --target x86_64-unknown-linux-musl
RUN ls -las /server/target/x86_64-unknown-linux-musl/release/

FROM rust:slim-buster
WORKDIR /
COPY --from=builder /server/target/x86_64-unknown-linux-musl/release/websocket_server2 /websocket_server2
EXPOSE 8081
ENTRYPOINT ["/websocket_server2"]
RUN echo output.log
