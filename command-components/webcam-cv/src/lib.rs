pub mod webcam;

// De WIT-bindingen en WASI export zijn enkel geldig in een WASM component.
// Op een native target (bijv. aarch64-apple-darwin) bestaan de WASI-symbolen
// niet, waardoor de linker faalt. Vandaar de cfg-guard.
#[cfg(target_family = "wasm")]
mod wasm_component {
    use usb_wasm_bindings as bindings;
    use bindings::exports::wasi::cli::run::Guest;

    struct WebcamComponent;

    impl Guest for WebcamComponent {
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

    bindings::export!(WebcamComponent);
}

pub fn run() -> anyhow::Result<()> {
    webcam::run_webcam()
}