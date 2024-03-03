# USB-WASM
Prototype design and implementation of an interface for accessing and communicating with USB devices from a WebAssembly Component. This project forms part of my Master's thesis.

## Repository structure
- [`./wit/`](./wit/) contains the interface definition in the WIT IDL.
- [`./usb-wasm`](./usb-wasm/) contains the host implementation of the interface for Wasmtime.
- [`./usb-wasm-bindings`](./usb-wasm-bindings/) contains the automatically generated Rust bindings. This crate is used in the example/test command components.
- [`./command-components`](./command-components/) contains a couple of command components implemented using the interface.
- [`./library-components`](./library-components/) is currently empty.
- The root crate implements a Component Model-enabled Wasmtime CLI application to run command components that use the interface.

## Requirements
- Rust
- [just](https://just.systems/)
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)

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

## TODO
The WIT interfaces are not yet finished. Most of the functionality is there, but errors and hotplug support are still missing.

## Open questions
The main purpose of this repo right now is to request feedback on the interface from the community. In addition to this, I have some open design questions that I would also like community input on.

### Should transfer functions take an endpoints resource or a u8? (Same question for configurations and interfaces)
Right now, the transfer functions and friends take an endpoint/interface/configuration resource. The idea being that this prevents a consumer of the interface from passing a non-existing endpoint to the function. Another benefit of this design is that we can implement access control at the configuration/interface/endpoint level, by preventing a consumer from getting these resources.

However, this design doesn't currently prevent a user from passing in a wrong endpoint anyway (like passing an Out endpoint to a read function, or an interrupt endpoint to an isochronous transfer function).
Additionally, it forces a consumer that already knows which endpoint address they need to read or write from to write additional boilerplate to get the endpoint resource they need (as can be seen in the components in [`./command-components`](./command-components/).

`read-interrupt: func(endpoint: borrow<usb-endpoint>) -> list<u8>;`

The alternative would be to pass in `u8`'s representing the endpoint address instead. 

`read-interrupt: func(endpoint: u8) -> list<u8>;`

This would follow other APIs like libusb or WebUSB, and eliminate the boilerplate for getting the endpoint resources. 

As I don't have much experience with writing USB drivers, I'm not sure how useful it would be to do access control on the level of configurations/interfaces/endpoints, in addition to access control at the device level.