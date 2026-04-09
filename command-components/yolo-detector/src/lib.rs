//! YOLOv8 Object Detection Component (Thin Client)
//!
//! This component:
//! - Imports `frame-stream` from the linked webcam-cv component (guest-to-guest)
//! - Imports `object-detector` from the host (native YOLO inference via tract-onnx)
//! - Calls read-frame → detect in a loop and prints results to stdout

use anyhow::Result;

#[cfg(target_family = "wasm")]
use usb_wasm_bindings::component::usb::cv::{FrameStream, ObjectDetector};

#[cfg(target_family = "wasm")]
pub fn run_detection(model_path: &str) -> Result<()> {
    println!("Initializing YOLOv8 detector via Host...");
    let detector = ObjectDetector::new(model_path);

    let camera_index = 0;
    println!("Initializing FrameStream for camera #{}...", camera_index);
    let stream = FrameStream::new(camera_index);

    println!("Detection loop started. Press Ctrl+C to stop.");
    loop {
        match stream.read_frame() {
            Ok(frame) => {
                match detector.detect(&frame) {
                    Ok(detections) => {
                        print!("\x1B[2J\x1B[H");
                        println!("Detected {} objects:", detections.len());
                        for det in &detections {
                            println!(
                                "- {}: {:.1}% at ({}, {})",
                                det.label,
                                det.confidence * 100.0,
                                det.box_.origin.x,
                                det.box_.origin.y,
                            );
                        }
                    }
                    Err(e) => eprintln!("Detection error: {}", e),
                }
            }
            Err(e) => eprintln!("Capture error: {}", e),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
pub fn run_detection(_model_path: &str) -> Result<()> {
    anyhow::bail!("yolo-detector must be compiled for wasm32")
}
