# USB-WASM

This repository contains the Wasmtime-based host runtime and WebAssembly guest components for the WASI-USB ecosystem. It demonstrates capability-based USB access from Wasm, with a real-time UVC webcam capture demo as the primary example.

Original research and implementation by **IDLab Discover**.

## Project Structure

- **[`wit/`](./wit/)**: WIT interface definitions — an **exact mirror** of `wasi-usb/wit/` (`component:usb@0.2.1`). Extended with `world.wit` that defines `host`, `cguest`, and `webcam-guest` worlds. The canonical source of truth is `wasi-usb/wit/`; update there first, then mirror here.
- **[`usb-wasm/`](./usb-wasm/)**: Host-side implementation of the WASI-USB WIT interfaces. Bridges libusb to the Wasmtime async runtime (`host_impl.rs`, `usb_backend.rs`).
- **[`wasmtime-usb/`](./wasmtime-usb/)**: Wasmtime-based CLI runner that incorporates the `usb-wasm` host implementation.
- **[`command-components/`](./command-components/)**: Guest components implementing USB logic — `lsusb`, `webcam`, `streams-test`, `enumerate-devices-rust`.
- **[`usb-wasm-bindings/`](./usb-wasm-bindings/)**: Pre-generated Rust bindings for the `cguest` world (used by benchmark guest components). The webcam guest uses inline `wit_bindgen::generate!` instead.

## Requirements

- Rust (latest stable)
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
- [just](https://just.systems/)
- [wit-bindgen-cli](https://github.com/bytecodealliance/wit-bindgen) v0.42.1 (only needed to regenerate `usb-wasm-bindings/src/cguest.rs`)

## Building and Running

We use `just` as the primary build system. Run `just --list` to see all available recipes.

### Build the Runtime

```shell
cargo build   # builds the wasmtime-usb host binary
```

### Webcam Demo (UVC capture)

```shell
just build-webcam   # compiles webcam guest to wasm32-wasip2 component
just webcam         # runs with sudo; streams MJPEG frames from any UVC camera
```

The webcam guest handles the full UVC probe/commit negotiation, alternate-setting switching, and MJPEG reassembly from isochronous packets. The host provides only raw USB transfers.

### Other components

```shell
just lsusb                   # list USB devices
just enumerate-devices-rust  # device enumeration demo
just streams-test            # USB 3.0 Bulk Streams validation (SanDisk USB 3.0 stick)
```

> [!NOTE]
> `sudo` is required for direct USB access on Linux/macOS unless udev rules grant permission to the current user.

## Funding information

This work has been partially supported by the ELASTIC project, which received funding from the Smart Networks and Services Joint Undertaking (SNS JU) under the European Union's Horizon Europe research and innovation programme under Grant Agreement No 101139067. Views and opinions expressed are those of the author(s) only.
