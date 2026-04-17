pub mod webcam;

#[cfg(target_family = "wasm")]
pub use usb_wasm_bindings as bindings;

#[cfg(target_family = "wasm")]
mod wasm_component {
    use super::bindings;
    use bindings::exports::component::wasm_usb_app::raw_frame_stream::Guest;

    struct WebcamComponent;

    impl Guest for WebcamComponent {
        type FrameSource = super::webcam::WebcamFrameStream;
    }

    // Use the export-only world macro to avoid importing raw-frame-stream
    bindings::export_webcam_export_only!(WebcamComponent with_types_in bindings);
}

pub fn run() -> anyhow::Result<()> {
    webcam::run_webcam()
}
