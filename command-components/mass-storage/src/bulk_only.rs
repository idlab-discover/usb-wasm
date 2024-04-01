use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;
use tracing::trace;
use usb_wasm_bindings::{
    device::{UsbConfiguration, UsbDevice, UsbEndpoint, UsbInterface},
    types::{ControlSetup, ControlSetupRecipient, ControlSetupType, Direction, TransferType},
};

#[derive(Debug, Error)]
pub enum BulkOnlyTransportError {
    #[error("Invalid LUN")]
    InvalidLUN,
    #[error("The device responded with a differnt tag than was expected")]
    IncorrectTag,
}

// Implementation of the base Bulk Only Transfer protocol
pub struct BulkOnlyTransportDevice {
    bulk_in: UsbEndpoint,
    bulk_out: UsbEndpoint,
    _interface: UsbInterface, // We need to keep these alive because of the endpoint resources
    _configuration: UsbConfiguration, // We need to keep these alive because of the endpoint resources
    device: UsbDevice,
    current_tag: u32,
    selected_lun: u8,
    max_lun: u8,
}

impl BulkOnlyTransportDevice {
    // Also opens the device, selects the configuration, and claims the interface
    pub fn new(
        device: UsbDevice,
        configuration: UsbConfiguration,
        interface: UsbInterface,
    ) -> Self {
        device.open();
        device.reset();
        if device.active_configuration().descriptor().number != configuration.descriptor().number {
            device.select_configuration(&configuration);
        };
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

        // Get Max LUN
        let max_lun = {
            let lun_data = device.read_control(
                ControlSetup {
                    request_type: ControlSetupType::Class,
                    request_recipient: ControlSetupRecipient::Interface,
                    request: 0xFE,
                    value: 0,
                    index: interface.descriptor().interface_number as u16,
                },
                1,
            );
            lun_data[0]
        };

        BulkOnlyTransportDevice {
            device,
            _configuration: configuration,
            _interface: interface,

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
        self.device.write_bulk(&self.bulk_out, &cbw_bytes);

        // TODO: implement proper error recovery
        // First, implement errrors in the WIT interface though
        // then, see section 5.3.3 and Figure 2 of the USB Mass Storage Class – Bulk Only Transport document

        // TODO: data stage
        let transfer_length = cbw.transfer_length as usize;
        // Receive data
        let data = self.device.read_bulk(&self.bulk_in, transfer_length as u64);

        let csw_bytes = self.device.read_bulk(&self.bulk_in, 13);
        let csw = CommandStatusWrapper::from_bytes(csw_bytes);

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
        self.device.write_bulk(&self.bulk_out, &cbw_bytes);

        // TODO: implement proper error recovery
        // First, implement errrors in the WIT interface though
        // then, see section 5.3.3 and Figure 2 of the USB Mass Storage Class – Bulk Only Transport document

        if let Some(data) = data {
            self.device.write_bulk(&self.bulk_out, data);
        }

        let csw_bytes = self.device.read_bulk(&self.bulk_in, 13);
        let csw = CommandStatusWrapper::from_bytes(csw_bytes);

        if csw.tag != tag {
            return Err(BulkOnlyTransportError::IncorrectTag);
        }

        trace!("Received Command Status: {:?}", csw);
        Ok(csw)
    }

    fn get_tag(&mut self) -> u32 {
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
    tag: u32,
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
