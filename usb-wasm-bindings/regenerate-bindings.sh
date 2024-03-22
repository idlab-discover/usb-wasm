#!/bin/sh

# Based on the regenerate script from the Rust WASI 0.2 bindings 
# https://github.com/bytecodealliance/wasi/blob/main/ci/regenerate.sh

set -ex

cargo install --locked wit-bindgen-cli@0.21.0
wit-bindgen rust ../wit --out-dir ./src --std-feature --type-section-suffix rust-wasi-from-crates-io
cargo fmt