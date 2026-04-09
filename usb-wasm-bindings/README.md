# usb-wasm-bindings

This crate contains pre-generated Rust bindings for the WASI-USB WIT interfaces. These bindings are used by guest components to interact with the USB host implementation.

## Purpose

The WebAssembly Component Model requires bindings to be generated from WIT files. To avoid requiring every guest component to run `wit-bindgen` during its build process, this crate provides a centralized set of generated bindings that can be imported as a dependency.

## Contents

- **`src/cguest.rs`**: The actual generated Rust bindings (World: `cguest`).
- **`src/lib.rs`**: Re-exports the generated modules for easier use.
- **`regenerate-bindings.sh`**: A utility script to re-run `wit-bindgen` and update the bindings.

## How to Update

If you modify the WIT definitions in the root `wit/` directory, you must regenerate these bindings:

```shell
./regenerate-bindings.sh
```

This script requires `wit-bindgen-cli` to be installed:

```shell
cargo install wit-bindgen-cli
```

## Usage in Guest Components

Add this crate as a dependency in your component's `Cargo.toml`:

```toml
[dependencies]
usb-wasm-bindings = { path = "../../usb-wasm-bindings" }
```

Then use the bindings in your Rust code:

```rust
use usb_wasm_bindings::component::usb::device::get_devices;

fn main() {
    let devices = get_devices();
    // ...
}
```