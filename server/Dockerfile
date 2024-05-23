## STAGE 1
# messense/rust-musl-cross:x86_64-musl AS planner
FROM rust:slim-buster
# WORKDIR /downloads
# COPY ./downloads .

WORKDIR /server
RUN apt-get update && apt-get install -y clang gcc build-essential libclang-dev
RUN cargo install cargo-watch
COPY . .
COPY ./mold /usr/local/



################ COMMENT OUT EVERYTHING BELOW FOR HOT RELOADING

# RUN cargo install cargo-chef
# RUN cargo chef prepare --recipe-path recipe.json

# ## STAGE 2
# FROM messense/rust-musl-cross:x86_64-musl AS cacher

# WORKDIR /server
# RUN cargo install cargo-chef
# COPY --from=planner /server/recipe.json recipe.json
# RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json

# ## STAGE 3
# FROM messense/rust-musl-cross:x86_64-musl as builder

# COPY . /server
# WORKDIR /server

# # copy deps
# COPY --from=cacher /server/target target
# # COPY --from=cacher /usr/local/cargo /usr/local/cargo

# RUN cargo build --release --target x86_64-unknown-linux-musl
# FROM scratch

# COPY --from=builder /server/target/x86_64-unknown-linux-musl/release/server /server
# CMD [ "./server" ]
# ENTRYPOINT ["server"]

################ COMMENT OUT EVERYTHING ABOVE FOR HOT RELOADING