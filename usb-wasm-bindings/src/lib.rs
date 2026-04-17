// Re-export common types for guests to use.
// This library provides a centralized location for WIT-generated bindings.
//
// The `cguest` module provides the core USB bindings used by all legacy
// command components (lsusb, ps5-maze, xbox, ping, mass-storage, etc.).
//
// Feature flags select additional worlds for composition:
//   webcam-provider  →  webcam_provider_world (USB bindings via component::usb)
//   yolo-command     →  yolo_command_world (raw-frame-stream import stubs)
//
// IMPORTANT: yolo_command_world generates raw-frame-stream import stubs.
// Do NOT compile it into the webcam binary or wac plug will see webcam as
// both importing and exporting raw-frame-stream (circular dependency).

// Core USB guest bindings — used by all legacy USB command components.
// These are enabled by default but should be disabled when using other
// worlds that also export wasi:cli/run (like yolo-command).
#[cfg(feature = "usb-guest")]
pub mod cguest;

// Re-export the core `export!` macro for legacy components
#[cfg(feature = "usb-guest")]
pub use crate::cguest::export;

// Top-level convenience re-exports (used by lsusb, ps5-maze, xbox, ping, etc.)
#[cfg(feature = "usb-guest")]
pub mod usb_reexports {
    pub use crate::cguest::component::usb::configuration::ConfigValue;
    pub use crate::cguest::component::usb::descriptors::{
        ConfigurationDescriptor, DeviceDescriptor, EndpointDescriptor, InterfaceDescriptor,
    };
    pub use crate::cguest::component::usb::device::{
        list_devices, DeviceHandle, DeviceLocation, UsbDevice, UsbSpeed,
    };
    pub use crate::cguest::component::usb::errors::LibusbError;
    pub use crate::cguest::component::usb::transfers::{
        await_transfer, Transfer, TransferOptions, TransferResult, TransferSetup, TransferType,
    };
}
#[cfg(feature = "usb-guest")]
pub use usb_reexports::*;

#[cfg(feature = "usb-guest")]
pub mod device {
    pub use crate::cguest::component::usb::device::*;
}
#[cfg(feature = "usb-guest")]
pub mod transfers {
    pub use crate::cguest::component::usb::transfers::*;
}
#[cfg(feature = "usb-guest")]
pub mod descriptors {
    pub use crate::cguest::component::usb::descriptors::*;
}
#[cfg(feature = "usb-guest")]
pub mod configuration {
    pub use crate::cguest::component::usb::configuration::*;
}
#[cfg(feature = "usb-guest")]
pub mod errors {
    pub use crate::cguest::component::usb::errors::*;
}

// ── Composition worlds ────────────────────────────────────────────────────────

// webcam_export_only provides the export-only ABI (GuestFrameSource trait + export macro).
// We only compile it if webcam-provider is NOT enabled to avoid macro collisions
// because webcam_provider_world already includes the same exports.
#[cfg(not(feature = "webcam-provider"))]
pub mod webcam_export_only;
#[cfg(not(feature = "webcam-provider"))]
pub use crate::__export_webcam_export_only_impl as export_webcam_export_only;

// webcam-provider: full USB bindings via webcam_provider_world.
// Include when building the webcam component.
#[cfg(feature = "webcam-provider")]
pub mod webcam_provider_world;
#[cfg(feature = "webcam-provider")]
pub use crate::__export_webcam_provider_world_impl as export_webcam_provider_world;

// Re-export the webcam export macro under the same name so users can use either.
#[cfg(feature = "webcam-provider")]
pub use export_webcam_provider_world as export_webcam_export_only;

// USB bindings re-exported through webcam_provider_world (for the webcam component).
// Only present with the webcam-provider feature so yolo-detector never links USB stubs.
#[cfg(feature = "webcam-provider")]
pub mod component {
    pub mod usb {
        pub use crate::webcam_provider_world::component::usb::configuration;
        pub use crate::webcam_provider_world::component::usb::descriptors;
        pub use crate::webcam_provider_world::component::usb::device;
        pub use crate::webcam_provider_world::component::usb::errors;
        pub use crate::webcam_provider_world::component::usb::transfers;
    }
}

// yolo-command: raw-frame-stream import stubs for the yolo-detector consumer.
// MUST NOT be compiled into the webcam binary.
#[cfg(feature = "yolo-command")]
pub mod yolo_command_world;
#[cfg(feature = "yolo-command")]
pub use crate::__export_yolo_command_world_impl as export_yolo_command_world;

// Export types for the wasm-usb-app package.
pub mod exports {
    pub mod component {
        pub mod wasm_usb_app {
            #[cfg(not(feature = "webcam-provider"))]
            pub use crate::webcam_export_only::exports::component::wasm_usb_app::raw_frame_stream;
            #[cfg(feature = "webcam-provider")]
            pub use crate::webcam_provider_world::exports::component::wasm_usb_app::raw_frame_stream;
        }
    }
    #[cfg(feature = "yolo-command")]
    pub mod wasi {
        pub mod cli {
            pub use crate::yolo_command_world::exports::wasi::cli::run;
        }
    }
}

// Frame transport types — only for yolo-command consumers.
// NOT available in webcam builds to prevent import stubs from leaking in.
#[cfg(feature = "yolo-command")]
pub mod frame_transport {
    pub use crate::yolo_command_world::component::wasm_usb_app::raw_frame_stream::{
        FrameSource, RawFrame,
    };
}
