pub mod webcam;

// On wasm32-wasip2 targets, generate WIT bindings inline from the canonical WIT.
// The webcam-guest world imports component:usb@0.2.1 USB interfaces + WASI CLI,
// and exports wasi:cli/run so the component can be invoked as a command.
#[cfg(target_family = "wasm")]
pub mod bindings {
    wit_bindgen::generate!({
        world: "webcam-guest",
        path: "../../wit",
        pub_export_macro: true,
        generate_all,
    });
}

// Wasm component entry point: export wasi:cli/run, call run_webcam().
#[cfg(target_family = "wasm")]
mod wasm_component {
    struct WebcamGuest;

    impl crate::bindings::exports::wasi::cli::run::Guest for WebcamGuest {
        fn run() -> Result<(), ()> {
            super::webcam::run_webcam().map_err(|e| {
                eprintln!("webcam error: {e}");
            })
        }
    }

    crate::bindings::export!(WebcamGuest with_types_in crate::bindings);
}
