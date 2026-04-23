// USB guest bindings for component:usb@0.2.1 guests.
//
// `cguest.rs` is pre-generated from the `cguest` WIT world and provides all
// USB import types + the wasi:cli/run export macro used by C benchmark components.
//
// Webcam Rust guests use inline wit_bindgen::generate! (see command-components/webcam/src/lib.rs)
// and no longer depend on this crate.

#[cfg(feature = "usb-guest")]
pub mod cguest;

// Re-export the core `export!` macro for C benchmark components.
#[cfg(feature = "usb-guest")]
pub use crate::cguest::export;

// Top-level convenience re-exports used by lsusb, streams-test, enumerate-devices-rust, etc.
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

#[cfg(feature = "usb-guest")]
pub mod component {
    pub mod usb {
        pub use crate::cguest::component::usb::configuration;
        pub use crate::cguest::component::usb::descriptors;
        pub use crate::cguest::component::usb::device;
        pub use crate::cguest::component::usb::errors;
        pub use crate::cguest::component::usb::transfers;
    }
}
