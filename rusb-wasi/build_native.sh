#!/bin/bash
set -e

# rusb-wasi native build script for macOS

echo "Starting native build of rusb..."

# Absolute path to the natively built libusb
LIBUSB_DIR="/Users/sibrenwieme/Documents/Masterproef/libusb-wasi"

if [ ! -f "$LIBUSB_DIR/libusb-1.0.pc" ]; then
    echo "Error: libusb-1.0.pc not found in $LIBUSB_DIR. Please run the libusb native build first."
    exit 1
fi

# Set PKG_CONFIG_PATH to point to our natively built libusb
export PKG_CONFIG_PATH="$LIBUSB_DIR"
export LIBUSB_STATIC=1

# Explicitly tell the linker where to find libusb-1.0.a if pkg-config fails to communicate it properly
# libusb-wasi puts the .a file in libusb/.libs/
export RUSTFLAGS="-L native=$LIBUSB_DIR/libusb/.libs"

echo "Building rusb and examples natively..."
cargo build --release --examples

echo "Native rusb build complete."
echo "Examples are located in target/release/examples/"
