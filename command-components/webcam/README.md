# Webcam Computer Vision Component (WASI-USB)

This component demonstrates low-level **UVC (USB Video Class)** webcam capture and isochronous transfer management, all within a sandboxed WebAssembly environment via the **WASI-USB** interface.

## Overview

The `webcam-cv` component implements a specialized video capture pipeline that interacts directly with USB endpoints of a UVC-compatible camera. It bypasses OS-level drivers (like Nokhwa or OpenCV) and communicates with the WASI-USB host-side interfaces to trigger isochronous IN transfers.

## Key Features

- **Isochronous Reassembly**: Manages raw USB packets and reassembles them into complete frames by tracking UVC payload header flags (FID toggle, EOF bit).
- **MJPEG & YUYV Support**: Decodes both MJPEG and YUYV (YUV 4:2:2) pixel formats.
- **ASCII Art Rendering**: Real-time rendering of the webcam feed as ASCII art in the terminal for instant verification.
- **WebAssembly Implementation**: Fully compliant with the `wasm32-wasip2` target, interacting securely with physical USB hardware.

## Technical Details

### Isochronous Pipeline
The component issues isochronous transfers with 32 packets per transfer, using a packet stride equal to the endpoint's Maximum Packet Size (MPS).

### UVC Handshake
The component performs a strict UVC 1.1 `Probe/Commit` handshake to negotiate the stream parameters (format, resolution, frame rate) before starting the stream.

## Running the Webcam CV

This component can be executed via the `just` command in the `usb-wasm/` directory:

```bash
just build-webcam-cv
just webcam-cv
```

---
Original research and implementation by the **contributors**!
Licensed under the **MIT License**.
