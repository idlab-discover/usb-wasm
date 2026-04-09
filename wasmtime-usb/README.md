# wasmtime-usb

`wasmtime-usb` is a custom WebAssembly runtime built on top of Wasmtime that provides out-of-the-box support for the WASI-USB interface.

## Overview

While standard Wasmtime provides WASI support for filesystem, network, and CLI, it does not natively support USB devices. `wasmtime-usb` bridges this gap by:
1. Initializing a `MyState` object which manages USB resources.
2. Linking the `usb-wasm` host implementation into the Wasmtime `Linker`.
3. Providing a CLI interface similar to `wasmtime run`, but with USB capabilities enabled.

## Features

- **USB Access**: Allows guest components to enumerate, open, and interact with USB devices.
- **WASI Support**: Includes standard WASI Preview 2 support (filesystem, exit, etc.).
- **Async Runtime**: Built on Wasmtime's asynchronous engine for efficient I/O.
- **Visibility**: Provides explicit `[WASI-USB-HOST]` logs in the terminal to confirm host-guest interaction.
- **Easy Integration**: Uses the `usb-wasm` library to provide a clean separation between the runner logic and the USB implementation.

## Usage

### Running a Component
To run a WebAssembly Component (`.wasm`) with USB access:

```shell
sudo ./target/debug/wasmtime-usb path/to/component.wasm
```

### Options
`wasmtime-usb` inherits standard Wasmtime behavior but is pre-configured for the WASI-USB world.

## Development

The runner is located in `src/bin/wasmtime-usb.rs`. It performs the following steps:
1. Configures the Wasmtime `Engine` with component model and async support.
2. Sets up a `Linker` and adds WASI Preview 2 and WASI-USB implementations to it.
3. Creates a `Store` with `MyState`.
4. Instantiates the component and calls its entry point via the `wasi:cli/run` interface.
