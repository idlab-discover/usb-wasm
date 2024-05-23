# USB-WASM
Prototype design and implementation of an interface for accessing and communicating with USB devices from a WebAssembly Component. This project forms part of my Master's thesis.

## Repository structure
- [`./wit/`](./wit/) contains the interface definition in the WIT IDL.
- [`./usb-wasm`](./usb-wasm/) contains the host implementation of the interface for Wasmtime.
- [`./usb-wasm-bindings`](./usb-wasm-bindings/) contains the automatically generated Rust bindings. This crate is used in the example/test command components.
- [`./command-components`](./command-components/) contains a couple of command components implemented using the interface.
- [`./wasmtime-usb`]() implements a Component Model-enabled Wasmtime CLI application to run command components that use the interface.

## Requirements
- Rust
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
- [just](https://just.systems/) (optional, to run the commands in the Justfile)

## Running the command components
Because Rust can only target WASI preview 1 right now, the compiled WASM binaries first need to be transformed into command components before they can be run by the Component Model-enabled Wasmtime CLI.

`wasm-tools` is used to 'adapt' the WASM binaries compiled by rustc into command components.

For example, to run the `lsusb` command component, the following commands have to be executed:
```
> cargo build -p lsusb --target=wasm32-wasi 
> wasm-tools component new ./target/wasm32-wasi/debug/lsusb.wasm --adapt ./command-components/wasi_snapshot_preview1.command.wasm -o out/lsusb.wasm
> cargo run -- ./out/lsusb.wasm
```

If you have the `just` command runner installed, you can just run `just lsusb`, which will perform all these steps for you.