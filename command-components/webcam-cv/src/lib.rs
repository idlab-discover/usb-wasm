// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

//! Webcam Computer Vision Component
//!
//! Captures UVC frames entirely in-guest (negotiation, isochronous reassembly)
//! and exports them via the `frame-stream` WIT resource.

pub mod webcam;

#[cfg(target_family = "wasm")]
pub use usb_wasm_bindings as bindings;

#[cfg(target_family = "wasm")]
mod wasm_component {
    use super::bindings;
    use bindings::exports::component::usb::cv::Guest;
    use bindings::exports::wasi::cli::run::Guest as RunGuest;

    struct WebcamComponent;

    impl Guest for WebcamComponent {
        type FrameStream = super::webcam::WebcamFrameStream;
        type ObjectDetector = super::webcam::UnimplementedObjectDetector;
    }

    impl RunGuest for WebcamComponent {
        fn run() -> Result<(), ()> {
            match super::webcam::run_webcam() {
                Ok(_) => Ok(()),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    Err(())
                }
            }
        }
    }

    bindings::export!(WebcamComponent with_types_in bindings);
}

pub fn run() -> anyhow::Result<()> {
    webcam::run_webcam()
}
