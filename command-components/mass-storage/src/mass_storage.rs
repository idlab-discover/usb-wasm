use std::io::{self, Read, Seek, Write};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;
use tracing::trace;

use crate::bulk_only::{
    BulkOnlyTransportCommandBlock, BulkOnlyTransportDevice, CommandStatusWrapperStatus,
};

#[derive(Debug, Error)]
pub enum MassStorageDeviceError {
    #[error("Incompatible device")]
    IncompatibleDevice,
    #[error("Device is not ready yet")]
    NotReady,
}

#[derive(Debug, Default)]
pub struct MassStorageDeviceProperties {
    pub name: String,
    pub capacity: u64,
    pub total_number_of_blocks: u32,
    pub block_size: u32,
}

// Implementation of a Mass Storage USB Device using SCSI commands on top of a Bulk Only Transport USB device
pub struct MassStorageDevice {
    device: BulkOnlyTransportDevice,
    properties: MassStorageDeviceProperties,

    buffer: (u32, u32, Vec<u8>), // first block, last block (not inclusive), block data

    cursor: u64,
}

impl MassStorageDevice {
    pub fn new(device: BulkOnlyTransportDevice) -> Result<Self, MassStorageDeviceError> {
        let mut mass_storage_device = MassStorageDevice {
            device,
            properties: Default::default(),

            buffer: (0, 0, Vec::new()),

            cursor: 0,
        };

        // Inquiry properties
        if !mass_storage_device.test_unit_ready() {
            return Err(MassStorageDeviceError::NotReady);
        }

        let inquiry = mass_storage_device.inquiry();
        let capacity = mass_storage_device.read_capacity();

        if inquiry.peripheral_qualifier != 0 && inquiry.peripheral_device_type != 0 {
            return Err(MassStorageDeviceError::IncompatibleDevice);
        }

        let name = format!("{} {}", inquiry.vendor_id, inquiry.product_id);

        let properties = MassStorageDeviceProperties {
            name,
            capacity: capacity.block_length_in_bytes as u64
                * capacity.returned_logical_block_address as u64,
            block_size: capacity.block_length_in_bytes,
            total_number_of_blocks: capacity.returned_logical_block_address,
        };
        mass_storage_device.properties = properties;

        Ok(mass_storage_device)
    }

    pub fn get_properties(&self) -> &MassStorageDeviceProperties {
        &self.properties
    }

    // SCSI commands
    pub fn test_unit_ready(&mut self) -> bool {
        // We'll assume LUN 0
        let cbw = BulkOnlyTransportCommandBlock {
            command_block: vec![0x00; 6],
            transfer_length: 0,
        };

        let csw = self.device.command_out(cbw, None).unwrap();

        csw.status == CommandStatusWrapperStatus::CommandPassed
    }

    pub fn inquiry(&mut self) -> InquiryResponse {
        let cbw = BulkOnlyTransportCommandBlock {
            command_block: vec![0x12, 0x00, 0x00, 0x00, 36, 0x00],
            transfer_length: 36,
        };

        let (csw, data) = self.device.command_in(cbw).unwrap();
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            todo!("Handle command failure")
        }

        InquiryResponse::from_bytes(&data)
    }

    pub fn read_capacity(&mut self) -> ReadCapacityResponse {
        let cbw = BulkOnlyTransportCommandBlock {
            command_block: vec![0x25, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0],
            transfer_length: 8,
        };

        let (csw, data) = self.device.command_in(cbw).unwrap();
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            todo!("Handle command failure")
        }

        ReadCapacityResponse::from_bytes(&data)
    }

    pub fn read_blocks(&mut self, address: u32, blocks: u16) -> Vec<u8> {
        let mut command_block = BytesMut::new();
        command_block.put_u8(0x28); // OPCODE
        command_block.put_u8(0); // Fields I don't care about
        command_block.put_u32(address); // Logical block address
        command_block.put_u8(0); // Fields I don't care about
        command_block.put_u16(blocks); // Number of blocks to transfer
        command_block.put_u8(0); // CONTROL
        let command_block = command_block.to_vec();

        let cbw = BulkOnlyTransportCommandBlock {
            command_block,
            transfer_length: blocks as u32 * self.properties.block_size,
        };

        let (csw, data) = self.device.command_in(cbw).unwrap();
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            todo!("Handle command failure")
        }

        data

        // ReadCapacityResponse::from_bytes(&data)
    }
}

impl Seek for MassStorageDevice {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        match pos {
            std::io::SeekFrom::Start(offset) => self.cursor = offset,
            std::io::SeekFrom::End(offset) => {
                if offset > 0 {
                    self.cursor = self.properties.capacity + offset as u64
                } else {
                    self.cursor = self.properties.capacity - (-offset) as u64
                }
            }
            std::io::SeekFrom::Current(offset) => {
                if offset > 0 {
                    self.cursor += offset as u64
                } else {
                    self.cursor -= (-offset) as u64
                }
            }
        }
        Ok(self.cursor)
    }
}

impl Read for MassStorageDevice {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        trace!("Reading {} bytes at address {:x}", buf.len(), self.cursor);

        let start_address = self.cursor as usize;
        let end_address = (self.cursor + buf.len() as u64).min(self.properties.capacity) as usize; // Not-inclusive
        let num_bytes = end_address.saturating_sub(start_address);

        if num_bytes == 0 {
            // End of disk
            return Ok(0);
        }

        // First find which blocks we need to read
        let start_block = (start_address / self.properties.block_size as usize) as u32;
        let offset_in_start_block = start_address % self.properties.block_size as usize;
        let end_block = ((end_address - 1) / self.properties.block_size as usize) as u32; // Because end_address is not inclusive
        let num_blocks = (end_block - start_block + 1) as _;

        trace!(
            "Reading {} block(s) starting at block {}",
            num_blocks,
            start_block
        );

        if start_block >= self.buffer.0 && end_block < self.buffer.1 {
            // We can handle this request from the buffer
            trace!("Servicing request from the buffer");
            let offset_in_buffer =
                ((start_block - self.buffer.0) * self.properties.block_size) as usize + offset_in_start_block;
            buf.copy_from_slice(&self.buffer.2[offset_in_buffer..(offset_in_buffer + num_bytes)]);

            self.cursor += num_bytes as u64;
            
            return Ok(num_bytes);
        }

        let data = self.read_blocks(start_block as _, num_blocks);
        buf.copy_from_slice(&data[offset_in_start_block..(offset_in_start_block + num_bytes)]);

        self.buffer = (start_block, end_block+1, data);

        self.cursor += num_bytes as u64;

        Ok(num_bytes)
    }
}

impl Write for MassStorageDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        todo!()
    }

    fn flush(&mut self) -> io::Result<()> {
        // We don't buffer anything ourselves so we don't need to flush
        Ok(())
    }
}

#[derive(Debug)]
pub struct InquiryResponse {
    pub peripheral_qualifier: u8,
    pub peripheral_device_type: u8,
    pub removable_media: bool,
    pub version: u8,
    // normaca: bool,
    // hisup: bool,
    pub response_data_format: u8,
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
    pub vendor_id: String,
    pub product_id: String,
    pub product_revision: String,
}

impl InquiryResponse {
    fn from_bytes(data: &[u8]) -> Self {
        let mut data = Bytes::copy_from_slice(data);
        let peripheral = data.get_u8();
        let peripheral_qualifier = (peripheral & 0b11100000) >> 5;
        let peripheral_device_type = peripheral & 0b00011111;

        let removable_media = (data.get_u8() & 0b10000000) != 0;

        let version = data.get_u8();

        let response_data_format = data.get_u8() & 0b00001111;

        // Skip a couple bytes
        data.advance(4);

        let vendor_id = String::from_utf8(data[0..8].to_vec())
            .unwrap()
            .trim()
            .to_owned();
        let product_id = String::from_utf8(data[8..24].to_vec())
            .unwrap()
            .trim()
            .to_owned();
        let product_revision = String::from_utf8(data[24..28].to_vec())
            .unwrap()
            .trim()
            .to_owned();

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

#[derive(Debug)]
pub struct ReadCapacityResponse {
    pub returned_logical_block_address: u32,
    pub block_length_in_bytes: u32,
    pub capacity_in_bytes: u64,
}

impl ReadCapacityResponse {
    fn from_bytes(data: &[u8]) -> Self {
        let mut data = Bytes::copy_from_slice(data);
        let returned_logical_block_address = data.get_u32();
        let block_length_in_bytes = data.get_u32();
        let capacity_in_bytes: u64 =
            returned_logical_block_address as u64 * block_length_in_bytes as u64;

        Self {
            block_length_in_bytes,
            returned_logical_block_address,
            capacity_in_bytes,
        }
    }
}
