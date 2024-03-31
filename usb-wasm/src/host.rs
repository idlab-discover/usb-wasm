use wasmtime_wasi::WasiView;

use crate::wadu436::usb::device::*;
use crate::wadu436::usb::types::{ControlSetupRecipient, ControlSetupType};

fn host_control_setup_to_rusb(
    setup: &crate::wadu436::usb::device::ControlSetup,
) -> crate::ControlSetup {
    let request_type = match setup.request_type {
        ControlSetupType::Standard => rusb::RequestType::Standard,
        ControlSetupType::Class => rusb::RequestType::Class,
        ControlSetupType::Vendor => rusb::RequestType::Vendor,
    };
    let request_recipient = match setup.request_recipient {
        ControlSetupRecipient::Device => rusb::Recipient::Device,
        ControlSetupRecipient::Interface => rusb::Recipient::Interface,
        ControlSetupRecipient::Endpoint => rusb::Recipient::Endpoint,
    };

    crate::ControlSetup {
        request_type,
        request_recipient,
        request: setup.request,
        value: setup.value,
        index: setup.index,
    }
}

impl<T: WasiView> HostUsbDevice for T {
    fn enumerate(&mut self) -> wasmtime::Result<Vec<wasmtime::component::Resource<UsbDevice>>> {
        let table = self.table();

        Ok(UsbDevice::enumerate()?
            .into_iter()
            .map(|device| table.push(device))
            .collect::<Result<_, _>>()?)
    }

    #[doc = "Convenience funtion, equivalent to calling enumerate(), applying the provided filters to the list, and returning the first element"]
    fn request_device(
        &mut self,
        filter: Filter,
    ) -> wasmtime::Result<Option<wasmtime::component::Resource<UsbDevice>>> {
        let table = self.table();
        let device = UsbDevice::enumerate()?.into_iter().find(|device| {
            let descriptor = &device.descriptor;
            let vendor_id = filter
                .vendor_id
                .map_or(true, |vendor_id| vendor_id == descriptor.vendor_id);
            let product_id = filter
                .product_id
                .map_or(true, |product_id| product_id == descriptor.product_id);
            let class_code = filter
                .class_code
                .map_or(true, |class_code| class_code == descriptor.device_class);
            let subclass_code = filter.subclass_code.map_or(true, |subclass_code| {
                subclass_code == descriptor.device_subclass
            });
            let protocol_code = filter.protocol_code.map_or(true, |protocol_code| {
                protocol_code == descriptor.device_protocol
            });
            let serial_number = filter.serial_number == descriptor.serial_number;

            vendor_id && product_id && class_code && subclass_code && protocol_code && serial_number
        });

        Ok(device.map(|device| table.push(device)).transpose()?)
    }

    fn descriptor(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
    ) -> wasmtime::Result<DeviceDescriptor> {
        let device = self.table().get(&rep)?;
        Ok(device.descriptor.clone())
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<UsbDevice>) -> wasmtime::Result<()> {
        self.table().delete(rep)?;
        Ok(())
    }

    fn configurations(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
    ) -> wasmtime::Result<Vec<wasmtime::component::Resource<UsbConfiguration>>> {
        let table = self.table();
        let device = table.get(&rep)?;

        Ok(device
            .get_configurations()
            .into_iter()
            .map(|configuration| table.push_child(configuration, &rep))
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn open(&mut self, rep: wasmtime::component::Resource<UsbDevice>) -> wasmtime::Result<()> {
        let device = self.table().get_mut(&rep)?;
        device.open()?;
        Ok(())
    }

    fn reset(&mut self, rep: wasmtime::component::Resource<UsbDevice>) -> wasmtime::Result<()> {
        let device = self.table().get_mut(&rep)?;
        device.reset()?;
        Ok(())
    }

    fn close(&mut self, rep: wasmtime::component::Resource<UsbDevice>) -> wasmtime::Result<()> {
        let device = self.table().get_mut(&rep)?;
        device.close();
        Ok(())
    }

    fn opened(&mut self, rep: wasmtime::component::Resource<UsbDevice>) -> wasmtime::Result<bool> {
        let device = self.table().get(&rep)?;
        Ok(device.opened())
    }

    fn select_configuration(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        configuration: wasmtime::component::Resource<UsbConfiguration>,
    ) -> wasmtime::Result<()> {
        let table = self.table();
        let configuration = table.get(&configuration)?;
        let configuration_value = configuration.descriptor.number;
        let device = table.get_mut(&rep)?;
        device.select_configuration(configuration_value)?;
        Ok(())
    }

    fn claim_interface(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        interface: wasmtime::component::Resource<UsbInterface>,
    ) -> wasmtime::Result<()> {
        let table = self.table();
        let interface = table.get(&interface)?;
        let interface_number = interface.descriptor.interface_number;
        let interface_setting = interface.descriptor.alternate_setting;
        let device = table.get_mut(&rep)?;
        device.claim_interface(interface_number).unwrap();
        device
            .set_alternate_setting(interface_number, interface_setting)
            .unwrap();
        Ok(())
    }

    fn release_interface(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        interface: wasmtime::component::Resource<UsbInterface>,
    ) -> std::result::Result<(), wasmtime::Error> {
        let table = self.table();
        let interface = table.get(&interface)?.descriptor.interface_number;
        let device = table.get_mut(&rep)?;
        device.release_interface(interface)?;
        Ok(())
    }

    fn clear_halt(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
    ) -> wasmtime::Result<()> {
        let table = self.table();
        let endpoint = table.get(&endpoint)?;
        let endpoint_address = endpoint.get_endpoint_number();
        let device = table.get_mut(&rep)?;
        device.clear_halt(endpoint_address)?;
        Ok(())
    }

    fn read_interrupt(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
        length: u64,
    ) -> wasmtime::Result<Vec<u8>> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.get_endpoint_number();
        let device = table.get_mut(&rep)?;
        let data = device
            .interrupt_transfer_in(address, length as usize)
            .unwrap();
        Ok(data)
    }

    fn write_interrupt(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
        data: Vec<u8>,
    ) -> wasmtime::Result<u64> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.get_endpoint_number();
        let device = table.get_mut(&rep)?;
        let bytes_written = device.interrupt_transfer_out(address, &data).unwrap();
        Ok(bytes_written as _)
    }

    fn read_bulk(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
        length: u64,
    ) -> wasmtime::Result<Vec<u8>> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.descriptor.endpoint_number
            + match ep.descriptor.direction {
                crate::wadu436::usb::types::Direction::Out => 0x00,
                crate::wadu436::usb::types::Direction::In => 0x80,
            };
        let device = table.get_mut(&rep)?;
        let data = device.bulk_transfer_in(address, length as usize).unwrap();
        Ok(data)
    }

    fn write_bulk(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
        data: Vec<u8>,
    ) -> wasmtime::Result<u64> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.descriptor.endpoint_number
            + match ep.descriptor.direction {
                crate::wadu436::usb::types::Direction::Out => 0x00,
                crate::wadu436::usb::types::Direction::In => 0x80,
            };
        let device = table.get_mut(&rep)?;
        let bytes_written = device.bulk_transfer_out(address, &data).unwrap();
        Ok(bytes_written as _)
    }

    fn read_isochronous(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
    ) -> wasmtime::Result<Vec<u8>> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.descriptor.endpoint_number
            + match ep.descriptor.direction {
                crate::wadu436::usb::types::Direction::Out => 0x00,
                crate::wadu436::usb::types::Direction::In => 0x80,
            };
        let buffer_size = ep.descriptor.max_packet_size;
        let device = table.get_mut(&rep)?;
        let mut data = device
            .iso_transfer_in(address, 1, buffer_size.into())
            .unwrap();

        Ok(data.swap_remove(0))
    }

    fn write_isochronous(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        endpoint: wasmtime::component::Resource<UsbEndpoint>,
        data: Vec<u8>,
    ) -> wasmtime::Result<u64> {
        let table = self.table();
        let ep = table.get(&endpoint)?;
        let address = ep.descriptor.endpoint_number
            + match ep.descriptor.direction {
                crate::wadu436::usb::types::Direction::Out => 0x00,
                crate::wadu436::usb::types::Direction::In => 0x80,
            };
        let device = table.get_mut(&rep)?;
        let bytes_written = device.iso_transfer_out(address, &[data]).unwrap();
        Ok(bytes_written)
    }

    fn active_configuration(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
    ) -> std::result::Result<wasmtime::component::Resource<UsbConfiguration>, wasmtime::Error> {
        let table = self.table();
        let device = table.get_mut(&rep)?;
        let configuration = device.active_configuration()?;

        Ok(table.push(configuration)?)
    }

    fn speed(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
    ) -> std::result::Result<Speed, wasmtime::Error> {
        let table = self.table();
        let device = table.get_mut(&rep)?;
        let speed = device.speed();
        Ok(match speed {
            rusb::Speed::Unknown => Speed::Unknown,
            rusb::Speed::Low => Speed::Low,
            rusb::Speed::Full => Speed::Full,
            rusb::Speed::High => Speed::High,
            rusb::Speed::Super => Speed::Super,
            rusb::Speed::SuperPlus => Speed::Superplus,
            _ => Speed::Unknown,
        })
    }

    fn read_control(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        request: ControlSetup,
        length: u16,
    ) -> wasmtime::Result<Vec<u8>> {
        let table = self.table();
        let device = table.get_mut(&rep)?;
        let setup = host_control_setup_to_rusb(&request);
        let data = device.control_transfer_in(setup, length).unwrap();
        Ok(data)
    }

    fn write_control(
        &mut self,
        rep: wasmtime::component::Resource<UsbDevice>,
        request: ControlSetup,
        data: Vec<u8>,
    ) -> wasmtime::Result<u64> {
        let table = self.table();
        let device = table.get_mut(&rep)?;
        let setup = host_control_setup_to_rusb(&request);
        let bytes_written = device.control_transfer_out(setup, &data).unwrap();
        Ok(bytes_written as _)
    }
}

impl<T: WasiView> HostUsbConfiguration for T {
    fn descriptor(
        &mut self,
        rep: wasmtime::component::Resource<UsbConfiguration>,
    ) -> wasmtime::Result<ConfigurationDescriptor> {
        let configuration = self.table().get(&rep)?;
        Ok(configuration.descriptor.clone())
    }

    fn drop(
        &mut self,
        rep: wasmtime::component::Resource<UsbConfiguration>,
    ) -> wasmtime::Result<()> {
        self.table().delete(rep)?;
        Ok(())
    }

    fn interfaces(
        &mut self,
        rep: wasmtime::component::Resource<UsbConfiguration>,
    ) -> wasmtime::Result<Vec<wasmtime::component::Resource<UsbInterface>>> {
        let table = self.table();
        let configuration = table.get(&rep)?;

        Ok(configuration
            .get_interfaces()
            .into_iter()
            .map(|interface| table.push_child(interface, &rep))
            .collect::<Result<_, _>>()?)
    }
}

impl<T: WasiView> HostUsbInterface for T {
    fn descriptor(
        &mut self,
        rep: wasmtime::component::Resource<UsbInterface>,
    ) -> wasmtime::Result<InterfaceDescriptor> {
        let interface = self.table().get(&rep).unwrap();

        Ok(interface.descriptor.clone())
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<UsbInterface>) -> wasmtime::Result<()> {
        self.table().delete(rep)?;
        Ok(())
    }

    fn endpoints(
        &mut self,
        rep: wasmtime::component::Resource<UsbInterface>,
    ) -> wasmtime::Result<Vec<wasmtime::component::Resource<UsbEndpoint>>> {
        let table = self.table();
        let interface = table.get(&rep).unwrap();

        Ok(interface
            .get_endpoints()
            .into_iter()
            .map(|endpoint| table.push_child(endpoint, &rep))
            .collect::<Result<_, _>>()?)
    }
}

impl<T: WasiView> HostUsbEndpoint for T {
    fn descriptor(
        &mut self,
        rep: wasmtime::component::Resource<UsbEndpoint>,
    ) -> wasmtime::Result<EndpointDescriptor> {
        let endpoint = self.table().get(&rep).unwrap();

        Ok(endpoint.descriptor)
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<UsbEndpoint>) -> wasmtime::Result<()> {
        self.table().delete(rep)?;
        Ok(())
    }
}

impl<T: WasiView> Host for T {}
