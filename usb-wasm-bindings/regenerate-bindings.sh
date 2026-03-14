#!/bin/sh

# Based on the regenerate script from the Rust WASI 0.2 bindings 
# https://github.com/bytecodealliance/wasi/blob/main/ci/regenerate.sh

set -ex

cargo install --locked wit-bindgen-cli@0.42.1
wit-bindgen rust ../wit --out-dir ./src --std-feature --world cguest --generate-all --pub-export-macro --default-bindings-module usb_wasm_bindings
# Also regenerate C bindings for libusb-wasi
wit-bindgen c ../wit --world cguest --out-dir ../rusb-wasi/libusb1-sys/libusb/libusb/os
cargo fmt