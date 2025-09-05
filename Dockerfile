FROM rust:1.89.0-bullseye AS build

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    clang \
    lld \
    git \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Build the application  
RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=cache,target=/app/target/ \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    cargo build --locked --release && \
    cp ./target/release/egs_client /bin/egs_client

FROM debian:bullseye-slim AS final

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd \
    --create-home \
    --shell /bin/sh \
    --uid 10001 \
    appuser

# Copy the binary
COPY --from=build /bin/egs_client /bin/

# Create required directories
RUN mkdir -p /app/cache /app/downloads && chown -R appuser:appuser /app

USER appuser
WORKDIR /app

EXPOSE 8080

ENV RUST_LOG=info
ENV BIND_ADDR=0.0.0.0:8080

CMD ["/bin/egs_client"]