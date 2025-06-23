# Stage 1 - Build
FROM rust:1.85 as builder

# Create app directory
WORKDIR /app

# Pre-cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY .sqlx ./.sqlx

# Compile dependencies first to cache them
RUN cargo build --release

# Stage 2 - Runtime
FROM rust:1.85

# Install required libraries for Rust binaries
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m appuser

# Copy binary from builder
COPY --from=builder /app/target/release/agon_service /usr/local/bin/app

# Use non-root user
USER appuser

# Expose the port your Poem app listens on
EXPOSE 3000

# Start the app
CMD ["app", "run-server", "abc.com"]
