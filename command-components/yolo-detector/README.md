# YOLOv8 Object Detector (WASI-USB)

This component implements a real-time object detection pipeline using the **YOLOv8** model, executed within a sandboxed WebAssembly environment via the **WASI-USB** interface.

## Overview

The `yolo-detector` leverages the `tract-onnx` crate to perform inference on a pre-trained YOLOv8 model (`.onnx`). It communicates with the host runtime to capture frames from a USB webcam and outputs detection results (bounding boxes, labels, and confidence scores) in JSON format.

## Key Features

- **Tract-ONNX Integration**: Optimized for Wasm execution, allowing for high-performance model inference without native dependencies.
- **Real-time Pipeline**: Captures frames, resizes them to 640x640, performs inference, and applies Non-Maximum Suppression (NMS).
- **Periodic Annotation**: Automatically saves an annotated debug image (`annotated.png`) to the filesystem every 5 seconds.
- **Coordinate Mapping**: Automatically scales detection coordinates from the 640x640 inference space back to the original webcam resolution.

## Technical Details

### Coordinate Scaling
The model outputs normalized coordinates based on its 640x640 input. This component maps these back to the source frame:
```rust
let x = (cx - w / 2.0) * f.width as f32;
let y = (cy - h / 2.0) * f.height as f32;
```

### Non-Maximum Suppression (NMS)
To prevent duplicate detections for the same object, an NMS algorithm with a 0.45 IoU (Intersection over Union) threshold is applied.

## Running the Detector

This component is typically executed via the `usb-wasi-host` with specialized flags:

```bash
cargo component build --release
sudo ../../../target/release/usb-wasi-host \
    --component-path target/wasm32-wasip2/release/yolo_detector.component.wasm \
    --enable-yolo -- yolov8n.onnx
```

---
Original research and implementation by the **contributors**!
Licensed under the **MIT License**.
