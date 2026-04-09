use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiView, IoView, WasiCtx};
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;

pub mod usb_backend;
pub use usb_backend::{HostUsbBackend, LibusbBackend, UsbDevice, UsbDeviceHandle};

wasmtime::component::bindgen!({
    world: "host",
    path: "../wit",
    with: {
         "component:usb/transfers@0.2.1/transfer": crate::UsbTransfer,
         "component:usb/device@0.2.1/usb-device": crate::UsbDevice,
         "component:usb/device@0.2.1/device-handle": crate::UsbDeviceHandle,
         "component:usb/cv/frame-stream": crate::host_impl::FrameStreamInnerStub,
         "component:usb/cv/object-detector": crate::host_impl::ObjectDetectorInner,
    },
});


pub use self::component::usb::errors::LibusbError;
pub use self::component::usb::device::UsbSpeed;

impl LibusbError {
    pub fn from_raw(res: i32) -> Self {
        match res {
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
            _ => UsbSpeed::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
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
            // CRITICAL: Synchronize the libusb_transfer buffer pointer with our internal Vec.
            // If the buffer was updated or moved, the old pointer in transfer-struct is dangling.
            (*self.transfer).buffer = self.buffer.as_ptr() as *mut u8;
            
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
            if res < 0 {
                return Err(LibusbError::from_raw(res));
            }
            Ok(())
        }
    }
}

unsafe impl Send for UsbTransfer {}
unsafe impl Sync for UsbTransfer {}

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

pub struct MyState {
    pub table: ResourceTable,
    pub wasi_ctx: wasmtime_wasi::WasiCtx,
    pub allowed_usbdevices: AllowedUSBDevices,
    pub backend: Box<dyn HostUsbBackend>,
    pub call_logs: Arc<Mutex<Vec<CallLog>>>,
}

impl MyState {
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

impl IoView for MyState {
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for MyState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi_ctx
    }
}

pub fn add_to_linker<T>(
    linker: &mut wasmtime::component::Linker<T>,
    get: impl Fn(&mut T) -> &mut MyState + Send + Sync + Copy + 'static,
) -> wasmtime::Result<()> 
where T: Send + 'static
{
    println!("[WASI-USB-HOST] Adding WASI-USB interfaces to linker...");
    Host_::add_to_linker::<T, MyState>(linker, get)
}

mod host_impl;
