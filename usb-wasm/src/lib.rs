wasmtime::component::bindgen!({
    world: "host",
    path: "usb.wit",

    with: {
        "component:usb/device/usb-device": UsbDevice,
        "component:usb/device/device-handle": DeviceHandle,
        "component:usb/transfers/transfer": Transfer,
    }
});

use error::UsbWasmError;
use rusb::{
    ffi::{libusb_alloc_transfer, libusb_handle_events_completed, libusb_submit_transfer},
    GlobalContext, Recipient, RequestType, UsbContext,
};
use std::{sync::Arc, time::Duration};
use component::usb;

use wasmtime_wasi::WasiView;

pub mod error;
mod host;

const TIMEOUT: Duration = Duration::from_secs(20);

pub fn map_error(e: UsbWasmError) -> usb::errors::LibusbError {
    match e {
        UsbWasmError::RusbError(rusb::Error::Io) => usb::errors::LibusbError::Io,
        UsbWasmError::RusbError(rusb::Error::InvalidParam) => usb::errors::LibusbError::InvalidParam,
        UsbWasmError::RusbError(rusb::Error::Access) => usb::errors::LibusbError::Access,
        UsbWasmError::RusbError(rusb::Error::NoDevice) => usb::errors::LibusbError::NoDevice,
        UsbWasmError::RusbError(rusb::Error::NotFound) => usb::errors::LibusbError::NotFound,
        UsbWasmError::RusbError(rusb::Error::Busy) => usb::errors::LibusbError::Busy,
        UsbWasmError::RusbError(rusb::Error::Timeout) => usb::errors::LibusbError::Timeout,
        UsbWasmError::RusbError(rusb::Error::Overflow) => usb::errors::LibusbError::Overflow,
        UsbWasmError::RusbError(rusb::Error::Pipe) => usb::errors::LibusbError::Pipe,
        UsbWasmError::RusbError(rusb::Error::Interrupted) => usb::errors::LibusbError::Interrupted,
        UsbWasmError::RusbError(rusb::Error::NoMem) => usb::errors::LibusbError::NoMem,
        UsbWasmError::RusbError(rusb::Error::NotSupported) => usb::errors::LibusbError::NotSupported,
        _ => usb::errors::LibusbError::Other,
    }
}

pub struct UsbDevice {
    device: rusb::Device<rusb::GlobalContext>,
}

pub struct DeviceHandle {
    handle: rusb::DeviceHandle<GlobalContext>,
    language: Option<rusb::Language>,
}

pub struct Transfer {
    // This will hold the raw transfer and any necessary state for async
    raw: *mut rusb::ffi::libusb_transfer,
    completed: Arc<std::sync::atomic::AtomicBool>,
}

unsafe impl Send for Transfer {}
unsafe impl Sync for Transfer {}

pub struct ControlSetup {
    pub request_type: RequestType,
    pub request_recipient: Recipient,
    pub request: u8,
    pub value: u16,
    pub index: u16,
}

fn error_from_libusb(err: i32) -> rusb::Error {
    match err {
        rusb::ffi::constants::LIBUSB_ERROR_IO => rusb::Error::Io,
        rusb::ffi::constants::LIBUSB_ERROR_INVALID_PARAM => rusb::Error::InvalidParam,
        rusb::ffi::constants::LIBUSB_ERROR_ACCESS => rusb::Error::Access,
        rusb::ffi::constants::LIBUSB_ERROR_NO_DEVICE => rusb::Error::NoDevice,
        rusb::ffi::constants::LIBUSB_ERROR_NOT_FOUND => rusb::Error::NotFound,
        rusb::ffi::constants::LIBUSB_ERROR_BUSY => rusb::Error::Busy,
        rusb::ffi::constants::LIBUSB_ERROR_TIMEOUT => rusb::Error::Timeout,
        rusb::ffi::constants::LIBUSB_ERROR_OVERFLOW => rusb::Error::Overflow,
        rusb::ffi::constants::LIBUSB_ERROR_PIPE => rusb::Error::Pipe,
        rusb::ffi::constants::LIBUSB_ERROR_INTERRUPTED => rusb::Error::Interrupted,
        rusb::ffi::constants::LIBUSB_ERROR_NO_MEM => rusb::Error::NoMem,
        rusb::ffi::constants::LIBUSB_ERROR_NOT_SUPPORTED => rusb::Error::NotSupported,
        rusb::ffi::constants::LIBUSB_ERROR_OTHER => rusb::Error::Other,
        _ => rusb::Error::Other,
    }
}

extern "system" fn libusb_transfer_cb(transfer: *mut rusb::ffi::libusb_transfer) {
    unsafe {
        *((*transfer).user_data as *mut i32) = 1;
    }
}

impl UsbDevice {
    pub fn enumerate() -> Result<Vec<(Self, usb::descriptors::DeviceDescriptor, usb::device::DeviceLocation)>, UsbWasmError> {
        let devices = rusb::devices()?;

        let mut devices_ = Vec::with_capacity(devices.len());

        for device in devices.iter() {
            let descriptor = device.device_descriptor()?;
            let usb_version = descriptor.usb_version();
            let device_version = descriptor.device_version();
            let speed = device.speed();

            let wit_descriptor = usb::descriptors::DeviceDescriptor {
                length: descriptor.num_configurations() * 0 + 18, // Simplified
                descriptor_type: 1,
                usb_version_bcd: ((usb_version.major() as u16) << 8) | (usb_version.minor() as u16),
                device_class: descriptor.class_code(),
                device_subclass: descriptor.sub_class_code(),
                device_protocol: descriptor.protocol_code(),
                max_packet_size0: descriptor.max_packet_size(),
                vendor_id: descriptor.vendor_id(),
                product_id: descriptor.product_id(),
                device_version_bcd: ((device_version.major() as u16) << 8) | (device_version.minor() as u16),
                manufacturer_index: descriptor.manufacturer_string_index().unwrap_or(0),
                product_index: descriptor.product_string_index().unwrap_or(0),
                serial_number_index: descriptor.serial_number_string_index().unwrap_or(0),
                num_configurations: descriptor.num_configurations(),
            };

            let wit_location = usb::device::DeviceLocation {
                bus_number: device.bus_number(),
                device_address: device.address(),
                port_number: device.port_number(),
                speed: match speed {
                    rusb::Speed::Low => usb::device::UsbSpeed::Low,
                    rusb::Speed::Full => usb::device::UsbSpeed::Full,
                    rusb::Speed::High => usb::device::UsbSpeed::High,
                    rusb::Speed::Super => usb::device::UsbSpeed::Super,
                    rusb::Speed::SuperPlus => usb::device::UsbSpeed::SuperPlus,
                    _ => usb::device::UsbSpeed::Unknown,
                },
            };

            devices_.push((
                UsbDevice { device },
                wit_descriptor,
                wit_location
            ));
        }

        Ok(devices_)
    }
}

impl UsbDevice {
    pub fn open(&self) -> Result<DeviceHandle, UsbWasmError> {
        let handle = self.device.open()?;
        let _ = handle.set_auto_detach_kernel_driver(true);
        let language = handle.read_languages(TIMEOUT).ok().and_then(|langs| langs.first().copied());
        Ok(DeviceHandle { handle, language })
    }

    pub fn get_configuration_descriptor(&self, index: u8) -> Result<usb::descriptors::ConfigurationDescriptor, UsbWasmError> {
        let config = self.device.config_descriptor(index)?;
        Ok(convert_config_descriptor(&config))
    }

    pub fn active_configuration_descriptor(&self) -> Result<usb::descriptors::ConfigurationDescriptor, UsbWasmError> {
        let config = self.device.active_config_descriptor()?;
        Ok(convert_config_descriptor(&config))
    }
}

fn convert_config_descriptor(config: &rusb::ConfigDescriptor) -> usb::descriptors::ConfigurationDescriptor {
    usb::descriptors::ConfigurationDescriptor {
        length: 9,
        descriptor_type: 2,
        total_length: config.total_length(),
        interfaces: config.interfaces().flat_map(|ifac| {
            ifac.descriptors().map(|alt| convert_interface_descriptor(&alt))
        }).collect(),
        configuration_value: config.number(),
        configuration_index: config.description_string_index().unwrap_or(0),
        attributes: 0, 
        max_power: config.max_power() as u8,
    }
}

fn convert_interface_descriptor(ifac: &rusb::InterfaceDescriptor) -> usb::descriptors::InterfaceDescriptor {
    usb::descriptors::InterfaceDescriptor {
        length: 9,
        descriptor_type: 4,
        interface_number: ifac.interface_number(),
        alternate_setting: ifac.setting_number(),
        endpoints: ifac.endpoint_descriptors().map(|ep| {
            usb::descriptors::EndpointDescriptor {
                length: 7,
                descriptor_type: 5,
                endpoint_address: ep.address(),
                attributes: ep.transfer_type() as u8,
                max_packet_size: ep.max_packet_size(),
                interval: ep.interval(),
                refresh: 0,
                synch_address: 0,
            }
        }).collect(),
        interface_class: ifac.class_code(),
        interface_subclass: ifac.sub_class_code(),
        interface_protocol: ifac.protocol_code(),
        interface_index: ifac.description_string_index().unwrap_or(0),
    }
}

impl DeviceHandle {
    pub fn close(&mut self) {
        // Drop closes
    }

    pub fn reset(&mut self) -> Result<(), UsbWasmError> {
        Ok(self.handle.reset()?)
    }

    pub fn set_configuration(&mut self, value: u8) -> Result<(), UsbWasmError> {
        Ok(self.handle.set_active_configuration(value)?)
    }

    pub fn claim_interface(&mut self, iface: u8) -> Result<(), UsbWasmError> {
        Ok(self.handle.claim_interface(iface)?)
    }

    pub fn release_interface(&mut self, iface: u8) -> Result<(), UsbWasmError> {
        Ok(self.handle.release_interface(iface)?)
    }

    pub fn set_interface_altsetting(&mut self, iface: u8, alt: u8) -> Result<(), UsbWasmError> {
        Ok(self.handle.set_alternate_setting(iface, alt)?)
    }

    pub fn clear_halt(&mut self, endpoint: u8) -> Result<(), UsbWasmError> {
        Ok(self.handle.clear_halt(endpoint)?)
    }

    pub fn get_configuration(&mut self) -> Result<u8, UsbWasmError> {
        Ok(self.handle.active_configuration()?)
    }
}

impl Transfer {
    pub fn new(
        handle: &mut DeviceHandle,
        xfer_type: usb::transfers::TransferType,
        setup: usb::transfers::TransferSetup,
        buf_size: u32,
        opts: &usb::transfers::TransferOptions,
    ) -> Result<Self, usb::errors::LibusbError> {
        let num_packets = opts.iso_packets;
        let mut raw = unsafe { libusb_alloc_transfer(num_packets as i32) };
        if raw.is_null() {
            return Err(usb::errors::LibusbError::NoMem);
        }

        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));

        unsafe {
            (*raw).dev_handle = handle.handle.as_raw();
            (*raw).endpoint = opts.endpoint;
            (*raw).transfer_type = match xfer_type {
                usb::transfers::TransferType::Control => 0,
                usb::transfers::TransferType::Isochronous => 1,
                usb::transfers::TransferType::Bulk => 2,
                usb::transfers::TransferType::Interrupt => 3,
            };
            (*raw).timeout = opts.timeout_ms;
            (*raw).callback = libusb_transfer_cb;
            (*raw).user_data = Arc::into_raw(completed.clone()) as *mut _;
            (*raw).num_iso_packets = num_packets as i32;
            
            // Set individual packet lengths for ISO
            if xfer_type == usb::transfers::TransferType::Isochronous && num_packets > 0 {
                let packet_len = buf_size / num_packets;
                let packets_ptr = (*raw).iso_packet_desc.as_mut_ptr();
                for i in 0..num_packets as usize {
                    (*packets_ptr.add(i)).length = packet_len as u32;
                }
            }
        }

        Ok(Transfer { raw: raw as *mut _, completed })
    }

    pub fn submit(&mut self, data: &[u8]) -> Result<(), usb::errors::LibusbError> {
        unsafe {
            let buf_ptr = if data.is_empty() {
                // For IN transfers, we need to allocate a buffer of the size we expect
                let size = (*self.raw).length as usize;
                if size == 0 {
                    // If length is 0, we might need to look at iso packets
                    let num_packets = (*self.raw).num_iso_packets as usize;
                    let packets_ptr = (*self.raw).iso_packet_desc.as_ptr();
                    let mut iso_size: u32 = 0;
                    for i in 0..num_packets {
                        iso_size += (*packets_ptr.add(i)).length;
                    }
                    
                    if iso_size > 0 {
                        let mut v = vec![0u8; iso_size as usize];
                        let ptr = v.as_mut_ptr();
                        std::mem::forget(v);
                        ptr
                    } else {
                        std::ptr::null_mut()
                    }
                } else {
                    let mut v = vec![0u8; size];
                    let ptr = v.as_mut_ptr();
                    std::mem::forget(v);
                    ptr
                }
            } else {
                let mut v = data.to_vec();
                let ptr = v.as_mut_ptr();
                (*self.raw).length = v.len() as i32;
                std::mem::forget(v);
                ptr
            };

            (*self.raw).buffer = buf_ptr;
            
            let res = libusb_submit_transfer(self.raw);
            if res != 0 {
                return Err(match error_from_libusb(res) {
                    rusb::Error::Io => usb::errors::LibusbError::Io,
                    rusb::Error::InvalidParam => usb::errors::LibusbError::InvalidParam,
                    rusb::Error::Access => usb::errors::LibusbError::Access,
                    rusb::Error::NoDevice => usb::errors::LibusbError::NoDevice,
                    rusb::Error::NotFound => usb::errors::LibusbError::NotFound,
                    rusb::Error::Busy => usb::errors::LibusbError::Busy,
                    rusb::Error::Timeout => usb::errors::LibusbError::Timeout,
                    rusb::Error::Overflow => usb::errors::LibusbError::Overflow,
                    rusb::Error::Pipe => usb::errors::LibusbError::Pipe,
                    rusb::Error::Interrupted => usb::errors::LibusbError::Interrupted,
                    rusb::Error::NoMem => usb::errors::LibusbError::NoMem,
                    rusb::Error::NotSupported => usb::errors::LibusbError::NotSupported,
                    _ => usb::errors::LibusbError::Other,
                });
            }
        }
        Ok(())
    }

    pub fn cancel(&mut self) -> Result<(), usb::errors::LibusbError> {
        unsafe {
            let res = rusb::ffi::libusb_cancel_transfer(self.raw);
            if res != 0 {
                return Err(match error_from_libusb(res) {
                    rusb::Error::Io => usb::errors::LibusbError::Io,
                    rusb::Error::InvalidParam => usb::errors::LibusbError::InvalidParam,
                    rusb::Error::Access => usb::errors::LibusbError::Access,
                    rusb::Error::NoDevice => usb::errors::LibusbError::NoDevice,
                    rusb::Error::NotFound => usb::errors::LibusbError::NotFound,
                    rusb::Error::Busy => usb::errors::LibusbError::Busy,
                    rusb::Error::Timeout => usb::errors::LibusbError::Timeout,
                    rusb::Error::Overflow => usb::errors::LibusbError::Overflow,
                    rusb::Error::Pipe => usb::errors::LibusbError::Pipe,
                    rusb::Error::Interrupted => usb::errors::LibusbError::Interrupted,
                    rusb::Error::NoMem => usb::errors::LibusbError::NoMem,
                    rusb::Error::NotSupported => usb::errors::LibusbError::NotSupported,
                    _ => usb::errors::LibusbError::Other,
                });
            }
        }
        Ok(())
    }

    pub fn await_completion(&mut self) -> Result<Vec<u8>, usb::errors::LibusbError> {
        while !self.completed.load(std::sync::atomic::Ordering::SeqCst) {
            unsafe {
                libusb_handle_events_completed(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }
        
        unsafe {
            let len = (*self.raw).actual_length as usize;
            let data = Vec::from_raw_parts((*self.raw).buffer, len, (*self.raw).length as usize);
            Ok(data)
        }
    }

    pub fn await_iso_completion(&mut self) -> Result<usb::transfers::IsoResult, usb::errors::LibusbError> {
        while !self.completed.load(std::sync::atomic::Ordering::SeqCst) {
            unsafe {
                libusb_handle_events_completed(std::ptr::null_mut(), std::ptr::null_mut());
            }
        }

        unsafe {
            let num_packets = (*self.raw).num_iso_packets as usize;
            let packets_ptr = (*self.raw).iso_packet_desc.as_ptr();
            
            let mut packets = Vec::with_capacity(num_packets);
            for i in 0..num_packets {
                let p = &*packets_ptr.add(i);
                packets.push(usb::transfers::IsoPacket {
                    actual_length: p.actual_length,
                    status: match p.status {
                        0 => usb::transfers::IsoPacketStatus::Success,
                        1 => usb::transfers::IsoPacketStatus::Error,
                        2 => usb::transfers::IsoPacketStatus::TimedOut,
                        3 => usb::transfers::IsoPacketStatus::Cancelled,
                        4 => usb::transfers::IsoPacketStatus::Stall,
                        5 => usb::transfers::IsoPacketStatus::NoDevice,
                        6 => usb::transfers::IsoPacketStatus::Overflow,
                        _ => usb::transfers::IsoPacketStatus::Error,
                    },
                });
            }

            let first_packet_len = (*packets_ptr).length as usize;
            let total_size = num_packets * first_packet_len;
            let data = Vec::from_raw_parts((*self.raw).buffer, total_size, total_size);
            
            Ok(usb::transfers::IsoResult { data, packets })
        }
    }
}

impl Drop for Transfer {
    fn drop(&mut self) {
        unsafe {
            if !self.raw.is_null() {
                // If buffer was allocated, we should free it.
                // But in this simple implementation, we assume await_completion has taken ownership back via Vec::from_raw_parts.
                // If it wasn't awaited, we have a leak. 
                // For a proper implementation, we'd need to track buffer ownership more carefully.
                rusb::ffi::libusb_free_transfer(self.raw);
            }
        }
    }
}

pub fn add_to_linker<T: WasiView>(
    linker: &mut wasmtime::component::Linker<T>,
) -> wasmtime::Result<()> {
    component::usb::device::add_to_linker(linker, |s| s)?;
    component::usb::transfers::add_to_linker(linker, |s| s)
}

