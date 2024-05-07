use std::time::Duration;

use bytes::{Buf, BufMut, Bytes, BytesMut};

use rusb::{request_type, Device, DeviceHandle, Direction, GlobalContext, TransferType};
use thiserror::Error;

use tracing::trace;

const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Error)]
pub enum BulkOnlyTransportError {
    #[error("Invalid LUN")]
    InvalidLUN,
    #[error("The device responded with a differnt tag than was expected")]
    IncorrectTag,
}

// Implementation of the base Bulk Only Transfer protocol
pub struct BulkOnlyTransportDevice {
    pub(crate) bulk_in: u8,
    pub(crate) bulk_out: u8,
    pub(crate) _device: Device<GlobalContext>,
    pub(crate) handle: DeviceHandle<GlobalContext>,
    pub(crate) current_tag: u32,
    pub(crate) selected_lun: u8,
    pub(crate) max_lun: u8,
}

impl BulkOnlyTransportDevice {
    // Also opens the device, selects the configuration, and claims the interface
    pub fn new(device: Device<GlobalContext>, configuration: u8, interface: u8) -> Self {
        let config_descriptor = device.config_descriptor(configuration).unwrap();

        let handle = device.open().unwrap();
        handle.set_auto_detach_kernel_driver(true).unwrap();
        handle.reset().unwrap();
        if handle.active_configuration().unwrap() != config_descriptor.number() {
            handle.set_active_configuration(configuration).unwrap();
        };
        handle.claim_interface(interface).unwrap();

        println!("configuration: {:?}", configuration);

        let interface_descriptor = config_descriptor
            .interfaces()
            .find(|i| i.number() == interface)
            .unwrap()
            .descriptors()
            .next()
            .unwrap();

        // Find endpoints
        let (bulk_in, bulk_out) = {
            let mut endpoints = interface_descriptor.endpoint_descriptors();
            (
                endpoints
                    .find(|ep| {
                        ep.direction() == Direction::In && ep.transfer_type() == TransferType::Bulk
                    })
                    .unwrap()
                    .address(),
                endpoints
                    .find(|ep| {
                        ep.direction() == Direction::Out && ep.transfer_type() == TransferType::Bulk
                    })
                    .unwrap()
                    .address(),
            )
        };

        // Get Max LUN
        let max_lun = {
            let mut data = [0u8; 1];
            handle
                .read_control(
                    request_type(
                        Direction::In,
                        rusb::RequestType::Class,
                        rusb::Recipient::Interface,
                    ),
                    0xFE,
                    0,
                    interface as u16,
                    &mut data[..],
                    TIMEOUT,
                )
                .unwrap();

            data[0]
        };

        BulkOnlyTransportDevice {
            _device: device,
            handle,

            bulk_in,
            bulk_out,

            current_tag: 0,

            selected_lun: 0,
            max_lun,
        }
    }

    pub fn max_lun(&self) -> u8 {
        self.max_lun
    }

    pub fn selected_lun(&self) -> u8 {
        self.selected_lun
    }

    pub fn select_lun(&mut self, lun: u8) -> Result<(), BulkOnlyTransportError> {
        if self.max_lun > lun {
            return Err(BulkOnlyTransportError::InvalidLUN);
        }
        self.selected_lun = lun;
        Ok(())
    }

    // Device to Host
    pub fn command_in(
        &mut self,
        command_block: BulkOnlyTransportCommandBlock,
    ) -> Result<(CommandStatusWrapper, Vec<u8>), BulkOnlyTransportError> {
        let tag = self.get_tag();
        let cbw = CommandBlockWrapper {
            tag,
            transfer_length: command_block.transfer_length,
            direction: Direction::In,
            lun: self.selected_lun,
            cbwcb: command_block.command_block,
        };

        trace!(
            tag,
            lun = self.selected_lun,
            transfer_length = command_block.transfer_length,
            "Sending command to device {:02x?}",
            cbw.cbwcb
        );
        let cbw_bytes = cbw.to_bytes();
        self.handle
            .write_bulk(self.bulk_out, &cbw_bytes, TIMEOUT)
            .unwrap();

        // TODO: implement proper error recovery
        // First, implement errrors in the WIT interface though
        // then, see section 5.3.3 and Figure 2 of the USB Mass Storage Class – Bulk Only Transport document

        // TODO: data stage
        let transfer_length = cbw.transfer_length as usize;
        // Receive data
        let mut data = vec![0u8; transfer_length];
        self.handle
            .read_bulk(self.bulk_in, &mut data, TIMEOUT)
            .unwrap();

        let mut csw_bytes = [0u8; 13];
        self.handle
            .read_bulk(self.bulk_in, &mut csw_bytes, TIMEOUT)
            .unwrap();
        let csw = CommandStatusWrapper::from_bytes(&csw_bytes);

        if csw.tag != tag {
            return Err(BulkOnlyTransportError::IncorrectTag);
        }

        trace!("Received Command Status: {:?}", csw);
        Ok((csw, data))
    }

    pub fn command_out(
        &mut self,
        command_block: BulkOnlyTransportCommandBlock,
        data: Option<&[u8]>,
    ) -> Result<CommandStatusWrapper, BulkOnlyTransportError> {
        let tag = self.get_tag();
        let cbw = CommandBlockWrapper {
            tag,
            transfer_length: command_block.transfer_length,
            direction: Direction::Out,
            lun: self.selected_lun,
            cbwcb: command_block.command_block,
        };

        trace!(
            tag,
            lun = self.selected_lun,
            transfer_length = command_block.transfer_length,
            "Sending command to device {:02x?}",
            cbw.cbwcb
        );
        let cbw_bytes = cbw.to_bytes();
        trace!("CBW Bytes: {:?}", cbw_bytes);
        self.handle
            .write_bulk(self.bulk_out, &cbw_bytes, TIMEOUT)
            .unwrap();

        // TODO: implement proper error recovery
        // First, implement errrors in the WIT interface though
        // then, see section 5.3.3 and Figure 2 of the USB Mass Storage Class – Bulk Only Transport document

        if let Some(data) = data {
            self.handle
                .write_bulk(self.bulk_out, data, TIMEOUT)
                .unwrap();
        }

        let mut csw_bytes = [0_u8; 13];
        self.handle
            .read_bulk(self.bulk_in, &mut csw_bytes, TIMEOUT)
            .unwrap();
        let csw = CommandStatusWrapper::from_bytes(&csw_bytes);

        if csw.tag != tag {
            return Err(BulkOnlyTransportError::IncorrectTag);
        }

        trace!("Received Command Status: {:?}", csw);
        Ok(csw)
    }

    pub(crate) fn get_tag(&mut self) -> u32 {
        let tag = self.current_tag;
        self.current_tag += 1;
        tag
    }
}

pub struct BulkOnlyTransportCommandBlock {
    pub command_block: Vec<u8>,
    pub transfer_length: u32,
}

#[derive(Debug)]
pub(crate) struct CommandBlockWrapper {
    pub(crate) tag: u32,
    pub(crate) transfer_length: u32,
    pub(crate) direction: Direction,
    pub(crate) lun: u8,
    pub(crate) cbwcb: Vec<u8>,
}

impl CommandBlockWrapper {
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut cbw = BytesMut::with_capacity(31);

        assert!(self.lun < 16, "Invalid LUN");
        assert!(
            !self.cbwcb.is_empty() && self.cbwcb.len() <= 16,
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

// TODO: make this private after implementing struct CommandStatus {}
#[derive(Debug)]
pub struct CommandStatusWrapper {
    pub(crate) tag: u32,
    pub data_residue: u32,
    pub status: CommandStatusWrapperStatus,
}

#[derive(Debug, PartialEq)]
pub enum CommandStatusWrapperStatus {
    CommandPassed, // Good
    CommandFailed,
    PhaseError,
    ReservedObsolete,
    Reserved,
}

impl CommandStatusWrapper {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() == 13, "CSW incorrect length");
        let mut bytes = Bytes::copy_from_slice(bytes);

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
