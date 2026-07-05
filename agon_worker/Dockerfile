# Build context is the WORKSPACE ROOT (not this directory), because agon_worker
# depends on the sibling agon_core crate via a path dependency and cargo needs
# the whole workspace manifest to resolve. Build from the repository root with:
#   docker build -f agon_worker/Dockerfile -t agon_worker .

# Stage 1 - Build
FROM rust:1.96 as builder

WORKDIR /app

# The whole workspace is needed: cargo parses every member listed in the root
# Cargo.toml even when building a single package with -p.
COPY . .

# Build only the worker binary (and its dependencies). Default features only —
# the `temporal` feature additionally needs `protoc` installed here (its deps
# compile protobuf at build time); add `protobuf-compiler` + `--features
# temporal` when/if that feature is enabled in the deployed image.
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
