use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::{self, Read, Seek, Write};
use thiserror::Error;
use tracing::{debug, trace};

use crate::bulk_only::{
    BulkOnlyTransportCommandBlock, BulkOnlyTransportDevice, CommandStatusWrapperStatus,
};
use uluru::LRUCache;

const CACHE_SIZE: usize = 128;

#[derive(Debug, Error)]
pub enum MassStorageDeviceError {
    #[error("Incompatible device")]
    IncompatibleDevice,
    #[error("Device is not ready yet")]
    NotReady,
}

#[derive(Debug, Default, Clone)]
pub struct MassStorageDeviceProperties {
    pub name: String,
    pub capacity: u64,
    pub total_number_of_blocks: u32,
    pub block_size: u32,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    block: u32,
    data: [u8; 512],
    dirty: bool,
}

impl CacheEntry {
    fn new(block: u32, data: [u8; 512], dirty: bool) -> Self {
        Self { block, data, dirty }
    }

    fn from_vec(block: u32, data: &[u8], dirty: bool) -> Self {
        let mut entry = Self::new(block, [0; 512], dirty);
        entry.data.copy_from_slice(data);
        entry
    }
}

// Implementation of a Mass Storage USB Device using SCSI commands on top of a Bulk Only Transport USB device
pub struct MassStorageDevice {
    device: BulkOnlyTransportDevice,
    properties: MassStorageDeviceProperties,

    cache: LRUCache<CacheEntry, CACHE_SIZE>, // Block -> Data
    reads: usize,
    blocks_read: usize,
    writes: usize,
    blocks_written: usize,

    cursor: u64,
}

impl MassStorageDevice {
    pub fn new(device: BulkOnlyTransportDevice) -> Result<Self, MassStorageDeviceError> {
        let mut mass_storage_device = MassStorageDevice {
            device,
            properties: Default::default(),
            cache: LRUCache::default(),
            cursor: 0,
            reads: 0,
            blocks_read: 0,
            writes: 0,
            blocks_written: 0,
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

    pub fn get_properties(&self) -> MassStorageDeviceProperties {
        self.properties.clone()
    }

    pub fn flush_cache(&mut self) {
        let mut entries_to_write = Vec::<(u32, [u8; 512])>::new();
        self.cache.iter().for_each(|entry| {
            if entry.dirty {
                entries_to_write.push((entry.block, entry.data));
            }
        });

        for (block, data) in entries_to_write {
            self.write_blocks(block, 1, &data);
        }

        self.cache.clear();
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
        self.blocks_read += blocks as usize;
        self.reads += 1;
        // println!("Reading {} block(s) starting at block {}", blocks, address);
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
    }

    pub fn write_blocks(&mut self, address: u32, blocks: u16, data: &[u8]) {
        self.blocks_written += blocks as usize;
        self.writes += 1;
        // println!("Writing {} blocks at address {:x}", blocks, address);
        let mut command_block = BytesMut::new();
        command_block.put_u8(0x2A); // OPCODE
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

        let csw = self.device.command_out(cbw, Some(data)).unwrap();
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            self.request_sense();
        }
    }

    pub fn request_sense(&mut self) {
        let mut command_block = BytesMut::new();
        command_block.put_u8(0x03);
        command_block.put_u8(0x00);
        command_block.put_u8(0x00);
        command_block.put_u8(0x00);
        command_block.put_u8(252);
        let command_block = command_block.to_vec();

        let cbw = BulkOnlyTransportCommandBlock {
            command_block,
            transfer_length: 252,
        };

        let (csw, data) = self.device.command_in(cbw).unwrap();
        if csw.status != CommandStatusWrapperStatus::CommandPassed {
            todo!("Handle command failure")
        }

        let mut bytes: bytes::Bytes = data.into();
        let valid_and_response_code = bytes.get_u8();
        let valid = valid_and_response_code & 0b10000000;
        let response_code = valid_and_response_code & 0b01111111;

        bytes.advance(1);

        let sense_key = bytes.get_u8() & 0b00001111;
        let information = bytes.get_u32();
        let additional_sense_length = bytes.get_u8();

        let command_specific_information = bytes.get_u32();

        let additional_sense_code = bytes.get_u8();
        let additional_sense_code_qualifier = bytes.get_u8();

        println!("valid: {}", valid);
        println!("response_code: {:x?}", response_code);
        println!("sense_key: {:x?}", sense_key);
        println!("information: {:x?}", information);
        println!("additional_sense_length: {:x?}", additional_sense_length);
        println!(
            "command_specific_information: {:x?}",
            command_specific_information
        );
        println!("additional_sense_code: {:x?}", additional_sense_code);
        println!(
            "additional_sense_code_qualifier: {:x?}",
            additional_sense_code_qualifier
        );
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
        let num_blocks = (end_block - start_block + 1) as usize;
        let offset_in_end_block = ((end_address - 1) % self.properties.block_size as usize) + 1;

        trace!(
            "Reading {} block(s) starting at block {}",
            num_blocks,
            start_block
        );

        let mut new_range: Option<u32> = None;
        let mut ranges: Vec<(u32, u32)> = Vec::new();

        for block in start_block..end_block + 1 {
            if let Some(entry) = self.cache.find(|item| item.block == block) {
                // Found in cache
                if block == start_block && block == end_block {
                    buf[..].copy_from_slice(&entry.data[offset_in_start_block..offset_in_end_block]);
                } else if block == start_block {
                    buf[..512 - offset_in_start_block]
                        .copy_from_slice(&entry.data[offset_in_start_block..]);
                } else if block == end_block {
                    buf[(512 - offset_in_start_block) + (num_blocks as usize - 2) * 512..]
                        .copy_from_slice(&entry.data[..offset_in_end_block]);
                } else {
                    buf[(512 - offset_in_start_block) + ((start_block - block) as usize) * 512
                        ..(512 - offset_in_start_block)
                            + ((start_block - block) as usize) * 512
                            + 512]
                        .copy_from_slice(&entry.data);
                }

                if let Some(new_range) = new_range {
                    ranges.push((new_range, block));
                }
            } else {
                if new_range.is_none() {
                    // We're at the start of the range
                    new_range = Some(block);
                }
            }
        }
        if let Some(new_range) = new_range {
            ranges.push((new_range, end_block + 1));
        }

        tracing::trace!(
            start_address,
            end_address,
            start_block,
            offset_in_start_block,
            end_block,
            num_blocks,
            num_bytes,
            buf_len = buf.len(),
            "reading"
        );

        for (start, end) in ranges {
            let data = self.read_blocks(start_block as _, (end - start) as _);
            // Put the data into the cache
            let range = if data.len() > 512 * CACHE_SIZE {
                data.len() - (512 * CACHE_SIZE)..data.len()
            } else {
                0..data.len()
            };
            for (i, chunk) in data[range].chunks(512).enumerate() {
                let block = start + i as u32;

                if let Some(item) = self.cache.find(|item| item.block == block) {
                    item.data.copy_from_slice(&chunk);
                } else {
                    let value = CacheEntry::from_vec(block, &chunk, false);
                    if let Some(evicted_entry) = self.cache.insert(value) {
                        if evicted_entry.dirty {
                            println!("Flushing block {}", block);
                            self.write_blocks(block, 1, &evicted_entry.data);
                        }
                    }
                }
            }
            // Copy the data into the buffer
            if start == start_block && end == end_block + 1 {
                buf[..].copy_from_slice(
                    &data[offset_in_start_block
                        ..(num_blocks as usize - 1) * 512 + offset_in_end_block],
                );
            } else if start == start_block {
                buf[..(512 - offset_in_start_block) + ((end - start_block - 1) as usize) * 512]
                    .copy_from_slice(&data[offset_in_start_block..(num_blocks as usize - 1) * 512]);
            } else if end == end_block + 1 {
                buf[(512 - offset_in_start_block) + ((start - start_block - 1) as usize) * 512..]
                    .copy_from_slice(
                        &data[..(num_blocks as usize - 1) * 512 + offset_in_end_block],
                    );
            } else {
                // General case
                buf[(512 - offset_in_start_block) + ((start - start_block - 1) as usize) * 512
                    ..(512 - offset_in_start_block) + ((end - start_block - 1) as usize) * 512]
                    .copy_from_slice(&data[..]);
            }
        }

        self.cursor += num_bytes as u64;

        Ok(num_bytes)
    }
}

impl Write for MassStorageDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // println!("Writing {} bytes at address {:x}", buf.len(), self.cursor);

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
        let num_blocks: u16 = (end_block - start_block + 1) as _;

        tracing::trace!(
            start_address,
            end_address,
            start_block,
            offset_in_start_block,
            end_block,
            num_blocks,
            num_bytes,
            buf_len = buf.len(),
            "writing"
        );

        debug!(
            "Writing {} block(s) starting at block {}",
            num_blocks, start_block
        );

        if num_blocks == 1 {
            let mut data = vec![0_u8; 512];
            if let Some(block) = self.cache.find(|item| item.block == start_block) {
                data.copy_from_slice(&block.data);
            } else {
                let original_data = self.read_blocks(start_block, 1);
                data.copy_from_slice(&original_data);
            }
            data[offset_in_start_block..offset_in_start_block + buf.len()].copy_from_slice(buf);
            // Update the cache
            if let Some(item) = self.cache.find(|item| item.block == start_block) {
                item.data.copy_from_slice(&data);
                item.dirty = true;
            } else {
                let value = CacheEntry::from_vec(start_block, &data, false);
                if let Some(evicted_entry) = self.cache.insert(value) {
                    if evicted_entry.dirty {
                        self.write_blocks(start_block, 1, &evicted_entry.data);
                    }
                }
            }
        } else {
            let mut data = vec![0_u8; num_blocks as usize * 512];
            let data_len = data.len();
            // First block
            if let Some(block) = self.cache.find(|item| item.block == start_block) {
                data[0..512].copy_from_slice(&block.data);
            } else {
                let original_data = self.read_blocks(start_block, 1);
                data[0..512].copy_from_slice(&original_data);
            }

            // Last block
            if let Some(block) = self.cache.find(|item| item.block == end_block) {
                data[data_len - 512..data_len].copy_from_slice(&block.data);
            } else {
                let original_data = self.read_blocks(end_block, 1);
                data[data_len - 512..data_len].copy_from_slice(&original_data);
            }
            data[offset_in_start_block..offset_in_start_block + buf.len()].copy_from_slice(buf);

            self.write_blocks(start_block, num_blocks, &data);

            // Update the cache
            for i in 0..num_blocks {
                let key = start_block + i as u32;
                // If the evicted block was dirty, we don't need to write it back because we already wrote it back just above
                if let Some(item) = self.cache.find(|item| item.block == key) {
                    item.data.copy_from_slice(&data);
                } else {
                    let value =
                        CacheEntry::from_vec(key, &data[i as usize * 512..(i + 1) as usize * 512], false);
                    if let Some(evicted_entry) = self.cache.insert(value) {
                        if evicted_entry.dirty {
                            self.write_blocks(key, 1, &evicted_entry.data);
                        }
                    }
                }
            }
        }

        self.cursor += num_bytes as u64;

        Ok(num_bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_cache();
        // We don't buffer anything ourselves so we don't need to flush
        Ok(())
    }
}

impl Drop for MassStorageDevice {
    fn drop(&mut self) {
        self.flush_cache();
        println!("Read Ops: {}", self.reads);
        println!("Read {} blocks", self.blocks_read);
        println!("Write Ops: {}", self.writes);
        println!("Wrote {} blocks", self.blocks_written);
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
