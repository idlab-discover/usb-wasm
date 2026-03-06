use std::io::{Read, Write, Seek, SeekFrom};
use std::time::Duration;

use rusb::{
    Context, Device, DeviceDescriptor, DeviceHandle, DeviceList, Direction, TransferType,
    UsbContext,
};

#[derive(Debug)]
struct Endpoint {
    config: u8,
    iface: u8,
    setting: u8,
    address: u8,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Rusb error: {0}")]
    Rusb(#[from] rusb::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("MBR error: {0}")]
    Mbr(#[from] mbrman::Error),
    #[error("FATFS error: {0}")]
    FatFs(String),
    #[error("No Mass Storage device found")]
    NoDevice,
    #[error("No readable endpoint found")]
    NoEndpoint,
    #[error("SCSI command failed")]
    ScsiError,
    #[error("Partition not found")]
    PartitionNotFound,
    #[error("File not found")]
    FileNotFound,
}

#[no_mangle]
#[export_name = "cabi_realloc"]
pub unsafe extern "C" fn cabi_realloc(
    old_ptr: *mut u8,
    old_size: usize,
    _align: usize,
    new_size: usize,
) -> *mut u8 {
    if new_size == 0 {
        return std::ptr::null_mut();
    }
    if old_ptr.is_null() {
        std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(new_size, _align))
    } else {
        std::alloc::realloc(
            old_ptr,
            std::alloc::Layout::from_size_align_unchecked(old_size, _align),
            new_size,
        )
    }
}

pub extern "C" fn exports_wasi_cli_run_run() -> bool {
    let _ = cabi_realloc as usize;
    println!("Starting rusb workload (WASI-USB) - Reading file from USB...");

    match run_usb_workload() {
        Ok(_) => {
            println!("Workload finished successfully.");
            true
        }
        Err(e) => {
            eprintln!("Workload failed: {:?}", e);
            false
        }
    }
}

fn main() {
    println!("Starting rusb workload (Native) - Reading file from USB...");
    if let Err(e) = run_usb_workload() {
        eprintln!("Workload failed: {:?}", e);
        std::process::exit(1);
    }
    println!("Workload finished successfully.");
}

fn run_usb_workload() -> std::result::Result<(), Error> {
    let context = Context::new()?;
    let list = DeviceList::new_with_context(context)?;

    println!("Devices found: {}", list.len());

    let mut target_device: Option<(Device<Context>, DeviceDescriptor, u8)> = None;

    for device in list.iter() {
        let device_desc = device.device_descriptor()?;

        println!(
            "Device {:04x}:{:04x} (bus {}, device {})",
            device_desc.vendor_id(),
            device_desc.product_id(),
            device.bus_number(),
            device.address()
        );

        if target_device.is_none() {
            if let Some(config) = find_mass_storage_config(&device, &device_desc) {
                println!(
                    " -> Found Mass Storage Device! (Config {})",
                    config.number()
                );
                target_device = Some((device, device_desc, config.number()));
            }
        }
    }

    if let Some((mut device, device_desc, config_value)) = target_device {
        println!(
            "\nAttempting to read from Mass Storage device {:04x}:{:04x}...",
            device_desc.vendor_id(),
            device_desc.product_id()
        );
        let handle = device.open()?;
        
        // Find endpoints
        let (in_ep, out_ep, iface) = find_bulk_endpoints(&mut device, config_value)
            .ok_or(Error::NoEndpoint)?;
        
        println!("Endpoints found: IN=0x{:02x}, OUT=0x{:02x}, Iface={}", in_ep, out_ep, iface);

        let _has_kernel_driver = match handle.kernel_driver_active(iface) {
            Ok(true) => {
                handle.detach_kernel_driver(iface).ok();
                true
            }
            _ => false,
        };

        handle.set_active_configuration(config_value)?;
        handle.claim_interface(iface)?;

        // Initialize Mass Storage Driver
        let mut storage = MassStorageDevice::new(handle, in_ep, out_ep, iface)?;
        
        // Reset recovery just in case
        // storage.reset_recovery()?;

        // Read MBR
        println!("Reading MBR...");
        // The MBR is in the first sector (LBA 0). We can read it using the storage device as a BlockDevice.
        // However, fatfs::FileSystem::new wants a stream that represents the partition.
        // So we first need to find the partition start.
        
        // mbrman needs a Read + Seek + Write. Our MassStorageDevice implements Read + Seek + Write.
        let mbr = mbrman::MBR::read_from(&mut storage, 512)?;
        
        println!("MBR read successfully.");
        
        // mbrman iter returns (index, entry)
        let partition = mbr.iter().find(|(_, p)| !p.is_unused()).ok_or(Error::PartitionNotFound)?;
        let start_lba = partition.1.starting_lba;
        let num_sectors = partition.1.sectors;
        
        println!("Found partition at LBA {} with {} sectors", start_lba, num_sectors);

        // Create a partition view
        // Note: fscommon 0.1.0 returns Result, 0.1.1 returns type directly. 
        // We handle Result just in case (map err to Io) or just try ? if it returns IoResult.
        // The previous error indicated it returns Result, so we add ?.
        let partition_stream = fscommon::StreamSlice::new(storage, start_lba as u64 * 512, (start_lba as u64 + num_sectors as u64) * 512)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        println!("Mounting filesystem...");
        let fs = fatfs::FileSystem::new(partition_stream, fatfs::FsOptions::new())
            .map_err(|e| Error::FatFs(format!("{:?}", e)))?;
        
        let root_dir = fs.root_dir();
        println!("---------------------------------------------------");
        
        println!("\n--- Optional Write Test ---");
        println!("Creating 'test.txt' and writing data...");
        let mut test_file = root_dir.create_file("test.txt")
            .map_err(|e| Error::FatFs(format!("{:?}", e)))?;
        let test_data = "WASI-USB Write Success! Timestamp: 2026-02-20";
        test_file.write_all(test_data.as_bytes())?;
        test_file.flush()?;
        println!("Write successful.");

        println!("Verifying 'test.txt'...");
        let mut verify_file = root_dir.open_file("test.txt")
            .map_err(|e| Error::FatFs(format!("{:?}", e)))?;
        let mut verify_content = String::new();
        verify_file.read_to_string(&mut verify_content)?;
        println!("Read back: '{}'", verify_content);

        if verify_content == test_data {
            println!("SUCCESS: Write/Read verification matched!");
        } else {
            println!("FAILURE: Data mismatch!");
        }
        println!("---------------------------------------------------");

        // Cleanup
        // We need to retrieve the handle from the storage/stream wrapper to re-attach kernel driver if needed
        // But due to ownership this is tricky. For this workload, we can skip re-attaching or implement a way to get it back.
        // For simplicity, we just drop it and let the OS handle it (or not).
    } else {
        println!("\nNo Mass Storage device found.");
        return Err(Error::NoDevice);
    }

    Ok(())
}

fn find_mass_storage_config<T: UsbContext>(
    device: &Device<T>,
    device_desc: &DeviceDescriptor,
) -> Option<rusb::ConfigDescriptor> {
    for n in 0..device_desc.num_configurations() {
        let config_desc = match device.config_descriptor(n) {
            Ok(c) => c,
            Err(_) => continue,
        };

        for interface in config_desc.interfaces() {
            for interface_desc in interface.descriptors() {
                if interface_desc.class_code() == 0x08 { // Mass Storage
                    return Some(config_desc);
                }
            }
        }
    }
    None
}

fn find_bulk_endpoints<T: UsbContext>(
    device: &mut Device<T>,
    config_value: u8,
) -> Option<(u8, u8, u8)> {
    let device_desc = device.device_descriptor().ok()?;

    for n in 0..device_desc.num_configurations() {
        let config_desc = device.config_descriptor(n).ok()?;
        if config_desc.number() != config_value {
            continue;
        }

        for interface in config_desc.interfaces() {
            for interface_desc in interface.descriptors() {
                if interface_desc.class_code() != 0x08 {
                    continue;
                }

                let mut in_ep = None;
                let mut out_ep = None;

                for endpoint_desc in interface_desc.endpoint_descriptors() {
                    if endpoint_desc.transfer_type() == TransferType::Bulk {
                        if endpoint_desc.direction() == Direction::In {
                            in_ep = Some(endpoint_desc.address());
                        } else {
                            out_ep = Some(endpoint_desc.address());
                        }
                    }
                }

                if let (Some(in_ep), Some(out_ep)) = (in_ep, out_ep) {
                    return Some((in_ep, out_ep, interface_desc.interface_number()));
                }
            }
        }
    }
    None
}

// Mass Storage Driver Implementation

// Command Block Wrapper (CBW)
#[repr(C, packed)]
struct Cbw {
    signature: [u8; 4],
    tag: u32,
    data_transfer_length: u32,
    flags: u8,
    lun: u8,
    cb_length: u8,
    cb: [u8; 16],
}

// Command Status Wrapper (CSW)
#[repr(C, packed)]
struct Csw {
    signature: [u8; 4],
    tag: u32,
    data_residue: u32,
    status: u8,
}

struct MassStorageDevice {
    handle: DeviceHandle<Context>,
    in_ep: u8,
    out_ep: u8,
    iface: u8, // Not used directly in Read/Write/Seek but kept for context
    tag: u32,
    position: u64,
    size: u64,
}

impl MassStorageDevice {
    fn new(handle: DeviceHandle<Context>, in_ep: u8, out_ep: u8, iface: u8) -> Result<Self, Error> {
        let mut device = Self {
            handle,
            in_ep,
            out_ep,
            iface,
            tag: 1,
            position: 0,
            size: 0,
        };
        
        // Perform Reset Recovery to ensure device is in a good state
        if let Err(e) = device.reset_recovery() {
            println!("Warning: Reset Recovery failed: {:?}", e);
            // We continue anyway, as it might work without it or the error might be benign on some controllers
        }
        
        device.read_capacity()?;
        Ok(device)
    }

    fn reset_recovery(&mut self) -> Result<(), Error> {
        println!("Performing Mass Storage Reset Recovery...");
        
        // 1. Bulk-Only Mass Storage Reset
        // Class-specific request to interface (0x21)
        // Request: 0xFF
        // Value: 0
        // Index: Interface Number
        // Length: 0
        let request_type = rusb::request_type(Direction::Out, rusb::RequestType::Class, rusb::Recipient::Interface);
        let timeout = Duration::from_secs(1);
        
        match self.handle.write_control(request_type, 0xFF, 0, self.iface as u16, &[], timeout) {
            Ok(_) => println!("  Mass Storage Reset sent."),
            Err(e) => println!("  Mass Storage Reset failed: {:?}", e),
        }

        // 2. Clear Halt on Bulk-In Endpoint
        match self.handle.clear_halt(self.in_ep) {
            Ok(_) => println!("  Clear Halt (IN) success."),
            Err(e) => println!("  Clear Halt (IN) failed: {:?}", e),
        }

        // 3. Clear Halt on Bulk-Out Endpoint
        match self.handle.clear_halt(self.out_ep) {
            Ok(_) => println!("  Clear Halt (OUT) success."),
            Err(e) => println!("  Clear Halt (OUT) failed: {:?}", e),
        }
        
        Ok(())
    }

    fn read_capacity(&mut self) -> Result<(), Error> {
        let mut cb = [0u8; 16];
        cb[0] = 0x25; // READ CAPACITY (10)

        // Data length is 8 bytes
        self.send_scsi_command(&cb, 10, 8, true)?;

        let mut buf = [0u8; 8];
        let timeout = Duration::from_secs(5);
        let mut total_read = 0;
        while total_read < 8 {
             match self.handle.read_bulk(self.in_ep, &mut buf[total_read..], timeout) {
                Ok(len) => total_read += len,
                Err(rusb::Error::Timeout) => continue,
                Err(e) => return Err(Error::Rusb(e)),
            }
        }
        
        self.read_csw()?;

        let last_lba = u32::from_be_bytes(buf[0..4].try_into().unwrap());
        let block_size = u32::from_be_bytes(buf[4..8].try_into().unwrap());
        
        self.size = (last_lba as u64 + 1) * block_size as u64;
        println!("Device Capacity: {} bytes ({} blocks of {})", self.size, last_lba + 1, block_size);

        Ok(())
    }

    fn read_sectors(&mut self, lba: u64, sectors: u16, buf: &mut [u8]) -> std::io::Result<usize> {
        let block_size = 512;
        let expected_bytes = sectors as u32 * block_size;
        
        if buf.len() < expected_bytes as usize {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Buffer too small"));
        }

        // Prepare SCSI READ(10) command
        let mut cb = [0u8; 16];
        cb[0] = 0x28; // READ(10) opcode
        cb[2] = (lba >> 24) as u8;
        cb[3] = (lba >> 16) as u8;
        cb[4] = (lba >> 8) as u8;
        cb[5] = lba as u8;
        cb[7] = (sectors >> 8) as u8;
        cb[8] = sectors as u8;

        self.send_scsi_command(&cb, 10, expected_bytes, true)?;

        // Read data
        let timeout = Duration::from_secs(10);
        let mut total_read = 0;
        while total_read < expected_bytes as usize {
            match self.handle.read_bulk(self.in_ep, &mut buf[total_read..], timeout) {
                Ok(len) => total_read += len,
                Err(rusb::Error::Timeout) => {
                     println!("Timeout reading data, retrying...");
                     continue;
                },
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("USB Read Error: {}", e))),
            }
        }

        // Read CSW
        self.read_csw()?;

        Ok(total_read)
    }

    fn write_sectors(&mut self, lba: u64, sectors: u16, buf: &[u8]) -> std::io::Result<usize> {
        let block_size = 512;
        let expected_bytes = sectors as u32 * block_size;
        
        if buf.len() < expected_bytes as usize {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Buffer too small"));
        }

        // Prepare SCSI WRITE(10) command
        let mut cb = [0u8; 16];
        cb[0] = 0x2A; // WRITE(10) opcode
        cb[2] = (lba >> 24) as u8;
        cb[3] = (lba >> 16) as u8;
        cb[4] = (lba >> 8) as u8;
        cb[5] = lba as u8;
        cb[7] = (sectors >> 8) as u8;
        cb[8] = sectors as u8;

        self.send_scsi_command(&cb, 10, expected_bytes, false)?;

        // Write data
        let timeout = Duration::from_secs(10);
        let mut total_written = 0;
        while total_written < expected_bytes as usize {
            match self.handle.write_bulk(self.out_ep, &buf[total_written..], timeout) {
                Ok(len) => total_written += len,
                Err(rusb::Error::Timeout) => {
                     println!("Timeout writing data, retrying...");
                     continue;
                },
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("USB Write Error: {}", e))),
            }
        }

        // Read CSW
        self.read_csw()?;

        Ok(total_written)
    }

    fn send_scsi_command(&mut self, cb: &[u8], cb_len: u8, data_len: u32, direction_in: bool) -> std::io::Result<()> {
        let mut cbw = Cbw {
            signature: [0x55, 0x53, 0x42, 0x43], // "USBC"
            tag: self.tag,
            data_transfer_length: data_len,
            flags: if direction_in { 0x80 } else { 0x00 },
            lun: 0,
            cb_length: cb_len,
            cb: [0; 16],
        };
        cbw.cb[..cb.len()].copy_from_slice(cb);

        let cbw_bytes = unsafe {
            std::slice::from_raw_parts(&cbw as *const _ as *const u8, std::mem::size_of::<Cbw>())
        };

        let timeout = Duration::from_secs(5);
        match self.handle.write_bulk(self.out_ep, cbw_bytes, timeout) {
            Ok(_) => Ok(()),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to send CBW: {}", e))),
        }
    }

    fn read_csw(&mut self) -> std::io::Result<()> {
        let mut buf = [0u8; 13];
        let timeout = Duration::from_secs(5);
        match self.handle.read_bulk(self.in_ep, &mut buf, timeout) {
            Ok(13) => {
                let signature = &buf[0..4];
                if signature != [0x55, 0x53, 0x42, 0x53] { // "USBS"
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CSW Signature"));
                }
                let status = buf[12];
                if status != 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("SCSI Command Failed (Status {})", status)));
                }
                self.tag += 1;
                Ok(())
            }
            Ok(_) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid CSW Length")),
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read CSW: {}", e))),
        }
    }
}

// Implement Read, Write, Seek for MassStorageDevice so it can be used by fatfs and mbrman via adapters
// NOTE: MassStorageDevice is block-based (512 bytes). Random access byte-level IO is inefficient but functional for this demo.

impl Read for MassStorageDevice {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let sector_size = 512;
        let start_sector = self.position / sector_size;
        let offset_in_sector = (self.position % sector_size) as usize;
        let mut sectors_to_read = (buf.len() + offset_in_sector + sector_size as usize - 1) / sector_size as usize;
        
        // Limit max sectors to avoid too huge buffers
        if sectors_to_read > 256 {
            sectors_to_read = 256;
        }

        let mut temp_buf = vec![0u8; sectors_to_read * sector_size as usize];
        
        self.read_sectors(start_sector, sectors_to_read as u16, &mut temp_buf)?;
        
        let available_bytes = temp_buf.len() - offset_in_sector;
        let bytes_to_copy = std::cmp::min(available_bytes, buf.len());
        
        buf[..bytes_to_copy].copy_from_slice(&temp_buf[offset_in_sector..offset_in_sector + bytes_to_copy]);
        
        self.position += bytes_to_copy as u64;
        Ok(bytes_to_copy)
    }
}

impl Seek for MassStorageDevice {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match pos {
            SeekFrom::Start(p) => self.position = p,
            SeekFrom::Current(p) => {
                let new_pos = self.position as i64 + p;
                if new_pos < 0 {
                     return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Seek to negative"));
                }
                self.position = new_pos as u64;
            },
            SeekFrom::End(p) => {
                 let new_pos = self.size as i64 + p;
                 if new_pos < 0 {
                     return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Seek to negative"));
                 }
                 self.position = new_pos as u64;
            },
        }
        Ok(self.position)
    }
}

impl std::io::Write for MassStorageDevice {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let sector_size = 512;
        let start_sector = self.position / sector_size;
        let offset_in_sector = (self.position % sector_size) as usize;

        if offset_in_sector == 0 && buf.len() >= sector_size as usize {
            // Fast path: Align write
            let sectors_to_write = buf.len() / sector_size as usize;
            let bytes_to_write = sectors_to_write * sector_size as usize;
            self.write_sectors(start_sector, sectors_to_write as u16, &buf[..bytes_to_write])?;
            self.position += bytes_to_write as u64;
            Ok(bytes_to_write)
        } else {
            // Read-Modify-Write slow path for unaligned or small writes
            let mut temp_buf = vec![0u8; sector_size as usize];
            self.read_sectors(start_sector, 1, &mut temp_buf)?;
            
            let bytes_to_copy = std::cmp::min(buf.len(), sector_size as usize - offset_in_sector);
            temp_buf[offset_in_sector..offset_in_sector + bytes_to_copy].copy_from_slice(&buf[..bytes_to_copy]);
            
            self.write_sectors(start_sector, 1, &temp_buf)?;
            self.position += bytes_to_copy as u64;
            Ok(bytes_to_copy)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
