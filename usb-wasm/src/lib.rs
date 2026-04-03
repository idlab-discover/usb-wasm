use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiView, IoView, WasiCtx};
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use tract_onnx::prelude::*;

pub mod usb_backend;
pub use usb_backend::{HostUsbBackend, LibusbBackend, UsbDevice, UsbDeviceHandle};

pub mod bindings {
    wasmtime::component::bindgen!({
        world: "host",
        path: "wit",
        with: {
            "component:usb/transfers/transfer": crate::UsbTransfer,
            "component:usb/device/usb-device": crate::UsbDevice,
            "component:usb/device/device-handle": crate::UsbDeviceHandle,
            "component:usb/cv/frame-stream": crate::FrameStream,
            "component:usb/cv/object-detector": crate::ObjectDetector,
        },
        async: {
            only_imports: ["await-transfer"]
        },
    });
}

pub use bindings::component::usb;
pub use usb::errors::LibusbError;
pub use usb::device::UsbSpeed;

impl LibusbError {
    pub fn from_raw(code: i32) -> Self {
        match code {
            -1 => LibusbError::Io,
            -2 => LibusbError::InvalidParam,
            -3 => LibusbError::Access,
            -4 => LibusbError::NoDevice,
            -5 => LibusbError::NotFound,
            -6 => LibusbError::Busy,
            -7 => LibusbError::Timeout,
            -8 => LibusbError::Overflow,
            -9 => LibusbError::Pipe,
            -10 => LibusbError::Interrupted,
            -11 => LibusbError::NoMem,
            -12 => LibusbError::NotSupported,
            _ => LibusbError::Other,
        }
    }
}

impl UsbSpeed {
    pub fn from_raw(code: u8) -> Self {
        match code {
            1 => UsbSpeed::Low,
            2 => UsbSpeed::Full,
            3 => UsbSpeed::High,
            4 => UsbSpeed::Super,
            5 => UsbSpeed::SuperPlus,
            /* libusb speed values are slightly different but let's map reasonably */
            _ => UsbSpeed::Unknown,
        }
    }
}

// --- Host Types ---

pub struct UsbTransfer {
    pub transfer: *mut libusb1_sys::libusb_transfer,
    pub completed: Arc<AtomicBool>,
    pub buffer: Vec<u8>,
    pub buf_len: u32,
    pub iso_packet_results: Arc<Mutex<Option<Vec<(u32, i32)>>>>,
}

impl UsbTransfer {
    pub fn submit(&self) -> Result<(), LibusbError> {
        unsafe {
            let res = libusb1_sys::libusb_submit_transfer(self.transfer);
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            Ok(())
        }
    }

    pub fn cancel(&self) -> Result<(), LibusbError> {
        unsafe {
            let res = libusb1_sys::libusb_cancel_transfer(self.transfer);
            if res < 0 && res != -10 { // Ignore NOT_FOUND (already completed/cancelled)
                return Err(LibusbError::from_raw(res));
            }
            Ok(())
        }
    }
}

impl Drop for UsbTransfer {
    fn drop(&mut self) {
        unsafe {
            libusb1_sys::libusb_free_transfer(self.transfer);
        }
    }
}

unsafe impl Send for UsbTransfer {}
unsafe impl Sync for UsbTransfer {}

pub struct FrameStream {
    pub index: u32,
    pub handle: Option<UsbDeviceHandle>,
    pub iface_num: u8,
    pub ep_addr: u8,
}

pub struct ObjectDetector {
    pub model_path: String,
    pub model: Option<SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct USBDeviceIdentifier {
    pub vendor_id: u16,
    pub product_id: u16,
}

#[derive(Debug, Clone)]
pub enum AllowedUSBDevices {
    Allowed(Vec<USBDeviceIdentifier>),
    Denied(Vec<USBDeviceIdentifier>),
}

impl AllowedUSBDevices {
    pub fn is_allowed(&self, device: &USBDeviceIdentifier) -> bool {
        match self {
            Self::Allowed(devices) => devices.contains(device),
            Self::Denied(devices) => !devices.contains(device),
        }
    }
}

pub struct CallLog {
    pub function_name: String,
    pub start_time: std::time::Instant,
    pub duration: std::time::Duration,
    pub buffer_size: Option<usize>,
}

pub struct UsbHostState {
    pub table: ResourceTable,
    pub wasi_ctx: wasmtime_wasi::WasiCtx,
    pub allowed_usbdevices: AllowedUSBDevices,
    pub backend: Box<dyn HostUsbBackend>,
    pub enable_yolo: bool,
    pub call_logs: Arc<Mutex<Vec<CallLog>>>,
}

impl UsbHostState {
    pub fn log_call(&self, name: &str, start: std::time::Instant, buf_size: Option<usize>) {
        if let Ok(mut logs) = self.call_logs.lock() {
            logs.push(CallLog {
                function_name: name.to_string(),
                start_time: start,
                duration: start.elapsed(),
                buffer_size: buf_size,
            });
        }
    }
}

// --- The "View" Pattern for Wasmtime v31 ---

/// A view into the host state that satisfies the ResourceView requirement
/// without conflicting with wasmtime-wasi's blanket implementation.
pub struct UsbView<'a>(pub &'a mut UsbHostState);

impl<'a> IoView for UsbView<'a> {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.0.table
    }
}

impl<'a> WasiView for UsbView<'a> {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.0.wasi_ctx
    }
}

// The Host traits will be implemented for UsbView<'a> in host_impl.rs

mod host_impl;
pub use host_impl::*;
