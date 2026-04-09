# Xbox Maze Component

A WASI component that implements a simple Maze game playable with an Xbox or PS5 controller over USB.

## Features
- Hardware-abstracted USB access via WASI-USB (WIT v0.2.1)
- Supports Xbox One/Series (0x045e:0x02ea) and DualSense PS5 (0x054c:0x0ce6)
- Real-time input handling using the `new_transfer` / `await_transfer` async pattern

## Prerequisites
- Rust with `wasm32-wasip1` target
- `wit-bindgen-cli` 0.42.1
- [wasmtime-usb-cli](../../wasmtime-usb) (Host side)

## Building
Use the workspace `just` command from the root directory:
```bash
just build-xbox-maze
```
This will compile the component to `target/wasm32-wasip1/release/xbox_maze.wasm`.

## Running
Run using the local `wasmtime-usb` host:
```bash
just run-xbox-maze
```
Ensure your controller is connected and you have permissions to access USB devices.
