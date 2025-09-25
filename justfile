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

package:
    bash -c "./scripts/dev_copy_binary_to_release_folder.sh"

install-release:
    cd release/files && sudo bash -c "./release_install.sh"

uninstall-release:
    cd release/files && sudo bash -c "./release_uninstall.sh"

# Test
test:
    cargo test

# --- Flutter desktop helpers ---
# Build Flutter desktop app (Linux)
# Note: include `flutter clean` to avoid stale symlink crashes during builds
flutter-build-linux:
    cd Flutter_EGL && flutter clean && flutter pub get && flutter build linux --release

# Run both backend and Flutter UI explicitly (ensure Flutter debug bundle is fresh)
run-both:
    cd Flutter_EGL && flutter clean && flutter pub get && flutter build linux --debug
    cargo run -- --mode=both

# Run Flutter UI only (assumes backend is already running elsewhere)
# Ensure a fresh Flutter debug build first so the spawned binary is up to date
run-frontend:
    cd Flutter_EGL && flutter clean && flutter pub get && flutter build linux --debug
    cargo run -- --mode=frontend

# Run backend only
run-backend:
    cargo run -- --mode=backend
