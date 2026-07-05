# Build context is the WORKSPACE ROOT (not this directory), because agon_service
# depends on the sibling agon_core crate via a path dependency and cargo needs
# the whole workspace manifest to resolve. Build from the repository root with:
#   docker build -f agon_service/Dockerfile -t agon_service .

# Stage 1 - Build
FROM rust:1.85 as builder

WORKDIR /app

# The whole workspace is needed: cargo parses every member listed in the root
# Cargo.toml even when building a single package with -p.
COPY . .

RUN cargo build --release -p agon_service

# Stage 2 - Runtime
FROM debian:bookworm-slim

# Install required libraries for Rust binaries
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m appuser

# Copy binary from builder
COPY --from=builder /app/target/release/agon_service /usr/local/bin/app

# Use non-root user
USER appuser

# Expose the port the Poem app listens on
EXPOSE 7000

# Start the app
CMD ["app", "run-server", "abc.com"]
