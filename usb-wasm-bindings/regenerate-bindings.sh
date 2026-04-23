#!/bin/sh

# Regenerate pre-generated WIT bindings for component:usb@0.2.1 guest worlds.
# Source of truth: ../wit/ (mirror of wasi-usb/wit/ plus cguest/webcam-guest worlds)
#
# Run this whenever the canonical WIT in wasi-usb/wit/ is updated.

set -ex

cargo install --locked wit-bindgen-cli@0.42.1

# Generate C-guest world bindings (used by libusb-wasi C benchmark components)
wit-bindgen rust ../wit --out-dir ./src --std-feature \
    --world "component:usb@0.2.1/cguest" \
    --generate-all --pub-export-macro
# Produces: src/cguest.rs

cargo fmt
