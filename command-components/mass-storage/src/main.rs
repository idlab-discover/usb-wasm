use std::{ffi::CStr, rc::Rc};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use thiserror::Error;
use usb_wasm_bindings::{
    descriptors::TransferType,
    device::{UsbConfiguration, UsbDevice, UsbEndpoint, UsbInterface},
    types::{ControlSetup, ControlSetupRecipient, ControlSetupType, Direction},
};

use anyhow::anyhow;

#[derive(Debug)]
struct CommandBlockWrapper {
    tag: u32,
    transfer_length: u32,
    direction: Direction,
    lun: u8,
    cbwcb: Vec<u8>,
}

impl CommandBlockWrapper {
    fn to_bytes(&self) -> Vec<u8> {
        let mut cbw = BytesMut::with_capacity(31);

        assert!(self.lun < 16, "Invalid LUN");
        assert!(
            self.cbwcb.len() >= 1 && self.cbwcb.len() <= 16,
            "Invalid CBWCB length"
        );

        cbw.put_u32_le(0x43425355);
        cbw.put_u32_le(self.tag);
        cbw.put_u32_le(self.transfer_length);
        cbw.put_u8(match self.direction {
            Direction::Out => 0b00000000,
            Direction::In => 0b10000000,
        });
        cbw.put_u8(self.lun);
        cbw.put_u8(self.cbwcb.len() as u8);
        cbw.put_slice(&self.cbwcb);
        cbw.put_bytes(0, 16 - self.cbwcb.len());

        cbw.to_vec()
    }
}

#[derive(Debug)]
struct CommandStatusWrapper {
    tag: u32,
    data_residue: u32,
    status: CommandStatusWrapperStatus,
}

#[derive(Debug, PartialEq)]
enum CommandStatusWrapperStatus {
    CommandPassed, // Good
    CommandFailed,
    PhaseError,
    ReservedObsolete,
    Reserved,
}

impl CommandStatusWrapper {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        assert!(bytes.len() == 13, "CSW incorrect length");
        let mut bytes = Bytes::from(bytes);

        let signature = bytes.get_u32_le();
        assert!(signature == 0x53425355, "invalid CSW signature");

        let tag = bytes.get_u32_le();
        let data_residue = bytes.get_u32_le();
        let status = match bytes.get_u8() {
            0 => CommandStatusWrapperStatus::CommandPassed,
            1 => CommandStatusWrapperStatus::CommandFailed,
            2 => CommandStatusWrapperStatus::PhaseError,
            3..=4 => CommandStatusWrapperStatus::ReservedObsolete,
            _ => CommandStatusWrapperStatus::Reserved,
        };

        CommandStatusWrapper {
            tag,
            data_residue,
            status,
        }
    }
}

#[derive(Debug)]
struct InquiryResponse {
    peripheral_qualifier: u8,
    peripheral_device_type: u8,
    removable_media: bool,
    version: u8,
    // normaca: bool,
    // hisup: bool,
    response_data_format: u8,
    // sccs: bool,
    // acc: bool,
    // tpgs: u8,
    // _3pc: bool,
    // protect: bool,
    // encserv: bool,
    // vs: bool,
    // multip: bool,
    // cmdque: bool,
    // vs2: bool,
    vendor_id: String,
    product_id: String,
    product_revision: String,
}

impl InquiryResponse {
    fn from_bytes(data: &[u8]) -> Self {
        let mut data = Bytes::copy_from_slice(data);
        let peripheral = data.get_u8();
        let peripheral_qualifier = (peripheral & 0b11100000) >> 5;
        let peripheral_device_type = peripheral & 0b00011111;

        println!("peripheral_qualifier, {}", peripheral_qualifier);
        println!("peripheral_device_type, {}", peripheral_device_type);

        let removable_media = (data.get_u8() & 0b10000000) != 0;
        println!("removable media: {}", removable_media);

        let version = data.get_u8();

        let response_data_format =data.get_u8() & 0b00001111;

        // Skip a couple bytes
        data.advance(4);

        let vendor_id = String::from_utf8(data[0..8].to_vec()).unwrap().trim().to_owned();
        let product_id = String::from_utf8(data[8..24].to_vec()).unwrap().trim().to_owned();
        let product_revision = String::from_utf8(data[24..28].to_vec()).unwrap().trim().to_owned();

        InquiryResponse {
            peripheral_qualifier,
            peripheral_device_type,
            removable_media,
            version,
            response_data_format,
            vendor_id,
            product_id,
            product_revision,
        }
    }
}

struct MassStorageDevice {
    device: Rc<UsbDevice>,
    configuration: Rc<UsbConfiguration>,
    interface: Rc<UsbInterface>,
    bulk_in: UsbEndpoint,
    bulk_out: UsbEndpoint,
    current_tag: u32,
}

impl MassStorageDevice {
    fn new(
        device: Rc<UsbDevice>,
        configuration: Rc<UsbConfiguration>,
        interface: Rc<UsbInterface>,
    ) -> Self {
        device.open();
        device.reset();
        device.select_configuration(&configuration);
        device.claim_interface(&interface);

        // Find endpoints
        let (bulk_in, bulk_out) = {
            (
                interface
                    .endpoints()
                    .into_iter()
                    .find(|ep| {
                        ep.descriptor().direction == Direction::In
                            && ep.descriptor().transfer_type == TransferType::Bulk
                    })
                    .unwrap(),
                interface
                    .endpoints()
                    .into_iter()
                    .find(|ep| {
                        ep.descriptor().direction == Direction::Out
                            && ep.descriptor().transfer_type == TransferType::Bulk
                    })
                    .unwrap(),
            )
        };

        MassStorageDevice {
            device,
            configuration,
            interface,

            bulk_in,
            bulk_out,

            current_tag: 0,
        }
    }

    fn send_command_block(
        &mut self,
        cbw: CommandBlockWrapper,
        data_out: Option<Vec<u8>>,
    ) -> (CommandStatusWrapper, Vec<u8>) {
        println!("Sending Command Block: {:?}", cbw);
        let cbw_bytes = cbw.to_bytes();
        println!("CBW Bytes: {:?}", cbw_bytes);
        self.device.write_bulk(&self.bulk_out, &cbw_bytes);

        // TODO: implement proper error recovery
        // First, implement errrors in the WIT interface though
        // then, see section 5.3.3 and Figure 2 of the USB Mass Storage Class – Bulk Only Transport document

        // TODO: data stage
        let data = if cbw.transfer_length > 0 {
            if cbw.direction == Direction::In {
                // Receive data
                let data = self.device.read_bulk(&self.bulk_in);
                data
            } else {
                // Send data
                let data_out = data_out.unwrap_or(Vec::new());
                assert!(
                    data_out.len() == cbw.transfer_length as usize,
                    "provided data buffer is incorrect length"
                );
                self.device.write_bulk(&self.bulk_out, &data_out);
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let csw_bytes = self.device.read_bulk(&self.bulk_in);
        let csw = CommandStatusWrapper::from_bytes(csw_bytes);
        println!("Received Command Block: {:?}", csw);
        (csw, data)
    }

    // Bulk Only Transport commands
    fn get_max_lun(&self) -> u8 {
        let lun_data = self.device.read_control(
            ControlSetup {
                request_type: ControlSetupType::Class,
                request_recipient: ControlSetupRecipient::Interface,
                request: 0xFE,
                value: 0,
                index: self.interface.descriptor().interface_number as u16,
            },
            1,
        );
        lun_data[0]
    }

    fn get_tag(&mut self) -> u32 {
        let tag = self.current_tag;
        self.current_tag += 1;
        tag
    }

    // SCSI commands
    fn test_unit_ready(&mut self, lun: u8) -> bool {
        // We'll assume LUN 0
        let cbw = CommandBlockWrapper {
            tag: self.get_tag(),
            transfer_length: 0,
            direction: Direction::Out,
            lun,
            cbwcb: vec![0x00; 6],
        };

        let (csw, _) = self.send_command_block(cbw, None);

        csw.status == CommandStatusWrapperStatus::CommandPassed
    }

    fn inquiry(&mut self, lun: u8) -> InquiryResponse {
        let cbw = CommandBlockWrapper {
            tag: self.get_tag(),
            transfer_length: 36,
            direction: Direction::In,
            lun,
            cbwcb: vec![0x12, 0x00, 0x00, 0x00, 36, 0x00],
        };

        let (csw, data) = self.send_command_block(cbw, None);
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            todo!("Handle command failure")
        }

        println!("Data: {:?}", data);
        InquiryResponse::from_bytes(&data)
    }
}

pub fn main() -> anyhow::Result<()> {
    // Find device
    let (device, configuration, interface) = {
        let mut mass_storage_interfaces: Vec<(
            Rc<UsbDevice>,
            Rc<UsbConfiguration>,
            Rc<UsbInterface>,
        )> = Vec::new();
        for device in UsbDevice::enumerate().into_iter().map(Rc::new) {
            for configuration in device.configurations().into_iter().map(Rc::new) {
                for interface in configuration.interfaces().into_iter().map(Rc::new) {
                    let if_descriptor = interface.descriptor();
                    if if_descriptor.interface_class == 0x08
                        && if_descriptor.interface_protocol == 0x50
                    {
                        mass_storage_interfaces.push((
                            device.clone(),
                            configuration.clone(),
                            interface,
                        ));
                    }
                }
            }
        }
        mass_storage_interfaces.iter().enumerate().for_each(|(i,(device,configuration,interface))| {
        let device_descriptor = device.descriptor();
        let configuration_descriptor = configuration.descriptor();
        let if_descriptor = interface.descriptor();
        println!("{}. USB Mass Storage Device Bulk Only Transport found: device {:04x}:{:04x}, configuration {}, interface {}", i, device_descriptor.vendor_id,device_descriptor.product_id,configuration_descriptor.number, if_descriptor.interface_number);
    });

        if mass_storage_interfaces.len() == 0 {
            return Err(anyhow!("No mass storage devices found. Exiting."));
        }

        println!("Using first device.");
        mass_storage_interfaces.swap_remove(0)
    };

    let mut msd = MassStorageDevice::new(device, configuration, interface);

    let max_lun = msd.get_max_lun();
    println!("max lun: {}", max_lun);
    let lun = 0;

    // Check if the device is ready to read/write
    let ready = msd.test_unit_ready(lun);
    if !ready {
        return Err(anyhow!("Device is not ready"));
    }
    println!("Device is ready");

    let inquiry_response = msd.inquiry(lun);
    println!("inquiry_response {:?}", inquiry_response);

    println!("USB Drive Info:");
    println!("Direct access block device: {}", inquiry_response.peripheral_device_type==0);
    println!("Removable: {}", inquiry_response.removable_media);
    println!("Obsolete format: {}", inquiry_response.response_data_format < 2);
    println!("Product name: {} {} {}", inquiry_response.vendor_id, inquiry_response.product_id, inquiry_response.product_revision);

    Ok(())
}
