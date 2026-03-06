#!/bin/bash
# Build script for rusb WASI workload
# Cross-compiles rusb against Robbe Leroy's libusb-wasi (guest implementation)
#
# Architecture:
#   Code -> rusb -> libusb guest (libusb-wasi) -> WASI-USB -> host implementation -> syscalls
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "${SCRIPT_DIR}"
SYSROOT="${SCRIPT_DIR}/wasi-sysroot"

echo "=== Rusb WASI Workload Cross-Compile Build ==="
echo "Sysroot: ${SYSROOT}"

# Configure pkg-config for cross-compilation (per rusb cross-compile docs)
# See: https://github.com/a1ien/rusb#cross-compiling
export PKG_CONFIG_DIR=
export PKG_CONFIG_LIBDIR="${SYSROOT}/usr/lib/pkgconfig:${SYSROOT}/usr/share/pkgconfig"
export PKG_CONFIG_SYSROOT_DIR="${SYSROOT}"
export PKG_CONFIG_ALLOW_CROSS=1

# Force static linking against libusb-wasi
export LIBUSB_STATIC=1

# Verify pkg-config finds libusb-wasi
echo ""
echo "--- pkg-config check ---"
pkg-config --cflags --libs libusb-1.0 || { echo "ERROR: pkg-config can't find libusb-1.0"; exit 1; }
echo ""

# Build directly for wasm32-wasip2
# The build.rs links cguest_component_type.o which provides the WIT world
# metadata, allowing wasm-component-ld to resolve custom WASI-USB imports.
echo "--- Building WASI component (wasm32-wasip2) ---"
cargo build --target wasm32-wasip2 --release
# Also build the lsusb example
echo "--- Building lsusb example ---"
cargo build --target wasm32-wasip2 --release --example lsusb

OUTPUT="target/wasm32-wasip2/release/rusb_wasi_workload.wasm"
OUTPUT_LSUSB="target/wasm32-wasip2/release/examples/lsusb.wasm"

echo ""
echo "=== Build complete ==="
echo "Workload: ${SCRIPT_DIR}/${OUTPUT}"
echo "lsusb:    ${SCRIPT_DIR}/${OUTPUT_LSUSB}"

# Verify
echo ""
echo "--- Verification ---"
file "${OUTPUT}"
