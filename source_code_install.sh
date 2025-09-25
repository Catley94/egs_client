#!/bin/bash

RUST_PROGRAM_NAME="egs_client"
FLUTTER_PROGRAM_NAME="Flutter_EGL"
ALIASES=("egs_client")

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (with sudo) as it installs binaries to /usr/share/$RUST_PROGRAM_NAME"
    exit 1
fi




# Check if flutter is installed
check_flutter() {
    REAL_USER="${SUDO_USER:-$USER}"
    HOME_DIR=$(eval echo ~$REAL_USER)

    # Source profile files and check flutter
    FLUTTER_BIN=$(sudo -u "$REAL_USER" bash -c 'source ~/.profile 2>/dev/null; source ~/.bashrc 2>/dev/null; command -v flutter')
    if [ -z "$FLUTTER_BIN" ]; then
        # Try direct path if environment variable approach failed
        if [ -x "/home/$REAL_USER/flutter_sdk/flutter/bin/flutter" ]; then
            FLUTTER_BIN="/home/$REAL_USER/flutter_sdk/flutter/bin/flutter"
        else
            echo "Error: Flutter not found in PATH. Please make sure Flutter is properly installed."
            exit 1
        fi
    fi

    # Verify flutter is actually executable
    if ! sudo -u "$REAL_USER" "$FLUTTER_BIN" --version >/dev/null 2>&1; then
        echo "Error: Flutter SDK not found or not executable. Please install Flutter first"
        echo "Make sure Flutter is in your PATH and properly configured"
        echo "Visit https://docs.flutter.dev/get-started/install/linux"
        exit 1
    fi

}


# Check for Rust toolchain
check_rust() {
    REAL_USER="${SUDO_USER:-$USER}"

    # Check both rustc and cargo using the actual user's environment
    if ! sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" 2>/dev/null; command -v rustc && command -v cargo' >/dev/null 2>&1; then
        echo "Error: Rust toolchain (rustc/cargo) not found. Please install Rust first:"
        echo "Visit https://rustup.rs/ or run:"
        echo "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

}

# Build release version
build_release() {
    echo "Building Rust API release version..."
    sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" 2>/dev/null && cargo build --release' || {
        echo "Rust API Build failed!"
        exit 1
    }

    echo "Building Flutter App release version..."
    sudo -u "$REAL_USER" bash -c 'cd ./'"${FLUTTER_PROGRAM_NAME}"' && '"$FLUTTER_BIN"' build linux --release' || {
        echo "Flutter failed!"
        exit 1
    }
}

# Install the program
install_program() {
    echo "Installing ${RUST_PROGRAM_NAME}..."

    echo "Creating folder /usr/share/${RUST_PROGRAM_NAME}"
    mkdir -p /usr/share/$RUST_PROGRAM_NAME

    # Create user data directories following XDG base directory spec
    echo "Creating XDG user directories for $RUST_PROGRAM_NAME"
    echo "Creating folder /home/$REAL_USER/.local/share/$RUST_PROGRAM_NAME/downloads"
    sudo -u "$REAL_USER" bash -c "mkdir -p ~/.local/share/$RUST_PROGRAM_NAME/downloads"
    echo "Creating folder /home/$REAL_USER/.cache/$RUST_PROGRAM_NAME"
    sudo -u "$REAL_USER" bash -c "mkdir -p ~/.cache/$RUST_PROGRAM_NAME"
    echo "Creating folder /home/$REAL_USER/.config/$RUST_PROGRAM_NAME"
    sudo -u "$REAL_USER" bash -c "mkdir -p ~/.config/$RUST_PROGRAM_NAME"


    # Copy Rust API binary
    echo "Copying $RUST_PROGRAM_NAME binary to /usr/share/$RUST_PROGRAM_NAME"
    cp "./target/release/${RUST_PROGRAM_NAME}" "/usr/share/${RUST_PROGRAM_NAME}/"
    echo "Making /usr/share/$RUST_PROGRAM_NAME/$RUST_PROGRAM_NAME binary executable"
    chmod +x "/usr/share/${RUST_PROGRAM_NAME}/${RUST_PROGRAM_NAME}"

    # Copy Flutter App binary files
    echo "Copying $FLUTTER_PROGRAM_NAME binary to /usr/share/$RUST_PROGRAM_NAME/client"
    cp -a "./${FLUTTER_PROGRAM_NAME}/build/linux/x64/release/bundle" "/usr/share/${RUST_PROGRAM_NAME}/client"
    echo "Making /usr/share/$RUST_PROGRAM_NAME/client/$FLUTTER_PROGRAM_NAME binary executable"
    chmod +x "/usr/share/${RUST_PROGRAM_NAME}/client/${FLUTTER_PROGRAM_NAME}"

}

# Main
main() {
    check_rust
    check_flutter
    build_release
    install_program

    echo "Installation complete!"
}

main "$@"