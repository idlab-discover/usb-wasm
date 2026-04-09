pub mod cguest;

// Re-export common functions and types from the various interfaces
// Use specific re-exports for root to avoid ambiguity
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

// Re-export the export! macro
pub use crate::cguest::export;

// Maintain module structure for explicitly qualified access
pub mod device {
    pub use crate::cguest::component::usb::device::*;
}

pub mod transfers {
    pub use crate::cguest::component::usb::transfers::*;
}

pub mod descriptors {
    pub use crate::cguest::component::usb::descriptors::*;
}

pub mod configuration {
    pub use crate::cguest::component::usb::configuration::*;
}

pub mod errors {
    pub use crate::cguest::component::usb::errors::*;
}

pub mod cv {
    pub use crate::cguest::component::usb::cv::*;
}

// Re-export WASI interfaces (imports)
pub mod wasi {
    pub use crate::cguest::wasi::*;
}

// Re-export component/exports for provider components (like yolo-detector)
pub mod component {
    pub use crate::cguest::component::*;
}

pub mod exports {
    pub use crate::cguest::exports::*;
}

// Map 'types' to where the common records are defined (mostly descriptors/transfers)
pub mod types {
    pub use crate::cguest::component::usb::cv::{BoundingBox, Detection, Frame, Point, Size};
    pub use crate::cguest::component::usb::descriptors::{
        ConfigurationDescriptor, DeviceDescriptor, EndpointDescriptor, InterfaceDescriptor,
    };
    pub use crate::cguest::component::usb::device::{DeviceLocation, UsbSpeed};
    pub use crate::cguest::component::usb::transfers::{
        IsoPacket, IsoPacketStatus, TransferOptions, TransferResult, TransferSetup, TransferType,
    };
}
