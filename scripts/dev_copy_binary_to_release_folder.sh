#!/bin/bash

set -euo pipefail

RUST_PROGRAM_NAME="egs_client"
FLUTTER_PROGRAM_NAME="Flutter_EGL"
RELEASE_FOLDER_NAME="release"

# Resolve project root (parent of the folder containing this script)
PROJECT_ROOT="$(cd "$(dirname "$0")"/.. && pwd)"

# Get version from Cargo.toml
VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f2)

if ! command -v zip >/dev/null 2>&1; then
    echo "Error: zip command not found. Please install zip first"
    exit 1
fi

echo "Building version: $RELEASE_FOLDER_NAME-$RUST_PROGRAM_NAME-$VERSION"

echo "Removing old release folder contents"
rm -rf "$PROJECT_ROOT/$RELEASE_FOLDER_NAME/files"

echo "Removing old release zipped folder"
rm -rf "$PROJECT_ROOT/$RELEASE_FOLDER_NAME/zipped"

echo "Building Rust API release version..."
(
  cd "$PROJECT_ROOT"
  cargo build --release
) || {
    echo "Build failed!"
    exit 1
}

echo "Building Flutter Linux release..."
(
  cd "$PROJECT_ROOT/$FLUTTER_PROGRAM_NAME"
  flutter build linux --release
) || {
    echo "Flutter build failed!"
    exit 1
}

echo "Making release folder within project"
mkdir -p "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/files/client"

echo "Creating zipped files folder"
mkdir -p "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/zipped"

echo "Copying (release) Rust API: ${RUST_PROGRAM_NAME} to ${PROJECT_ROOT}/${RELEASE_FOLDER_NAME}/files"
cp "$PROJECT_ROOT/target/release/$RUST_PROGRAM_NAME" "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/files/"

echo "Copying (release) Flutter App bundle to ${PROJECT_ROOT}/${RELEASE_FOLDER_NAME}/files/client"
cp -r "$PROJECT_ROOT/$FLUTTER_PROGRAM_NAME/build/linux/x64/release/bundle/"* "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/files/client/"

echo "Copying release_install.sh to ${PROJECT_ROOT}/${RELEASE_FOLDER_NAME}/files"
cp "$PROJECT_ROOT/scripts/release_install.sh" "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/files"
echo "Copying release_uninstall.sh to ${PROJECT_ROOT}/${RELEASE_FOLDER_NAME}/files"
cp "$PROJECT_ROOT/scripts/release_uninstall.sh" "$PROJECT_ROOT/${RELEASE_FOLDER_NAME}/files"

echo "Generating desktop entry..."
cat > "$PROJECT_ROOT"/${RELEASE_FOLDER_NAME}/files/${RUST_PROGRAM_NAME}.desktop <<EOF
[Desktop Entry]
Name=${RUST_PROGRAM_NAME}
Comment=
Exec=/usr/share/${RUST_PROGRAM_NAME}/${RUST_PROGRAM_NAME}
Icon=
Terminal=false
Type=Application
Categories=Development
EOF


echo "Creating zip archive..."
zip -r "$PROJECT_ROOT/release/zipped/linux-release-${VERSION}.zip" \
    -j0 /dev/null >/dev/null 2>&1 || true
# Zip the 'files' directory with relative paths inside the archive
( cd "$PROJECT_ROOT/$RELEASE_FOLDER_NAME" && zip -r "zipped/linux-release-${VERSION}.zip" "files" )

echo "Done! Created linux-release-${VERSION}.zip at $PROJECT_ROOT/release/zipped"