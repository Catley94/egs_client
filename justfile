# Run locally for development
dev:
    cargo run

# Build release
build:
    cargo build --release

# Test
test:
    cargo test

# --- Flutter desktop helpers ---
# Build Flutter desktop app (Linux)
flutter-build-linux:
    cd Flutter_EGL && flutter build linux --release

# Build Flutter desktop app (Windows)
flutter-build-windows:
    cd Flutter_EGL && flutter build windows --release

# Build Flutter desktop app (macOS)
flutter-build-macos:
    cd Flutter_EGL && flutter build macos --release

# Run both backend and Flutter UI (ensure you built the UI first)
run-both:
    cargo run -- --mode=both

# Run Flutter UI only (assumes backend is already running elsewhere)
run-frontend:
    cargo run -- --mode=frontend

# Run backend only (default)
run-backend:
    cargo run -- --mode=backend
