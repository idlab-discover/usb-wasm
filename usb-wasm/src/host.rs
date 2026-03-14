use wasmtime_wasi::WasiView;
use crate::component::usb::descriptors::ConfigurationDescriptor;
use crate::component::usb::descriptors::DeviceDescriptor;
use crate::component::usb::device::DeviceLocation;

// Traits
use crate::component::usb::device::HostUsbDevice;
use crate::component::usb::device::HostDeviceHandle;
use crate::component::usb::transfers::HostTransfer;

// Types from interfaces (mapped ones)
// We use the local types for the resources in the table, but the bindgen-generated 
// resource handles for the function signatures.
// UsbDevice -> crate::UsbDevice
// DeviceHandle -> crate::DeviceHandle
// Transfer -> crate::Transfer

impl<T: WasiView> HostUsbDevice for T {
    fn open(&mut self, rep: wasmtime::component::Resource<crate::UsbDevice>) -> wasmtime::Result<Result<wasmtime::component::Resource<crate::DeviceHandle>, crate::component::usb::errors::LibusbError>> {
        let device = self.table().get(&rep)?;
        match device.open() {
            Ok(handle) => Ok(Ok(self.table().push(handle)?)),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn get_configuration_descriptor(&mut self, rep: wasmtime::component::Resource<crate::UsbDevice>, index: u8) -> wasmtime::Result<Result<ConfigurationDescriptor, crate::component::usb::errors::LibusbError>> {
        let device = self.table().get(&rep)?;
        match device.get_configuration_descriptor(index) {
            Ok(desc) => Ok(Ok(desc)),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn get_configuration_descriptor_by_value(&mut self, _rep: wasmtime::component::Resource<crate::UsbDevice>, _value: u8) -> wasmtime::Result<Result<ConfigurationDescriptor, crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn get_active_configuration_descriptor(&mut self, rep: wasmtime::component::Resource<crate::UsbDevice>) -> wasmtime::Result<Result<ConfigurationDescriptor, crate::component::usb::errors::LibusbError>> {
        let device = self.table().get(&rep)?;
        match device.active_configuration_descriptor() {
            Ok(desc) => Ok(Ok(desc)),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<crate::UsbDevice>) -> wasmtime::Result<()> {
        self.table().delete(rep)?;
        Ok(())
    }
}

impl<T: WasiView> HostDeviceHandle for T {
    fn get_configuration(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>) -> wasmtime::Result<Result<u8, crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match handle.get_configuration() {
            Ok(v) => Ok(Ok(v)),
            Err(e) => Ok(Err(crate::map_error(e))),
        }
    }

    fn set_configuration(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, config: crate::component::usb::configuration::ConfigValue) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        let val = match config {
            crate::component::usb::configuration::ConfigValue::Unconfigured => 0,
            crate::component::usb::configuration::ConfigValue::Value(v) => v,
        };
        match handle.set_configuration(val) {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn claim_interface(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, ifac: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match handle.claim_interface(ifac) {
            Ok(()) => Ok(Ok(())),
            Err(e) => {
                println!("[HOST] claim_interface({}) failed: {:?}", ifac, e);
                Ok(Err(crate::map_error(e)))
            },
        }
    }

    fn release_interface(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, ifac: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match handle.release_interface(ifac) {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn set_interface_altsetting(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, iface: u8, alt: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        println!("[HOST] set_interface_altsetting(iface={}, alt={})", iface, alt);
        match handle.set_interface_altsetting(iface, alt) {
            Ok(()) => Ok(Ok(())),
            Err(e) => {
                println!("[HOST] set_interface_altsetting failed: {:?}", e);
                Ok(Err(crate::map_error(e)))
            }
        }
    }

    fn clear_halt(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, endpoint: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match handle.clear_halt(endpoint) {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(crate::map_error(e))),
        }
    }

    fn reset_device(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match handle.reset() {
            Ok(()) => Ok(Ok(())),
            Err(_) => Ok(Err(crate::component::usb::errors::LibusbError::Other)),
        }
    }

    fn alloc_streams(&mut self, _rep: wasmtime::component::Resource<crate::DeviceHandle>, _num_streams: u32, _endpoints: Vec<u8>) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn free_streams(&mut self, _rep: wasmtime::component::Resource<crate::DeviceHandle>, _endpoints: Vec<u8>) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn kernel_driver_active(&mut self, _rep: wasmtime::component::Resource<crate::DeviceHandle>, _ifac: u8) -> wasmtime::Result<Result<bool, crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn detach_kernel_driver(&mut self, _rep: wasmtime::component::Resource<crate::DeviceHandle>, _ifac: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn attach_kernel_driver(&mut self, _rep: wasmtime::component::Resource<crate::DeviceHandle>, _ifac: u8) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        Ok(Err(crate::component::usb::errors::LibusbError::NotSupported))
    }

    fn new_transfer(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>, xfer_type: crate::component::usb::transfers::TransferType, setup: crate::component::usb::transfers::TransferSetup, buf_size: u32, opts: crate::component::usb::transfers::TransferOptions) -> wasmtime::Result<Result<wasmtime::component::Resource<crate::Transfer>, crate::component::usb::errors::LibusbError>> {
        let handle = self.table().get_mut(&rep)?;
        match crate::Transfer::new(handle, xfer_type, setup, buf_size, &opts) {
            Ok(xfer) => Ok(Ok(self.table().push(xfer)?)),
            Err(e) => Ok(Err(e)),
        }
    }

    fn close(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep)?;
        Ok(())
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<crate::DeviceHandle>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep)?;
        Ok(())
    }
}

impl<T: WasiView> HostTransfer for T {
    fn submit_transfer(&mut self, rep: wasmtime::component::Resource<crate::Transfer>, data: Vec<u8>) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let xfer = self.table().get_mut(&rep)?;
        match xfer.submit(&data) {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(e)),
        }
    }

    fn cancel_transfer(&mut self, rep: wasmtime::component::Resource<crate::Transfer>) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        let xfer = self.table().get_mut(&rep)?;
        match xfer.cancel() {
            Ok(()) => Ok(Ok(())),
            Err(e) => Ok(Err(e)),
        }
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<crate::Transfer>) -> wasmtime::Result<()> {
        let _ = self.table().delete(rep)?;
        Ok(())
    }
}

impl<T: WasiView> crate::component::usb::transfers::Host for T {
    fn await_transfer(&mut self, xfer: wasmtime::component::Resource<crate::Transfer>) -> wasmtime::Result<Result<Vec<u8>, crate::component::usb::errors::LibusbError>> {
        let mut xfer = self.table().delete(xfer)?;
        match xfer.await_completion() {
            Ok(data) => Ok(Ok(data)),
            Err(e) => Ok(Err(e)),
        }
    }

    fn await_iso_transfer(&mut self, xfer: wasmtime::component::Resource<crate::Transfer>) -> wasmtime::Result<Result<crate::component::usb::transfers::IsoResult, crate::component::usb::errors::LibusbError>> {
        let mut xfer = self.table().delete(xfer)?;
        match xfer.await_iso_completion() {
            Ok(res) => Ok(Ok(res)),
            Err(e) => Ok(Err(e)),
        }
    }
}

impl<T: WasiView> crate::component::usb::device::Host for T {
    fn init(&mut self) -> wasmtime::Result<Result<(), crate::component::usb::errors::LibusbError>> {
        Ok(Ok(()))
    }

    fn list_devices(&mut self) -> wasmtime::Result<Result<Vec<(wasmtime::component::Resource<crate::UsbDevice>, DeviceDescriptor, DeviceLocation)>, crate::component::usb::errors::LibusbError>> {
        let table = self.table();
        println!("[HOST] listing devices...");
        match crate::UsbDevice::enumerate() {
            Ok(devices) => {
                println!("[HOST] found {} devices", devices.len());
                let mut result = Vec::new();
                for (dev, desc, loc) in devices {
                    println!("[HOST] device: {:04x}:{:04x}", desc.vendor_id, desc.product_id);
                    let res = table.push(dev)?;
                    result.push((res, desc, loc));
                }
                Ok(Ok(result))
            }
            Err(e) => {
                println!("[HOST] error listing devices: {:?}", e);
                Ok(Err(crate::component::usb::errors::LibusbError::Other))
            }
        }
    }
}
