#!/data/data/com.termux/files/usr/bin/bash
# Masix Termux Build Script

set -e

echo "=== Masix Termux Build ==="

# Install dependencies
echo "Installing dependencies..."
pkg install -y rust nodejs-lts termux-api

# Build
echo "Building Masix..."
cargo build --release

# Install binary
echo "Installing masix binary..."
SOURCE_BIN="target/release/masix"
if [ ! -f "$SOURCE_BIN" ]; then
  SOURCE_BIN="target/aarch64-linux-android/release/masix"
fi

if [ ! -f "$SOURCE_BIN" ]; then
  echo "Error: masix binary not found after build."
  exit 1
fi

cp "$SOURCE_BIN" "$PREFIX/bin/"
chmod +x $PREFIX/bin/masix

echo "=== Build Complete ==="
echo "Run 'masix --help' to get started"
echo "Run 'masix config init' to create default config"
