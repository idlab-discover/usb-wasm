use crate::bindings::component::usb::device::{Host as HostDevice, HostUsbDevice, HostDeviceHandle, DeviceLocation};
use crate::bindings::component::usb::descriptors::{DeviceDescriptor, ConfigurationDescriptor};
use crate::bindings::component::usb::transfers::{
    Host as HostTransfers, HostTransfer,
    IsoPacket, IsoPacketStatus, TransferResult,
    TransferType, TransferSetup, TransferOptions,
};
use crate::bindings::component::usb::errors::LibusbError;
use crate::bindings::component::usb::configuration::ConfigValue;
use crate::bindings::component::usb::usb_hotplug::{Host as HostHotplug, Event, Info};
use crate::{UsbDevice, UsbDeviceHandle, UsbTransfer, MyState};
use wasmtime::component::Resource;



use std::time::Instant;
use tracing::info;

// --- Device ---

impl HostDevice for MyState {
    async fn init(&mut self) -> Result<(), LibusbError> {
        eprintln!("[WASI-USB-HOST] Initializing backend...");
        info!("WASI-USB Host: Backend will initialize on demand.");
        Ok(())
    }

    async fn list_devices(&mut self) -> Result<Vec<(Resource<UsbDevice>, DeviceDescriptor, DeviceLocation)>, LibusbError> {
        let devices = self.backend.list_devices(&self.allowed_usbdevices)?;
        eprintln!("[WASI-USB-HOST] list_devices: found {} devices", devices.len());
        let mut result = Vec::new();
        for (dev, desc, loc, name) in devices {
            let name_str = name.unwrap_or_else(|| "Unknown Device".to_string());
            eprintln!("  Device: {:04x}:{:04x} - {}", desc.vendor_id, desc.product_id, name_str);
            let res = self.table.push(dev).map_err(|_| LibusbError::Other)?;
            result.push((res, desc, loc));
        }
        Ok(result)
    }


}

impl HostUsbDevice for MyState {
    async fn open(&mut self, self_: Resource<UsbDevice>) -> Result<Resource<UsbDeviceHandle>, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        
        let handle = self.backend.open(device)?;
        let res = self.table.push(handle).map_err(|_| LibusbError::Other)?;
        Ok(res)
    }


    async fn get_configuration_descriptor(&mut self, self_: Resource<UsbDevice>, config_index: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration_descriptor(device, config_index)
    }

    async fn get_configuration_descriptor_by_value(&mut self, self_: Resource<UsbDevice>, config_value: u8) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration_descriptor_by_value(device, config_value)
    }

    async fn get_active_configuration_descriptor(&mut self, self_: Resource<UsbDevice>) -> Result<ConfigurationDescriptor, LibusbError> {
        let device = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_active_configuration_descriptor(device)
    }

    async fn drop(&mut self, rep: Resource<UsbDevice>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Device Handle ---

impl HostDeviceHandle for MyState {
    async fn get_configuration(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<u8, LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.get_configuration(h)
    }
    async fn set_configuration(&mut self, self_: Resource<UsbDeviceHandle>, config: ConfigValue) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.set_configuration(h, config)
    }
    async fn claim_interface(&mut self, self_: Resource<UsbDeviceHandle>, iface: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.claim_interface(h, iface)
    }
    async fn release_interface(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.release_interface(h, ifac)
    }
    async fn set_interface_altsetting(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8, alt: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.set_interface_alt_setting(h, ifac, alt)
    }
    async fn clear_halt(&mut self, self_: Resource<UsbDeviceHandle>, endpoint: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.clear_halt(h, endpoint)
    }
    async fn reset_device(&mut self, self_: Resource<UsbDeviceHandle>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.reset_device(h)
    }
    async fn alloc_streams(&mut self, self_: Resource<UsbDeviceHandle>, num: u32, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.alloc_streams(h, num, endpoints)
    }
    async fn free_streams(&mut self, self_: Resource<UsbDeviceHandle>, endpoints: Vec<u8>) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.free_streams(h, endpoints)
    }
    async fn kernel_driver_active(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<bool, LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.kernel_driver_active(h, ifac)
    }
    async fn detach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.detach_kernel_driver(h, ifac)
    }
    async fn attach_kernel_driver(&mut self, self_: Resource<UsbDeviceHandle>, ifac: u8) -> Result<(), LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        self.backend.attach_kernel_driver(h, ifac)
    }
    async fn new_transfer(
        &mut self,
        self_: Resource<UsbDeviceHandle>,
        xfer_type: TransferType,
        setup: TransferSetup,
        buf_size: u32,
        opts: TransferOptions,
    ) -> Result<Resource<UsbTransfer>, LibusbError> {
        let h = self.table.get(&self_).map_err(|_| LibusbError::NoDevice)?;
        let transfer = h.new_transfer(xfer_type, setup, buf_size, opts)?;
        let res = self.table.push(transfer).map_err(|_| LibusbError::Other)?;
        Ok(res)
    }
    async fn close(&mut self, self_: Resource<UsbDeviceHandle>) {
        if let Ok(handle) = self.table.delete(self_) {
            self.backend.close(handle);
        }
    }
    async fn drop(&mut self, rep: Resource<UsbDeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Unified Transfers ---

impl HostTransfers for MyState {
    async fn await_transfer(&mut self, xfer: Resource<UsbTransfer>) -> Result<TransferResult, LibusbError> {
        let tx = self.table.get::<UsbTransfer>(&xfer).map_err(|_| LibusbError::Io)?;

        // Spin until the transfer completion flag is set by the background event thread.
        while !tx.completed.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }

        let data = tx.buffer.clone();
        let mut packets = Vec::new();

        if let Some(results) = tx.iso_packet_results.lock().unwrap().clone() {
            for (actual_length, status_code) in results {
                let status = match status_code {
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

        Ok(TransferResult { data, packets })
    }

}

impl HostTransfer for MyState {
    async fn submit_transfer(&mut self, self_: Resource<UsbTransfer>, data: Vec<u8>) -> Result<(), LibusbError> {
        let xfer = self.table.get_mut::<UsbTransfer>(&self_).map_err(|_| LibusbError::NotFound)?;
        if !data.is_empty() {
            xfer.buffer = data;
        }
        xfer.submit()
    }
    async fn cancel_transfer(&mut self, self_: Resource<UsbTransfer>) -> Result<(), LibusbError> {
        let xfer = self.table.get::<UsbTransfer>(&self_).map_err(|_| LibusbError::NotFound)?;
        xfer.cancel()
    }
    async fn drop(&mut self, rep: Resource<UsbTransfer>) -> wasmtime::Result<()> {
        let _ = self.table.delete(rep);
        Ok(())
    }
}

// --- Hotplug ---

impl HostHotplug for MyState {
    // Leroy's WIT: enable-hotplug returns result<_, libusb-error> (no pollable).
    async fn enable_hotplug(&mut self) -> Result<(), LibusbError> {
        Err(LibusbError::NotSupported)
    }
    // Leroy's WIT: poll-events returns list<tuple<event, info, usb-device>>.
    async fn poll_events(&mut self) -> Vec<(Event, Info, Resource<UsbDevice>)> { Vec::new() }
}

impl crate::bindings::component::usb::errors::Host for MyState {}
impl crate::bindings::component::usb::configuration::Host for MyState {}
impl crate::bindings::component::usb::descriptors::Host for MyState {}
