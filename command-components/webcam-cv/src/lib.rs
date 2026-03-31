// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

//! Webcam Computer Vision Component
//!
//! This component provides functionality for capturing frames from a webcam
//! using WASI-USB and exposing them through the WASI-CV interface.

pub mod webcam;

#[allow(dead_code)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "cv-world",
    });
}

#[cfg(target_family = "wasm")]
mod wasm_component {
    use super::bindings;
    use bindings::exports::component::usb::cv::{Guest, GuestFrameStream, GuestObjectDetector, Frame, Detection};
    use bindings::exports::wasi::cli::run::Guest as RunGuest;

    struct WebcamComponent;

    impl Guest for WebcamComponent {
        type FrameStream = super::webcam::WebcamFrameStream;
        type ObjectDetector = super::webcam::UnimplementedObjectDetector;

        fn open_webcam() -> Result<Self::FrameStream, String> {
            super::webcam::open_webcam_stream()
        }

        fn create_object_detector(_model: String) -> Result<Self::ObjectDetector, String> {
            Err("Object detection not implemented in this component".to_string())
        }
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
