wasmtime::component::bindgen!({
    world: "wadu436:usb/imports",
    path: "../wit/deps/usb",

    with: {
        "wadu436:usb/device/usb-device": UsbDevice,
        "wadu436:usb/device/usb-configuration": UsbConfiguration,
        "wadu436:usb/device/usb-interface": UsbInterface,
        "wadu436:usb/device/usb-endpoint": UsbEndpoint,
    }
});

use error::UsbWasmError;
use rusb::{
    constants::LIBUSB_TRANSFER_TYPE_ISOCHRONOUS,
    ffi::{libusb_alloc_transfer, libusb_handle_events_completed, libusb_submit_transfer},
    GlobalContext, Recipient, RequestType, Speed, UsbContext,
};
use std::{error::Error, sync::Arc, time::Duration};
use wadu436::usb::{self, types::Direction};

use wasmtime_wasi::WasiView;

mod error;
mod host;

const TIMEOUT: Duration = Duration::from_secs(1);

pub struct UsbDevice {
    device: rusb::Device<rusb::GlobalContext>,
    handle: Option<rusb::DeviceHandle<GlobalContext>>,
    language: rusb::Language,
    descriptor: usb::device::DeviceDescriptor,
}

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
    pub fn enumerate() -> Result<Vec<Self>, UsbWasmError> {
        let devices = rusb::devices()?;

        let mut devices_ = Vec::with_capacity(devices.len());

        for device in devices.iter() {
            let handle = device.open()?;

            // First get all the information needed to apply the filters
            let descriptor = device.device_descriptor()?;
            let language = handle.read_languages(TIMEOUT)?[0];

            let product_name = handle
                .read_product_string(language, &descriptor, TIMEOUT)
                .ok();
            let manufacturer_name = handle
                .read_manufacturer_string(language, &descriptor, TIMEOUT)
                .ok();
            let serial_number = handle
                .read_serial_number_string(language, &descriptor, TIMEOUT)
                .ok();

            let device_version = descriptor.device_version();
            let usb_version = descriptor.usb_version();

            let descriptor = usb::device::DeviceDescriptor {
                vendor_id: descriptor.vendor_id(),
                product_id: descriptor.product_id(),
                device_class: descriptor.class_code(),
                device_subclass: descriptor.sub_class_code(),
                device_protocol: descriptor.protocol_code(),
                manufacturer_name,
                product_name,
                serial_number,
                device_version: (
                    device_version.major(),
                    device_version.minor(),
                    device_version.sub_minor(),
                ),
                usb_version: (
                    usb_version.major(),
                    usb_version.minor(),
                    usb_version.sub_minor(),
                ),
                max_packet_size: descriptor.max_packet_size(),
            };

            devices_.push(UsbDevice {
                device,
                handle: None,
                language,
                descriptor,
            });
        }

        Ok(devices_)
    }

    pub fn open(&mut self) -> Result<(), UsbWasmError> {
        self.handle = Some(self.device.open()?);
        Ok(())
    }

    pub fn active_configuration(&mut self) -> Result<UsbConfiguration, UsbWasmError> {
        let configuration = self.device.active_config_descriptor()?;
        let description = self
            .device
            .open()
            .unwrap()
            .read_configuration_string(self.language, &configuration, TIMEOUT)
            .ok();

        // Find the index of this configuration
        let config_index = (0..self.device.device_descriptor()?.num_configurations())
            .find(|i| {
                self.device
                    .config_descriptor(*i)
                    .ok()
                    .map_or(false, |descriptor| {
                        descriptor.number() == configuration.number()
                    })
            })
            .unwrap();

        Ok(UsbConfiguration {
            language: self.language,
            index: config_index,
            descriptor: usb::device::ConfigurationDescriptor {
                number: configuration.number(),
                description,
                self_powered: configuration.self_powered(),
                remote_wakeup: configuration.remote_wakeup(),
                max_power: configuration.max_power(),
            },
            device: self.device.clone(),
        })
    }

    pub fn speed(&mut self) -> Speed {
        self.device.speed()
    }

    pub fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            std::mem::drop(handle);
        }
    }

    pub fn reset(&mut self) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            Ok(handle.reset()?)
        } else {
            Err(rusb::Error::InvalidParam.into())
        }
    }

    pub fn opened(&self) -> bool {
        self.handle.is_some()
    }

    pub fn clear_halt(&mut self, endpoint: u8) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            handle.clear_halt(endpoint)?;
        }
        Ok(())
    }

    pub fn select_configuration(&mut self, config: u8) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            if handle.kernel_driver_active(0)? {
                handle.detach_kernel_driver(0).unwrap();
            }
            handle.set_active_configuration(config).unwrap();
        }
        Ok(())
    }

    pub fn claim_interface(&mut self, interface: u8) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            handle.claim_interface(interface)?;
        }
        Ok(())
    }

    pub fn release_interface(&mut self, interface: u8) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            handle.release_interface(interface)?;
        }
        Ok(())
    }

    pub fn set_alternate_setting(
        &mut self,
        interface: u8,
        setting: u8,
    ) -> Result<(), UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            handle.set_alternate_setting(interface, setting)?;
        }
        Ok(())
    }

    pub fn interrupt_transfer_in(
        &mut self,
        endpoint: u8,
        buffer_size: usize,
    ) -> Result<Vec<u8>, UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            let mut buffer = vec![0; buffer_size];
            let _bytes_read = handle
                .read_interrupt(endpoint, &mut buffer, TIMEOUT)
                .unwrap();
            buffer.resize(_bytes_read, 0);
            Ok(buffer)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn interrupt_transfer_out(
        &mut self,
        endpoint: u8,
        buffer: &[u8],
    ) -> Result<usize, UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            let bytes_written = handle.write_interrupt(endpoint, buffer, TIMEOUT).unwrap();
            Ok(bytes_written)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn bulk_transfer_in(
        &mut self,
        endpoint: u8,
        buffer_size: usize,
    ) -> Result<Vec<u8>, UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            let mut buffer = vec![0; buffer_size];
            let _bytes_read = handle.read_bulk(endpoint, &mut buffer, TIMEOUT).unwrap();
            buffer.resize(_bytes_read, 0);
            Ok(buffer)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn bulk_transfer_out(
        &mut self,
        endpoint: u8,
        buffer: &[u8],
    ) -> Result<usize, UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            let bytes_written = handle.write_bulk(endpoint, buffer, TIMEOUT).unwrap();
            Ok(bytes_written)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn iso_transfer_in(
        &mut self,
        endpoint: u8,
        num_packets: i32,
        buffer_size: usize,
    ) -> Result<Vec<Vec<u8>>, UsbWasmError> {
        if num_packets < 0 {
            // Error
            return Err(rusb::Error::InvalidParam.into());
        }
        if let Some(handle) = &mut self.handle {
            let transfer = unsafe { libusb_alloc_transfer(num_packets) };
            let transfer_ref = unsafe { &mut *transfer };

            let mut completed = 0_i32;
            let completed_ptr = (&mut completed) as *mut i32;

            let mut buffer = vec![0; buffer_size * num_packets as usize];

            transfer_ref.dev_handle = handle.as_raw();
            transfer_ref.endpoint = endpoint;
            transfer_ref.transfer_type = LIBUSB_TRANSFER_TYPE_ISOCHRONOUS;
            transfer_ref.timeout = 1000;
            transfer_ref.buffer = buffer.as_mut_slice().as_ptr() as *mut _;
            transfer_ref.length = buffer.len() as _;
            transfer_ref.num_iso_packets = num_packets;
            transfer_ref.user_data = completed_ptr as *mut _;
            for i in 0..num_packets as usize {
                let entry = unsafe { (*transfer).iso_packet_desc.get_unchecked_mut(i) };
                entry.length = buffer_size as _;
                entry.status = 0;
                entry.actual_length = 0;
            }

            transfer_ref.callback = libusb_transfer_cb;

            let err = unsafe { libusb_submit_transfer(transfer) };
            if err != 0 {
                return Err(error_from_libusb(err).into());
            }

            let mut err = 0;
            unsafe {
                while (*completed_ptr) == 0 {
                    err = libusb_handle_events_completed(handle.context().as_raw(), completed_ptr);
                }
            };
            if err != 0 {
                return Err(error_from_libusb(err).into());
            }

            let mut output_data = Vec::with_capacity(num_packets as usize);
            for i in 0..num_packets as usize {
                let entry = unsafe { (*transfer).iso_packet_desc.get_unchecked_mut(0) };
                if entry.status == 0 {
                    output_data.push(
                        buffer[i * buffer_size..i * buffer_size + entry.actual_length as usize]
                            .to_vec(),
                    );
                } else {
                    // TODO: handle errors here
                    // Status code meanings
                    // https://libusb.sourceforge.io/api-1.0/group__libusb__asyncio.html#ga9fcb2aa23d342060ebda1d0cf7478856
                }
            }

            Ok(output_data)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn iso_transfer_out(
        &mut self,
        endpoint: u8,
        buffers: &[Vec<u8>],
    ) -> Result<u64, UsbWasmError> {
        if let Some(handle) = &mut self.handle {
            let transfer = unsafe { libusb_alloc_transfer(1) };
            let transfer_ref = unsafe { &mut *transfer };

            let mut completed = 0_i32;
            let completed_ptr = (&mut completed) as *mut i32;

            // reorder the buffers so they're continuous in memory
            let buffer: Vec<u8> = buffers.iter().flatten().copied().collect::<Vec<u8>>();

            transfer_ref.dev_handle = handle.as_raw();
            transfer_ref.endpoint = endpoint;
            transfer_ref.transfer_type = LIBUSB_TRANSFER_TYPE_ISOCHRONOUS;
            transfer_ref.timeout = 1000;
            transfer_ref.buffer = buffer.as_ptr() as *mut _;
            transfer_ref.length = buffer.len() as _;
            transfer_ref.num_iso_packets = 1;
            // It should be okay to pass in this (stack) variable, as this function will not return untill after the transfer is complete.
            transfer_ref.user_data = completed_ptr as *mut _;

            for (i, buffer) in buffers.iter().enumerate() {
                let entry = unsafe { (*transfer).iso_packet_desc.get_unchecked_mut(i) };
                entry.length = buffer.len() as _;
                entry.status = 0;
                entry.actual_length = 0;
            }

            transfer_ref.callback = libusb_transfer_cb;

            let err = unsafe { libusb_submit_transfer(transfer) };
            if err != 0 {
                return Err(error_from_libusb(err).into());
            }

            let mut err = 0;
            unsafe {
                while (*completed_ptr) == 0 {
                    err = libusb_handle_events_completed(handle.context().as_raw(), completed_ptr);
                }
            };
            if err != 0 {
                return Err(error_from_libusb(err).into());
            }

            let mut bytes_written: u64 = 0;
            for i in 0..buffers.len() {
                let entry = unsafe { (*transfer).iso_packet_desc.get_unchecked_mut(i) };
                if entry.status == 0 {
                    bytes_written += entry.actual_length as u64;
                } else {
                    // TODO: handle errors here
                    // Status code meanings
                    // https://libusb.sourceforge.io/api-1.0/group__libusb__asyncio.html#ga9fcb2aa23d342060ebda1d0cf7478856
                }
            }

            Ok(bytes_written)
        } else {
            // TODO: fix a proper error here
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn control_transfer_in(
        &mut self,
        setup: ControlSetup,
        length: u16,
    ) -> Result<Vec<u8>, UsbWasmError> {
        if let Some(handle) = &self.handle {
            let request_type = rusb::request_type(
                rusb::Direction::In,
                setup.request_type,
                setup.request_recipient,
            );
            // Low speed: 8 bytes
            // High and Full speed: 64 bytes
            // Super speed: 512 bytes
            // Not sure for super speed plus?
            // let mut buffer = match self.device.speed() {
            //     rusb::Speed::Low => vec![0; 8],
            //     rusb::Speed::High | rusb::Speed::Full => vec![0; 64],
            //     rusb::Speed::Super | rusb::Speed::SuperPlus | rusb::Speed::Unknown => {
            //         vec![0; 512]
            //     } // Assume highest buffer needed for unknown speed
            //     _ => {
            //         vec![0; 512]
            //     } // Assume highest buffer needed for non-exhaustive checks
            // };
            let mut buffer = vec![0; length as usize];
            let bytes_read = handle.read_control(
                request_type,
                setup.request,
                setup.value,
                setup.index,
                &mut buffer,
                TIMEOUT,
            )?;
            buffer.truncate(bytes_read);
            Ok(buffer)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn control_transfer_out(
        &mut self,
        setup: ControlSetup,
        data: &[u8],
    ) -> Result<u64, UsbWasmError> {
        if let Some(handle) = &self.handle {
            let request_type = rusb::request_type(
                rusb::Direction::Out,
                setup.request_type,
                setup.request_recipient,
            );
            let bytes_written = handle.write_control(
                request_type,
                setup.request,
                setup.value,
                setup.index,
                data,
                TIMEOUT,
            )?;
            Ok(bytes_written as u64)
        } else {
            Err(UsbWasmError::DeviceNotOpened)
        }
    }

    pub fn get_configurations(&self) -> Vec<UsbConfiguration> {
        let mut configurations = Vec::new();

        for i in 0..self
            .device
            .device_descriptor()
            .unwrap()
            .num_configurations()
        {
            let configuration = self.device.config_descriptor(i).unwrap();

            let description = self
                .device
                .open()
                .unwrap()
                .read_configuration_string(self.language, &configuration, TIMEOUT)
                .ok();
            configurations.push(UsbConfiguration {
                language: self.language,
                index: i,
                descriptor: usb::device::ConfigurationDescriptor {
                    number: configuration.number(),
                    description,
                    self_powered: configuration.self_powered(),
                    remote_wakeup: configuration.remote_wakeup(),
                    max_power: configuration.max_power(),
                },
                device: self.device.clone(),
            });
        }

        configurations
    }
}

pub struct UsbConfiguration {
    device: rusb::Device<rusb::GlobalContext>,
    index: u8,
    language: rusb::Language,
    descriptor: usb::device::ConfigurationDescriptor,
}

impl UsbConfiguration {
    pub fn get_interfaces(&self) -> Vec<UsbInterface> {
        let handle = self.device.open().unwrap();
        let config_descriptor = Arc::new(self.device.config_descriptor(self.index).unwrap());
        config_descriptor
            .interfaces()
            .flat_map(|interface| {
                interface.descriptors().map(|interface_descriptor| {
                    let interface_name = handle
                        .read_interface_string(self.language, &interface_descriptor, TIMEOUT)
                        .ok();

                    UsbInterface {
                        config_descriptor: config_descriptor.clone(),
                        descriptor: usb::device::InterfaceDescriptor {
                            interface_number: interface_descriptor.interface_number(),
                            alternate_setting: interface_descriptor.setting_number(),
                            interface_class: interface_descriptor.class_code(),
                            interface_subclass: interface_descriptor.sub_class_code(),
                            interface_protocol: interface_descriptor.protocol_code(),
                            interface_name,
                        },
                    }
                })
            })
            .collect()
    }
}

pub struct UsbInterface {
    descriptor: usb::device::InterfaceDescriptor,
    config_descriptor: Arc<rusb::ConfigDescriptor>,
}

impl UsbInterface {
    pub fn get_endpoints(&self) -> Vec<UsbEndpoint> {
        let interface_descriptor = self
            .config_descriptor
            .interfaces()
            .find(|interface| interface.number() == self.descriptor.interface_number)
            .unwrap()
            .descriptors()
            .find(|descriptor| descriptor.setting_number() == self.descriptor.alternate_setting)
            .unwrap();
        interface_descriptor
            .endpoint_descriptors()
            .map(|endpoint| UsbEndpoint {
                descriptor: usb::device::EndpointDescriptor {
                    endpoint_number: endpoint.number(),
                    direction: match endpoint.direction() {
                        rusb::Direction::In => usb::types::Direction::In,
                        rusb::Direction::Out => usb::types::Direction::Out,
                    },
                    transfer_type: match endpoint.transfer_type() {
                        rusb::TransferType::Control => usb::types::TransferType::Control,
                        rusb::TransferType::Isochronous => usb::types::TransferType::Isochronous,
                        rusb::TransferType::Bulk => usb::types::TransferType::Bulk,
                        rusb::TransferType::Interrupt => usb::types::TransferType::Interrupt,
                    },
                    max_packet_size: endpoint.max_packet_size(),
                    interval: endpoint.interval(),
                },
            })
            .collect()
    }
}

pub struct UsbEndpoint {
    descriptor: usb::device::EndpointDescriptor,
}

impl UsbEndpoint {
    pub fn get_endpoint_number(&self) -> u8 {
        self.descriptor.endpoint_number
            + match self.descriptor.direction {
                Direction::Out => 0x00,
                Direction::In => 0x80,
            }
    }
}

#[derive(Debug)]
pub enum UsbError {
    NotFound,
    AlreadyDropped,
}

impl std::fmt::Display for UsbError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            UsbError::NotFound => write!(f, "Not found"),
            UsbError::AlreadyDropped => write!(f, "Already dropped"),
        }
    }
}

impl Error for UsbError {}

pub fn add_to_linker<T: WasiView>(
    linker: &mut wasmtime::component::Linker<T>,
) -> wasmtime::Result<()> {
    wadu436::usb::device::add_to_linker(linker, |s| s)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_enumeration() -> anyhow::Result<()> {
        let devices = UsbDevice::enumerate()?;

        for device in devices {
            for configuration in device.get_configurations() {
                for interface in configuration.get_interfaces() {
                    for endpoint in interface.get_endpoints() {
                        println!("{:?}", endpoint.descriptor);
                    }
                }
            }
        }

        Ok(())
    }
}
