# Multi-stage build for egs_client
# 1) Builder stage compiles the Rust binary in release mode
# 2) Runtime stage runs the binary on a slim Debian base

# ----- Builder -----
FROM rust:1.86-bullseye AS builder

WORKDIR /app

# Create a new empty shell project to prime the dependency cache
# Copy only manifests first to maximize caching
COPY Cargo.toml Cargo.lock ./
# Create dummy src to allow "cargo build" to resolve and cache dependencies
RUN mkdir src && echo "fn main(){}" > src/main.rs

# Build to cache dependencies
RUN cargo build --release || true

# Now copy the actual source
COPY src ./src
#COPY Epic-Asset-Manager ./Epic-Asset-Manager
#COPY assets_reference.txt ./assets_reference.txt
#COPY workflow.rs ./workflow.rs

# Build the actual project
RUN cargo build --release

# ----- Runtime -----
FROM debian:bullseye-slim AS runtime

# Install runtime dependencies if any SSL/certificates needed
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Set a non-root user for safety
RUN useradd -m -u 10001 appuser

WORKDIR /app

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/egs_client /usr/local/bin/egs_client

# Create directories that the app expects to read/write
RUN mkdir -p /app/cache /app/downloads
RUN chown -R appuser:appuser /app

USER appuser

# Expose default Actix port
EXPOSE 8080

# Default command
# Ensure service is reachable by default inside Docker
ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:8080

# Run the service
CMD ["/usr/local/bin/egs_client"]
