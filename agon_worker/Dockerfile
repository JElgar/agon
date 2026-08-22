# Build context is the WORKSPACE ROOT (not this directory), because agon_worker
# depends on the sibling agon_core crate via a path dependency and cargo needs
# the whole workspace manifest to resolve. Build from the repository root with:
#   docker build -f agon_worker/Dockerfile -t agon_worker .

# Stage 1 - cargo-chef: separate "what are the dependencies" from "compile
# them" so the (slow) dependency-build layer is keyed only on Cargo.toml/
# Cargo.lock, not on every source edit. Without this, `COPY . .` followed by
# a single `cargo build` means ANY source change busts the layer cache and
# recompiles the entire dependency graph — including the Temporal SDK, which
# is the bulk of this crate's build time — from scratch. See
# https://github.com/LukeMathWalker/cargo-chef.
FROM rust:1.96 AS chef
WORKDIR /app
RUN cargo install cargo-chef --locked

# The Temporal SDK deps (temporalio-protos / prost-build) compile protobuf at
# build time, so `protoc` must be present in every stage that builds worker
# deps or the worker binary itself.
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Builds only agon_worker's dependencies (and agon_core's, as its path dep) —
# cached across builds as long as recipe.json is unchanged, i.e. as long as no
# Cargo.toml/Cargo.lock changed. A pure source edit skips straight to the much
# cheaper `cargo build` below instead of recompiling every crate from scratch.
RUN cargo chef cook --release --recipe-path recipe.json -p agon_worker
COPY . .
RUN cargo build --release -p agon_worker

# Stage 2 - Runtime
# Must share glibc with the builder base. `rust:1.96` is on Debian trixie
# (glibc 2.38/2.39); bookworm-slim (glibc 2.36) is too old and the binary fails
# at startup with `GLIBC_2.3x not found`, so match the builder with trixie.
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

RUN useradd -m appuser

COPY --from=builder /app/target/release/agon_worker /usr/local/bin/agon_worker

USER appuser

CMD ["agon_worker"]
