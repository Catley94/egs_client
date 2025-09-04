Dockerizing egs_client

This project provides a minimal Actix Web service to interact with Epic Fab assets. This guide explains how to build and run it using Docker.

Prerequisites
- Docker 20.10+
- For native builds (outside Docker): Rust toolchain 1.85+ (edition 2024 support)

Build the image

# From the repository root
docker build -t egs_client:latest .

Run the container

# Map port 8080 and mount local cache/downloads directories if you want persistence
mkdir -p ./cache ./downloads

docker run --rm -it \
  -p 8080:8080 \
  -v $(pwd)/cache:/app/cache \
  -v $(pwd)/downloads:/app/downloads \
  -e RUST_LOG=info \
  --name egs_client \
  egs_client:latest

Then open:
- http://localhost:8080/ (welcome)
- http://localhost:8080/health (health check)
- http://localhost:8080/get-fab-list

Notes
- Inside Docker, the image now defaults to binding 0.0.0.0:8080 (via ENV BIND_ADDR). You can still override with `-e PORT=8080` or `-e BIND_ADDR=IP:PORT`.
- For higher logs: -e RUST_LOG=debug
- The container runs as a non-root user (uid 10001). Ensure mounted directories are writable by this uid, or adjust permissions locally, e.g.:
  sudo chown -R 10001:10001 cache downloads

Troubleshooting
- Container exits immediately: If you built with an older Dockerfile revision that used RUN instead of CMD, rebuild the image (docker build --no-cache -t egs_client:latest .). Also ensure port 8080 on the host isn’t occupied, or set a different port with `-e PORT=8081 -p 8081:8081`. The server now retries binding if it temporarily fails.
- Volumes permissions: mounted `./cache` and `./downloads` must be writable by uid 10001 inside the container. Adjust ownership: `sudo chown -R 10001:10001 cache downloads` or run without mounts to test.
- SSL issues: the runtime image contains ca-certificates. If you still encounter TLS errors, verify your network/proxy settings.
- Build cache: The Dockerfile is set up to maximize Rust dependency cache usage across builds.
