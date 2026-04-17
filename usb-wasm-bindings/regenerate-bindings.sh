#!/bin/sh

# Based on the regenerate script from the Rust WASI 0.2 bindings 
# https://github.com/bytecodealliance/wasi/blob/main/ci/regenerate.sh

set -ex

cargo install --locked wit-bindgen-cli@0.42.1

# For webcam: generate the export-only world that ONLY exports raw-frame-stream
# This ensures the webcam component doesn't import what it should only export
wit-bindgen rust ../wit --out-dir ./src --std-feature --world "component:wasm-usb-app/webcam-export-only" \
    --pub-export-macro
# Produces: src/webcam_export_only.rs

# Generate webcam-provider world (WIT types for imports)
wit-bindgen rust ../wit --out-dir ./src --std-feature --world "component:wasm-usb-app/webcam-provider-world" \
    --generate-all --pub-export-macro
# Produces: src/webcam_provider_world.rs

# Generate yolo-command world
wit-bindgen rust ../wit --out-dir ./src --std-feature --world "component:wasm-usb-app/yolo-command-world" \
    --generate-all --pub-export-macro
# Produces: src/yolo_command_world.rs

# Also regenerate C bindings for libusb-wasi (if needed in future)
# wit-bindgen c ../wit --world webcam-provider --out-dir ../rusb-wasi/libusb1-sys/libusb/libusb/os

cargo fmt