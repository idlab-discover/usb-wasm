# DualSense (PS5) Pacman Maze (WASI-USB)

This component implements a complete maze game, modeled after **Pacman**, that uses a **PS5 DualSense** (or Xbox) controller for real-time input. It demonstrates the capabilities of **WASI-USB** and `rusb-wasi` to handle complex industrial/gaming peripherals from within a sandboxed WebAssembly environment.

## Overview

The `ps5-maze` component interacts directly with the USB HID reports of a connected gamepad. It parses the raw button and axis data, calculates character movement, and implements Ghost AI that mimics the original Pacman targeting logic.

## Key Features

- **Direct HID Parsing**: Connects to the DualSense controller (`054c:0ce6`) or Xbox controller (`045e:02ea`) and parses the report maps directly.
- **Ghost AI Personality**:
  - **Blinky**: Directly targets Pacman's current position (Chasing).
  - **Pinky**: Targets a position 2 tiles ahead of Pacman (Ambushing).
  - **Inky/Clyde**: Specialized targeting modes (Patrolling/Randomized).
- **ANSI Console Rendering**: Real-time rendering of the game field in the terminal using ANSI escape codes.
- **Single-threaded Polling**: Uses non-blocking USB reads (`50ms` timeout) to keep the game loop and AI active without blocking the main thread.

## Technical Details

### Controller Mapping
The component parses the `Interrupt IN` endpoint data. For the PS5 controller, it specifically claims **Interface 03** for HID reports.
- **Byte 1-4**: Analog Stick Axes (normalized to `-1.0` to `1.0`).
- **Byte 8**: D-Pad (Hat switch) and Action Buttons.

### Ghost Logic
The ghosts follow the original AI logic: they cannot perform 180-degree turns and always choose the direction that minimizes the Euclidean distance to their target tile.

## Running the Maze

This component can be executed via the `just` command in the `usb-wasm/` directory.

```bash
# Ensure your controller is connected via USB
just build-ps5-maze
just ps5-maze
```

---
Original research and implementation by the **contributors**!
Licensed under the **MIT License**.
