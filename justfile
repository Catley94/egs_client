# Interactive run (for authentication)
run:
    docker compose build
    docker compose run --service-ports --rm egs_client

# Background run (after auth is cached)
run-bg:
    docker compose up -d --build

# Stop all
stop:
    docker compose down

# Clean everything
clean:
    docker compose down --rmi all --volumes
    docker system prune -f

# Run locally for development
dev:
    cargo run
