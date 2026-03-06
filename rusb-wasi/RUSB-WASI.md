# rusb-wasi

`rusb-wasi` is a Rust wrapper for USB device access, forked from the original `rusb` crate. It has been adapted to seamlessly support cross-compilation to WebAssembly (`wasm32-wasip2`).

## Role in the Project
While `rusb` natively links against the system `libusb`, `rusb-wasi` uses a custom `pkg-config` sysroot to link against our WebAssembly-compatible **`libusb-wasi`**. 

This allows Rust developers to write safe, idiomatic Rust code for USB interaction and compile their entire workload to a WASI-Component. The resulting component can then be executed by the `wasi-usb` host runtime.

## Cross-Compiling to WASI

No changes are required in your application code. The adaptation happens purely in the build configuration:

1. Ensure the `wasi-sysroot` is correctly set up with the generated `libusb-wasi.a` and component metadata (`cguest_component_type.o`).
2. Set the necessary `pkg-config` environment variables.
3. Compile using the `wasm32-wasip2` target:
   ```bash
   cargo build --target wasm32-wasip2 --release
   ```

*(See [COMPILING.md](../wasi-usb/COMPILING.md) or the master thesis documentation for the exact sysroot and pkg-config setup).*