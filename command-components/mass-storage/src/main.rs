use std::{
    io::{self, Cursor},
    rc::Rc,
};

use fatfs::{Dir, FsOptions, ReadWriteSeek};
use mass_storage::{bulk_only::BulkOnlyTransportDevice, mass_storage::MassStorageDevice};
use tracing::{info, Level};
use usb_wasm_bindings::device::{UsbConfiguration, UsbDevice, UsbInterface};

use anyhow::anyhow;

fn human_readable_file_size(size_in_bytes: u64, decimal_places: usize) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];

    let mut size = size_in_bytes as f64;

    let mut i = 0;

    while i < (units.len() - 1) && size > 1024.0 {
        size /= 1024.0;
        i += 1;
    }

    format!("{:.1$} {2}", size, decimal_places, units[i])
}

fn build_fs_tree_<T: ReadWriteSeek>(
    dir: Dir<'_, T>,
    depth: usize,
) -> Result<Vec<String>, io::Error> {
    if depth > 1 {
        return Ok(vec![]);
    }

    let mut lines: Vec<String> = Vec::new();
    for entry in dir.iter() {
        let entry = entry?;
        lines.push(format!("{}|_{}", "  ".repeat(depth), entry.file_name()));
        if entry.is_dir() {
            lines.extend(build_fs_tree_(entry.to_dir(), depth + 1)?);
        }
    }
    Ok(lines)
}

pub fn build_fs_tree<T: ReadWriteSeek>(dir: Dir<'_, T>) -> Result<String, io::Error> {
    let lines = [vec!["\\.".to_string()], build_fs_tree_(dir, 0)?].concat();
    Ok(lines.join("\n"))
}

pub fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    // Find device
    let (interface, configuration, device) = {
        let mut mass_storage_interfaces: Vec<(
            Rc<UsbInterface>,
            Rc<UsbConfiguration>,
            Rc<UsbDevice>,
        )> = Vec::new();
        for device in UsbDevice::enumerate().into_iter().map(Rc::new) {
            for configuration in device.configurations().into_iter().map(Rc::new) {
                for interface in configuration.interfaces().into_iter().map(Rc::new) {
                    let if_descriptor = interface.descriptor();
                    if if_descriptor.interface_class == 0x08
                        && if_descriptor.interface_protocol == 0x50
                    {
                        mass_storage_interfaces.push((
                            interface,
                            configuration.clone(),
                            device.clone(),
                        ));
                    }
                }
            }
        }

        mass_storage_interfaces.iter().enumerate().for_each(|(i, (interface,configuration,device))| {
            let device_descriptor = device.descriptor();
            let configuration_descriptor = configuration.descriptor();
            let if_descriptor = interface.descriptor();
            info!("{}. USB Mass Storage Device Bulk Only Transport found: device {:04x}:{:04x} ({} {}), configuration {}, interface {}", i, device_descriptor.vendor_id, device_descriptor.product_id,  device_descriptor.manufacturer_name.unwrap_or_default(), device_descriptor.product_name.unwrap_or_default(), configuration_descriptor.number, if_descriptor.interface_number);
        });

        if mass_storage_interfaces.is_empty() {
            return Err(anyhow!("No mass storage devices found. Exiting."));
        }

        info!("Using first device.");
        mass_storage_interfaces.swap_remove(0)
    };
    let device = Rc::try_unwrap(device).unwrap();
    let configuration = Rc::try_unwrap(configuration).unwrap();
    let interface = Rc::try_unwrap(interface).unwrap();

    let bulk_only_transport = BulkOnlyTransportDevice::new(device, configuration, interface);
    let mut msd = MassStorageDevice::new(bulk_only_transport)?;
    let properties = msd.get_properties();

    info!("Device name: {}", properties.name);
    info!(
        "Capacity: {}",
        human_readable_file_size(properties.capacity, 2),
    );
    info!("Block size: {}B", properties.block_size);

    let mut buf_stream = fscommon::BufStream::new(&mut msd);
    // let mut buf_stream = msd;

    let mbr = mbrman::MBR::read_from(&mut buf_stream, 512)?;

    info!("Disk signature: {:?}", mbr.header.disk_signature);

    for (i, p) in mbr.iter() {
        // NOTE: The first four partitions are always provided by iter()
        if p.is_used() {
            info!(
                "Partition #{}: type = {:?}, size = {} bytes, starting lba = {}",
                i,
                p.sys,
                p.sectors as u64 * mbr.sector_size as u64,
                p.starting_lba
            );
        }
    }

    let (partition_start_address, partition_end_address) =
        if let Some((_, partition)) = mbr.iter().next() {
            (
                partition.starting_lba as u64 * mbr.sector_size as u64,
                (partition.starting_lba + partition.sectors) as u64 * mbr.sector_size as u64,
            )
        } else {
            return Err(anyhow!("No partition found"));
        };

    println!(
        "partition_start_address: {}, partition_end_address: {}",
        partition_start_address, partition_end_address
    );

    let fat_slice =
        fscommon::StreamSlice::new(buf_stream, partition_start_address, partition_end_address)?;
    let fs = fatfs::FileSystem::new(fat_slice, fatfs::FsOptions::new())?;

    let fs_tree = build_fs_tree(fs.root_dir())?;
    println!("{}", fs_tree);

    // println!("{:?}", fs.stats()?);

    Ok(())
}
