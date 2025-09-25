#!/bin/bash

RUST_PROGRAM_NAME="egs_client"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root (with sudo) as it uninstalls binaries in /usr/share/$RUST_PROGRAM_NAME"
    exit 1
fi

# Install the program
uninstall_program() {
    REAL_USER="${SUDO_USER:-$USER}"
    echo "Uninstalling ${RUST_PROGRAM_NAME}..."

    echo "Removing folder /usr/share/${RUST_PROGRAM_NAME}"
    rm -rf /usr/share/${RUST_PROGRAM_NAME:?}

    echo "Removing folder /home/$REAL_USER/.local/share/$RUST_PROGRAM_NAME"
    sudo -u "$REAL_USER" bash -c "rm -rf ~/.local/share/$RUST_PROGRAM_NAME"
    echo "Removing folder /home/$REAL_USER/.cache/$RUST_PROGRAM_NAME"
    sudo -u "$REAL_USER" bash -c "rm -rf ~/.cache/$RUST_PROGRAM_NAME"
    echo "Removing folder /home/$REAL_USER/.config/$RUST_PROGRAM_NAME"
    sudo -u "$REAL_USER" bash -c "rm -rf ~/.config/$RUST_PROGRAM_NAME"


    # Copy Rust and Flutter binaries
    echo "Removing $RUST_PROGRAM_NAME from /usr/share/"
    rm -rf "/usr/share/${RUST_PROGRAM_NAME:?}/"

    echo "Removing .desktop from ~/.local/share/applications"
    sudo -u "$REAL_USER" bash -c "rm ~/.local/share/applications/$RUST_PROGRAM_NAME.desktop"

}

# Main
main() {
    uninstall_program

    echo "Uninstallation complete!"
}

main "$@"