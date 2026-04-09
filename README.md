# USB-WASM

This repository contains WebAssembly components, WIT interface definitions, and host implementations for the WASI-USB ecosystem. It includes a variety of demonstrations, from simple USB enumeration to real-time Computer Vision pipelines.

Original research and implementation by **IDLab Discover**.

## Project Structure

- **[`wit/`](./wit/)**: Formal interface definitions (WIT) for USB operations, descriptors, transfers, and Computer Vision (CV).
- **[`usb-wasm/`](./usb-wasm/)**: Host-side implementation of the WASI-USB WIT interfaces. This is the core library that bridges libusb to the Wasmtime runtime.
- **[`wasmtime-usb/`](./wasmtime-usb/)**: A custom Wasmtime-based CLI runner that incorporates the `usb-wasm` host implementation.
- **[`command-components/`](./command-components/)**: Guest components (WebAssembly) implementing specific USB and CV logic (e.g., `lsusb`, `yolo-detector`).
- **[`usb-wasm-bindings/`](./usb-wasm-bindings/)**: Generated Rust bindings for the guest components to use (versioned at `@0.2.1`).
- **[`rusb-wasi/`](./rusb-wasi/)**: A crate providing a wrapper around the generated bindings for a more idiomatic Rust API.

## Requirements

- Rust (latest stable)
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
- [just](https://just.systems/)
- [wit-bindgen-cli](https://github.com/bytecodealliance/wit-bindgen) v0.42.1 (for regenerating bindings)

## Building and Running

We use `just` as the primary build system. You can see all supported commands by running `just --list`.

### 1. Build the Runtime
The runtime consists of the `usb-wasm` host library and the `wasmtime-usb` CLI.

```shell
just build-runtime
```

### 2. Build and Run a Component
You can build and run any of the guest components. For example, to run the `lsusb` demo:

```shell
just lsusb
```

This command will:
1. Compile the `lsusb` component to WASM targeting `wasm32-wasip2`.
2. Wrap it into a WebAssembly Component.
3. Run it using `wasmtime-usb`.

### 3. Manual Workspace Build
To check or build all components in the workspace:

```shell
cargo check --workspace --target=wasm32-wasip2
cargo build --workspace --target=wasm32-wasip2
```

> [!NOTE]
> `sudo` is often required for direct USB access on Linux/macOS when running the host runtime, depending on your udev rules.

## Best xbox pacman performance

Since the game uses CLI output, it needs to write this line per line. This might cause some flickering. For best performance, use Alacritty with a font size of 30 and in fullscreen.

## Funding information

This work has been partially supported by the ELASTIC project, which received funding from the Smart Networks and Services Joint Undertaking (SNS JU) under the European Union’s Horizon Europe research and innovation programme under Grant Agreement No 101139067. Views and opinions expressed are however those of the author(s) only and do not necessarily reflect those of the European Union. Neither the European Union nor the granting authority can be held responsible for them.
