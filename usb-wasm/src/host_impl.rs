use crate::bindings::component::usb::device::{Host as HostDevice, HostUsbDevice, HostDeviceHandle, DeviceLocation};
use crate::bindings::component::usb::descriptors::{DeviceDescriptor, ConfigurationDescriptor};
use wasmtime_wasi::IoView;
use crate::bindings::component::usb::configuration::ConfigValue;
use crate::bindings::component::usb::transfers::{Host as HostTransfers, HostTransfer, IsoPacket, IsoPacketStatus, TransferResult, TransferType, TransferSetup, TransferOptions};
use crate::bindings::component::usb::cv::{HostFrameStream, HostObjectDetector, Detection, Frame};
use crate::bindings::component::usb::errors::LibusbError;
use crate::bindings::component::usb::usb_hotplug::{Host as HostHotplug, Event, Info};
use crate::{UsbDevice, UsbDeviceHandle, UsbTransfer, ObjectDetector, FrameStream, UsbView};
use wasmtime::component::Resource;

use std::time::Instant;
use log::info;

const USB_CLASS_VIDEO: u8 = 0x0E;

// --- Host Implementation for UsbView ---

impl<'a> HostDevice for UsbView<'a> {
    fn init(&mut self) -> Result<(), LibusbError> {
        info!("Initializing wasi-usb host...");
        Ok(())
    }

    fn list_devices(&mut self) -> Result<Vec<(Resource<UsbDevice>, DeviceDescriptor, DeviceLocation)>, LibusbError> {
        let start = Instant::now();
        let devices = self.0.backend.list_devices(&self.0.allowed_usbdevices)?;
        let mut result = Vec::new();
        
        for (dev, desc, loc) in devices {
            let res = self.0.table.push(dev).map_err(|_| LibusbError::Other)?;
            result.push((res, desc, loc));
        }

        self.0.log_call("device::list_devices", start, Some(result.len()));
        Ok(result)
    }
}

impl<'a> HostUsbDevice for UsbView<'a> {
    fn open(&mut self, self_: Resource<UsbDevice>) -> Result<Resource<UsbDeviceHandle>, LibusbError> {
        let start = Instant::now();
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let handle = self.0.backend.open(device)?;
        let res = self.0.table.push(handle).map_err(|_| LibusbError::Other)?;
        self.0.log_call("usb_device::open", start, None);
        Ok(res)
    }

    fn get_configuration_descriptor(&mut self, self_: Resource<UsbDevice>, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration_descriptor(device, config_index)
    }

    fn get_configuration_descriptor_by_value(&mut self, self_: Resource<UsbDevice>, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration_descriptor_by_value(device, config_value)
    }

    fn get_active_configuration_descriptor(&mut self, self_: Resource<UsbDevice>) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_active_configuration_descriptor(device)
    }

    fn drop(&mut self, rep: Resource<UsbDevice>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

impl<'a> HostDeviceHandle for UsbView<'a> {
    fn get_configuration(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<u8, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.get_configuration(handle)
    }
    fn set_configuration(&mut self, self_: Resource<UsbDeviceHandle>, config: ConfigValue) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.set_configuration(handle, config)
    }
    fn claim_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.claim_interface(handle, ifac)
    }
    fn release_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.release_interface(handle, ifac)
    }
    fn set_interface_altsetting(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8, alt_setting: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.set_interface_alt_setting(handle, ifac, alt_setting)
    }
    fn clear_halt(&mut self, self_: Resource<UsbDeviceHandle>, endpoint: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.clear_halt(handle, endpoint)
    }
    fn reset_device(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.reset_device(handle)
    }
    fn alloc_streams(&mut self, self_: Resource<UsbDeviceHandle>, num_streams: u32, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.alloc_streams(handle, num_streams, endpoints)
    }
    fn free_streams(&mut self, self_: Resource<UsbDeviceHandle>, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.free_streams(handle, endpoints)
    }
    fn kernel_driver_active(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<bool, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.kernel_driver_active(handle, ifac)
    }
    fn detach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.detach_kernel_driver(handle, ifac)
    }
    fn attach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.0.backend.attach_kernel_driver(handle, ifac)
    }
    fn new_transfer(&mut self, self_: Resource<UsbDeviceHandle>, xfer_type: TransferType, setup: TransferSetup, buf_size: u32, opts: TransferOptions) -> Result<Resource<UsbTransfer>, LibusbError> {
        let handle = self.0.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let xfer = handle.new_transfer(xfer_type, setup, buf_size, opts)?;
        let res = self.0.table.push(xfer).map_err(|_| LibusbError::Other)?;
        Ok(res)
    }
    fn close(&mut self, self_: Resource<UsbDeviceHandle>) {
        let _ = self.0.table.delete(self_);
    }
    fn drop(&mut self, rep: Resource<UsbDeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

// --- Transfers Implementation for UsbView ---

impl<'a> HostTransfers for UsbView<'a> {
    async fn await_transfer(&mut self, xfer: Resource<UsbTransfer>) -> Result<TransferResult, LibusbError> {
        let start = Instant::now();
        let tx = self.table().get(&xfer).map_err(|_| LibusbError::Io)?;
        
        // Polling loop for completion (since we're async in a synchronous context)
        while !tx.completed.load(std::sync::atomic::Ordering::SeqCst) {
             tokio::task::yield_now().await;
        }

        let mut packets = Vec::new();
        if let Some(results) = tx.iso_packet_results.lock().unwrap().clone() {
            for (actual_length, status) in results {
                let status = match status {
                    0 => IsoPacketStatus::Success,
                    1 => IsoPacketStatus::Error,
                    2 => IsoPacketStatus::TimedOut,
                    3 => IsoPacketStatus::Cancelled,
                    4 => IsoPacketStatus::Stall,
                    5 => IsoPacketStatus::NoDevice,
                    6 => IsoPacketStatus::Overflow,
                    _ => IsoPacketStatus::Error,
                };
                packets.push(IsoPacket { actual_length, status });
            }
        }

        let result = TransferResult {
            data: tx.buffer.clone(),
            packets,
        };

        self.0.log_call("transfers::await_transfer", start, Some(result.data.len()));
        Ok(result)
    }
}

impl<'a> HostTransfer for UsbView<'a> {
    fn submit_transfer(&mut self, self_: Resource<UsbTransfer>, data: Vec<u8>) -> Result<(), LibusbError> {
        let xfer = self.0.table.get_mut(&self_).map_err(|_| LibusbError::NotFound)?;
        if !data.is_empty() {
             xfer.buffer = data;
        }
        xfer.submit()
    }
    fn cancel_transfer(&mut self, self_: Resource<UsbTransfer>) -> Result<(), LibusbError> {
        let xfer = self.0.table.get(&self_).map_err(|_| LibusbError::NotFound)?;
        xfer.cancel()
    }
    fn drop(&mut self, rep: Resource<UsbTransfer>) -> wasmtime::Result<()> {
        let _ = self.0.table.delete(rep);
        Ok(())
    }
}

// --- Hotplug Implementation for UsbView ---

impl<'a> HostHotplug for UsbView<'a> {
    fn enable_hotplug(&mut self) -> Result<(), LibusbError> {
        Ok(())
    }
    fn poll_events(&mut self) -> Vec<(Event, Info, Resource<UsbDevice>)> {
        Vec::new()
    }
}

// --- CV Trait Implementation for UsbView ---

impl<'a> HostFrameStream for UsbView<'a> {
    fn new(&mut self, index: u32) -> Resource<FrameStream> {
        self.table().push(FrameStream { index, handle: None, iface_num: 0, ep_addr: 0 }).expect("resource push failed")
    }
    fn read_frame(&mut self, _rep: Resource<FrameStream>) -> Result<Frame, String> {
        Err("Not implemented".to_string())
    }
    fn drop(&mut self, rep: Resource<FrameStream>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep);
        Ok(())
    }
}

impl<'a> HostObjectDetector for UsbView<'a> {
    fn new(&mut self, model_path: String) -> Resource<ObjectDetector> {
        info!("Loading YOLO model from {}", model_path);
        // This is a placeholder for actual tract integration
        self.table().push(ObjectDetector { model_path, model: None }).expect("resource push failed")
    }
    fn detect(&mut self, _rep: Resource<ObjectDetector>, _f: Frame) -> Result<Vec<Detection>, String> {
        Ok(vec![])
    }
    fn drop(&mut self, rep: Resource<ObjectDetector>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep);
        Ok(())
    }
}
