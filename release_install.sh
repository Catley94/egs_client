#!/bin/bash

RUST_PROGRAM_NAME="egs_client"
FLUTTER_DIRECTORY_NAME="Flutter_EGL"
FLUTTER_APP_NAME="test_app_ui"
ALIASES=("egs_client")

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (with sudo)"
    exit 1
fi

# Install the program
install_program() {
    REAL_USER="${SUDO_USER:-$USER}"
    HOME_DIR=$(eval echo ~$REAL_USER)
    echo "User: $REAL_USER"
    echo "Installing ${RUST_PROGRAM_NAME}..."

    echo "Creating folder /usr/share/${RUST_PROGRAM_NAME}"
    mkdir -p /usr/share/$RUST_PROGRAM_NAME/client

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
    cp "./${RUST_PROGRAM_NAME}" "/usr/share/${RUST_PROGRAM_NAME}/"
    echo "Making /usr/share/$RUST_PROGRAM_NAME/$RUST_PROGRAM_NAME binary executable"
    chmod +x "/usr/share/${RUST_PROGRAM_NAME}/${RUST_PROGRAM_NAME}"

    # Copy Flutter App binary files
    echo "Copying $FLUTTER_PROGRAM_NAME binary to /usr/share/$RUST_PROGRAM_NAME/client"
    cp -a "./client/" "/usr/share/${RUST_PROGRAM_NAME}/"
    echo "Making /usr/share/$RUST_PROGRAM_NAME/client/$FLUTTER_PROGRAM_NAME binary executable"
    chmod +x "/usr/share/${RUST_PROGRAM_NAME}/client/${FLUTTER_APP_NAME}"

}

# Main
main() {
    install_program

    echo "Installation complete!"
}

main "$@"