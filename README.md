# USB-WASM

This repository contains WebAssembly components, WIT interface definitions, and host implementations for the WASI-USB ecosystem. It includes a variety of demonstrations, from simple USB enumeration to real-time Computer Vision pipelines.

Original research and implementation by **IDLab Discover**.

## Project Structure

- **[`wit/`](./wit/)**: Formal interface definitions (WIT) for USB operations, descriptors, transfers, and Computer Vision (CV).
- **[`command-components/`](./command-components/)**: Guest components implementing specific USB and CV logic (e.g., `lsusb`, `yolo-detector`).
- **[`usb-wasm-bindings/`](./usb-wasm-bindings/)**: Automatically generated Rust bindings for the WIT interfaces.
- **[`wasmtime-usb/`](./wasmtime-usb/)**: A custom Wasmtime-based runtime that incorporates the WASI-USB host implementation.

## Requirements

- Rust
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
- [just](https://just.systems/)

## Building

We use `just` as build system. You can see all supported commands by running `just --list`. You can build the modified wasmtime runtime with the following command.

```shell
just build-runtime
```

You can also use `just` to build any of the guest components (command components). For example, to build the xbox pacman demo, run the following.

```shell
just build-xbox-maze
```

Afterwards, you can run this demo using the following command.

```shell
sudo ./target/debug/wasmtime-usb out/xbox-maze.wasm
```

## Building components manually

Instead of using `just`, you can also build the components manually using the following method.

Because Rust can only target WASI preview 1 right now, the compiled WASM binaries first need to be transformed into command components before they can be run by the Component Model-enabled Wasmtime CLI.

`wasm-tools` is used to 'adapt' the WASM binaries compiled by rustc into command components.

For example, to run the `lsusb` command component, the following commands have to be executed:

```shell
> cargo build -p lsusb --target=wasm32-wasi 
> wasm-tools component new ./target/wasm32-wasi/debug/lsusb.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/lsusb.wasm
> cargo run -- ./out/lsusb.wasm
```

If you have the `just` command runner installed, you can just run `just lsusb`, which will perform all these steps for you.

## Best xbox pacman performance

Since the game uses CLI output, it needs to write this line per line. This might cause some flickering. For best performance, use Alacritty with a font size of 30 and in fullscreen.

## Funding information

This work has been partially supported by the ELASTIC project, which received funding from the Smart Networks and Services Joint Undertaking (SNS JU) under the European Union’s Horizon Europe research and innovation programme under Grant Agreement No 101139067. Views and opinions expressed are however those of the author(s) only and do not necessarily reflect those of the European Union. Neither the European Union nor the granting authority can be held responsible for them.
