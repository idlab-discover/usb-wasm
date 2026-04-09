# usb-wasm

`usb-wasm` is the core host-side implementation for the WASI-USB WebAssembly interface. It provides the necessary logic to interface between the host's USB stack (via `libusb`) and WebAssembly components.

## Core Responsibilities

1. **Host State Management**: Maintains the `MyState` struct, which tracks all active USB devices, handles, and transfers requested by guest components.
2. **WIT Implementation**: Implements the `Host` traits generated from the WIT definitions in the `wit/` directory.
3. **Resource Mapping**: Translates between WebAssembly opaque resource IDs and real host-side USB resources.
4. **Asynchronous I/O**: Handles asynchronous USB transfers using Wasmtime's async support, allowing non-blocking I/O for guest components.

## Key Components

- **`src/lib.rs`**: Entry point for the host library. Contains the `bindgen!` macro configured for Wasmtime 31.0.0 and the `add_to_linker` helper function.
- **`src/host_impl.rs`**: Unified implementation of all WASI-USB traits for `MyState`. This is the single source of truth for the host logic.
- **`src/usb_backend.rs`**: High-level wrapper around the underlying USB backend (e.g., `libusb`).
- **`src/error.rs`**: Shared error definitions for the WASI-USB interface.

## Integration with Wasmtime

To use this host implementation in a Wasmtime runtime, use the `add_to_linker` function:

```rust
use usb_wasm::add_to_linker;

// ... set up your Linker and Store with MyState ...

add_to_linker(&mut linker, |state: &mut MyState| state)?;
```

## Generic Transport Vision

`usb-wasm` is designed as an **abstract USB transport layer**. It follows the principle that the host handles high-level USB management, while the guest handles device-specific protocols:
- **Host**: Generic device discovery, memory management for transfers, and raw UVC/isochronous pipe transport.
- **Guest**: UVC negotiation, framerate settings, and reassembly of isochronous packets into video frames.

This separation ensures that the host remains lightweight and generic, while allowing guests to implement specialized USB device logic without host-side modification.
