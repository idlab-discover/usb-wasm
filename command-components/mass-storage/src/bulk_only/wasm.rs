use bytes::{Buf, BufMut, Bytes, BytesMut};

use thiserror::Error;

use tracing::trace;

use usb_wasm_bindings::configuration::ConfigValue;
use usb_wasm_bindings::descriptors::{ConfigurationDescriptor, InterfaceDescriptor};
use usb_wasm_bindings::device::{DeviceHandle, UsbDevice};
use usb_wasm_bindings::transfers::{await_transfer, TransferOptions, TransferSetup, TransferType};

#[derive(Debug, Error)]
pub enum BulkOnlyTransportError {
    #[error("Invalid LUN")]
    InvalidLUN,
    #[error("The device responded with a different tag than was expected")]
    IncorrectTag,
}

// Implementation of the base Bulk Only Transfer protocol
pub struct BulkOnlyTransportDevice {
    pub(crate) bulk_in: u8,
    pub(crate) bulk_out: u8,
    pub(crate) handle: DeviceHandle,
    pub(crate) current_tag: u32,
    pub(crate) selected_lun: u8,
    pub(crate) max_lun: u8,
}

impl BulkOnlyTransportDevice {
    // Also opens the device, selects the configuration, and claims the interface
    pub fn new(
        device: UsbDevice,
        configuration: ConfigurationDescriptor,
        interface: InterfaceDescriptor,
    ) -> Self {
        let handle = device.open().expect("Failed to open device");
        handle.reset_device().ok();

        // Check active configuration
        let active_config = handle.get_configuration().unwrap_or(0);
        if active_config != configuration.configuration_value {
            handle
                .set_configuration(ConfigValue::Value(configuration.configuration_value))
                .ok();
        }

        handle.claim_interface(interface.interface_number).ok();

        // Find endpoints
        let mut bulk_in = 0;
        let mut bulk_out = 0;

        for ep in &interface.endpoints {
            let is_in = (ep.endpoint_address & 0x80) != 0;
            let is_bulk = (ep.attributes & 0x03) == 0x02;
            if is_bulk {
                if is_in {
                    bulk_in = ep.endpoint_address;
                } else {
                    bulk_out = ep.endpoint_address;
                }
            }
        }

        // Get Max LUN
        let setup = TransferSetup {
            bm_request_type: 0xA1, // Class, Interface, IN
            b_request: 0xFE,       // GET_MAX_LUN
            w_value: 0,
            w_index: interface.interface_number as u16,
        };
        let opts = TransferOptions {
            endpoint: 0,
            timeout_ms: 1000,
            stream_id: 0,
            iso_packets: 0,
        };

        let xfer = handle
            .new_transfer(TransferType::Control, setup, 1, opts)
            .expect("Failed to create GET_MAX_LUN transfer");
        xfer.submit_transfer(&[]).ok();
        let lun_data = await_transfer(xfer).unwrap_or(vec![0]);
        let max_lun = lun_data[0];

        BulkOnlyTransportDevice {
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

        // Write CBW
        let opts_out = TransferOptions {
            endpoint: self.bulk_out,
            timeout_ms: 5000,
            stream_id: 0,
            iso_packets: 0,
        };
        let xfer_cbw = self
            .handle
            .new_transfer(
                TransferType::Bulk,
                empty_setup(),
                cbw_bytes.len() as u32,
                opts_out,
            )
            .expect("CBW transfer");
        xfer_cbw.submit_transfer(&cbw_bytes).ok();
        await_transfer(xfer_cbw).ok();

        let transfer_length = cbw.transfer_length as usize;

        // Receive Data
        let opts_in = TransferOptions {
            endpoint: self.bulk_in,
            timeout_ms: 10000,
            stream_id: 0,
            iso_packets: 0,
        };
        let data = if transfer_length > 0 {
            let xfer_data = self
                .handle
                .new_transfer(
                    TransferType::Bulk,
                    empty_setup(),
                    transfer_length as u32,
                    opts_in.clone(),
                )
                .expect("Data transfer");
            xfer_data.submit_transfer(&[]).ok();
            await_transfer(xfer_data).unwrap_or_default()
        } else {
            vec![]
        };

        // Receive CSW
        let xfer_csw = self
            .handle
            .new_transfer(TransferType::Bulk, empty_setup(), 13, opts_in)
            .expect("CSW transfer");
        xfer_csw.submit_transfer(&[]).ok();
        let csw_bytes = await_transfer(xfer_csw).unwrap_or_default();

        if csw_bytes.len() != 13 {
            return Err(BulkOnlyTransportError::IncorrectTag);
        }

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

        let opts_out = TransferOptions {
            endpoint: self.bulk_out,
            timeout_ms: 5000,
            stream_id: 0,
            iso_packets: 0,
        };

        // Write CBW
        let xfer_cbw = self
            .handle
            .new_transfer(
                TransferType::Bulk,
                empty_setup(),
                cbw_bytes.len() as u32,
                opts_out.clone(),
            )
            .expect("CBW transfer");
        xfer_cbw.submit_transfer(&cbw_bytes).ok();
        await_transfer(xfer_cbw).ok();

        // Write Data
        if let Some(data) = data {
            let xfer_data = self
                .handle
                .new_transfer(
                    TransferType::Bulk,
                    empty_setup(),
                    data.len() as u32,
                    opts_out,
                )
                .expect("Data transfer");
            xfer_data.submit_transfer(data).ok();
            await_transfer(xfer_data).ok();
        }

        // Receive CSW
        let opts_in = TransferOptions {
            endpoint: self.bulk_in,
            timeout_ms: 5000,
            stream_id: 0,
            iso_packets: 0,
        };
        let xfer_csw = self
            .handle
            .new_transfer(TransferType::Bulk, empty_setup(), 13, opts_in)
            .expect("CSW transfer");
        xfer_csw.submit_transfer(&[]).ok();
        let csw_bytes = await_transfer(xfer_csw).unwrap_or_default();

        if csw_bytes.len() != 13 {
            return Err(BulkOnlyTransportError::IncorrectTag);
        }

        let csw = CommandStatusWrapper::from_bytes(csw_bytes);

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

fn empty_setup() -> TransferSetup {
    TransferSetup {
        bm_request_type: 0,
        b_request: 0,
        w_value: 0,
        w_index: 0,
    }
}

pub struct BulkOnlyTransportCommandBlock {
    pub command_block: Vec<u8>,
    pub transfer_length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
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
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
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
            3..4 => CommandStatusWrapperStatus::ReservedObsolete,
            _ => CommandStatusWrapperStatus::Reserved,
        };

        CommandStatusWrapper {
            tag,
            data_residue,
            status,
        }
    }
}
