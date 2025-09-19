# Run locally for development (auto-runs both if Flutter desktop binary is present)
dev:
    cargo run

# Build release
build:
    cargo build --release

# Build both Rust (release) and Flutter desktop (release) — for a single-command packaged run
# Note: `flutter clean` avoids PathExistsException on stale .plugin_symlinks
build-both:
    cargo build --release
    cd Flutter_EGL && flutter clean && flutter pub get && flutter build linux --release

# Test
test:
    cargo test

# --- Flutter desktop helpers ---
# Build Flutter desktop app (Linux)
# Note: include `flutter clean` to avoid stale symlink crashes during builds
flutter-build-linux:
    cd Flutter_EGL && flutter clean && flutter pub get && flutter build linux --release

# Run both backend and Flutter UI explicitly
run-both:
    cargo run -- --mode=both

# Run Flutter UI only (assumes backend is already running elsewhere)
run-frontend:
    cargo run -- --mode=frontend

# Run backend only
run-backend:
    cargo run -- --mode=backend
