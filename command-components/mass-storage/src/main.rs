use core::num;
use std::{
    env,
    io::{self, Cursor, Read, Seek, Write},
    rc::Rc,
};

use bytes::buf;
use chrono::NaiveDateTime;
use fat32::{fat, volume::Volume};
use fatfs::{
    format_volume, Dir, FileSystem, FormatVolumeOptions, FsOptions, OemCpConverter, ReadWriteSeek,
    StdIoWrapper, TimeProvider,
};
use mass_storage::{bulk_only::BulkOnlyTransportDevice, mass_storage::MassStorageDevice};
use rand::{Fill, Rng};
use tracing::{debug, error, info, info_span, Level};
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

fn build_fs_tree_<T: ReadWriteSeek, TP: TimeProvider, OCC: OemCpConverter>(
    dir: Dir<'_, T, TP, OCC>,
    depth: usize,
) -> Result<Vec<String>, io::Error> {
    debug!(depth, "build_fs_tree_");
    if depth > 10 {
        return Ok(vec![]);
    }

    let mut lines: Vec<String> = Vec::new();
    for entry in dir.iter() {
        let entry = entry.unwrap();
        if entry.file_name().starts_with(".") {
            continue;
        }
        lines.push(format!("{}|_ {}", "  ".repeat(depth), entry.file_name()));
        if entry.is_dir() {
            lines.extend(build_fs_tree_(entry.to_dir(), depth + 1)?);
        }
    }
    Ok(lines)
}

pub fn build_fs_tree<T: ReadWriteSeek, TP: TimeProvider, OCC: OemCpConverter>(
    dir: Dir<'_, T, TP, OCC>,
) -> Result<String, io::Error> {
    let lines = [vec!["\\.".to_string()], build_fs_tree_(dir, 0)?].concat();
    Ok(lines.join("\n"))
}

fn ls<RWS: Read + Write + Seek>(slice: RWS) -> anyhow::Result<()> {
    fn format_file_size(size: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;
        const GB: u64 = 1024 * MB;
        if size < KB {
            format!("{}B", size)
        } else if size < MB {
            format!("{}KB", size / KB)
        } else if size < GB {
            format!("{}MB", size / MB)
        } else {
            format!("{}GB", size / GB)
        }
    }

    let fs = FileSystem::new(slice, FsOptions::new())?;
    let root_dir = fs.root_dir();
    let dir = match env::args().nth(1) {
        None => root_dir,
        Some(ref path) if path == "." => root_dir,
        Some(ref path) => root_dir.open_dir(path)?,
    };
    for r in dir.iter() {
        let e = r?;
        let modified = NaiveDateTime::from(e.modified())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        println!(
            "{:4}  {}  {}",
            format_file_size(e.len()),
            modified,
            e.file_name()
        );
    }
    Ok(())
}

fn cat<RWS: Read + Write + Seek>(slice: RWS) -> anyhow::Result<()> {
    let fs = FileSystem::new(slice, FsOptions::new())?;
    let root_dir = fs.root_dir();
    let mut file = root_dir.open_file(&env::args().nth(1).expect("filename expected"))?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    print!("{}", String::from_utf8_lossy(&buf));
    Ok(())
}

fn write<RWS: Read + Write + Seek>(slice: RWS, path: &str, contents: &[u8]) -> anyhow::Result<()> {
    let options = FsOptions::new().update_accessed_date(true);
    let fs = FileSystem::new(slice, options)?;
    let mut file = fs.root_dir().create_file(path)?;
    file.truncate()?;
    file.write_all(contents)?;
    Ok(())
}

fn get_mass_storage_device() -> anyhow::Result<MassStorageDevice> {
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
    Ok(MassStorageDevice::new(bulk_only_transport)?)
}

fn benchmark_raw_speed() -> anyhow::Result<()> {
    let mut msd = get_mass_storage_device()?;
    let properties = msd.get_properties();

    let num_blocks_range = [32, 64, 128, 256, 512, 1024, 2048, 4096];

    let mut rng = rand::thread_rng();

    struct Report {
        num_blocks: u32,
        sequential_write_speed: f64,
        sequential_read_speed: f64,
        random_write_speed: f64,
        random_read_speed: f64,
    }

    info!("Starting benchmark");
    for num_blocks in num_blocks_range {
        let mut report = Report {
            num_blocks,
            sequential_write_speed: 0.0,
            sequential_read_speed: 0.0,
            random_write_speed: 0.0,
            random_read_speed: 0.0,
        };

        const NUM_REPETITIONS: u32 = 64;

        // Benchmark SEQUENTIAL reads and writes:
        {
            let mut data = vec![0_u8; num_blocks as usize * 512];
            data[..].try_fill(&mut rng)?;

            let address = 0;
            // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * num_blocks);
            let start_write = std::time::Instant::now();
            for i in 0..NUM_REPETITIONS {
                msd.write_blocks(address + i * num_blocks, num_blocks as u16, &data);
            }
            let end_write = std::time::Instant::now();
            let write_time = end_write - start_write;
            report.sequential_write_speed =
                (NUM_REPETITIONS as f64 * num_blocks as f64 * 512.0) / write_time.as_secs_f64();
        }

        {
            let address = 0;
            // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * num_blocks);
            let start_read = std::time::Instant::now();
            for i in 0..NUM_REPETITIONS {
                msd.read_blocks(address + i * num_blocks, num_blocks as u16);
            }
            let end_read = std::time::Instant::now();
            let read_time = end_read - start_read;
            report.sequential_read_speed =
                (NUM_REPETITIONS as f64 * num_blocks as f64 * 512.0) / read_time.as_secs_f64();
        }

        // Benchmark RANDOM reads and writes:
        // {
        //     let mut data = vec![0_u8; num_blocks as usize * 512];
        //     data[..].try_fill(&mut rng)?;

        //     let start_write = std::time::Instant::now();
        //     for _ in 0..NUM_REPETITIONS {
        //         let address = rng.gen_range(0..properties.total_number_of_blocks - num_blocks);
        //         msd.write_blocks(address, num_blocks as u16, &data);
        //     }
        //     let end_write = std::time::Instant::now();
        //     let write_time = end_write - start_write;
        //     report.random_write_speed =
        //         (NUM_REPETITIONS as f64 * num_blocks as f64 * 512.0) / write_time.as_secs_f64();
        // }

        // {
        //     let start_read = std::time::Instant::now();
        //     for _ in 0..NUM_REPETITIONS {
        //         let address = rng.gen_range(0..properties.total_number_of_blocks - num_blocks);
        //         msd.read_blocks(address, num_blocks as u16);
        //     }
        //     let end_read = std::time::Instant::now();
        //     let read_time = end_read - start_read;
        //     report.random_read_speed =
        //         (NUM_REPETITIONS as f64 * num_blocks as f64 * 512.0) / read_time.as_secs_f64();
        // }

        // reports.push(report);
        info!(
            "Blocks: {}, Sequential write speed: {}/s, Sequential read speed: {}/s, Random write speed: {}/s, Random read speed: {}/s",
            report.num_blocks,
            human_readable_file_size(report.sequential_write_speed as u64, 2),
            human_readable_file_size(report.sequential_read_speed as u64, 2),
            human_readable_file_size(report.random_write_speed as u64, 2),
            human_readable_file_size(report.random_read_speed as u64, 2),
        );
    }

    // for report in reports {

    // }

    Ok(())
}

pub fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    // let mut msd = get_mass_storage_device()?;
    // let properties = msd.get_properties();

    // info!("Device name: {}", properties.name);
    // info!(
    //     "Capacity: {}",
    //     human_readable_file_size(properties.capacity, 2),
    // );
    // info!("Block size: {}B", properties.block_size);

    // let mut buf_stream = fscommon::BufStream::new(&mut msd);

    // let mbr = mbrman::MBR::read_from(&mut msd, 512)?;

    // info!("Disk signature: {:?}", mbr.header.disk_signature);

    // for (i, p) in mbr.iter() {
    //     // NOTE: The first four partitions are always provided by iter()
    //     if p.is_used() {
    //         info!(
    //             "Partition #{}: type = {:?}, size = {} bytes, starting lba = {}",
    //             i,
    //             p.sys,
    //             p.sectors as u64 * mbr.sector_size as u64,
    //             p.starting_lba
    //         );
    //     }
    // }

    // let (partition_start_address, partition_end_address) =
    //     if let Some((_, partition)) = mbr.iter().next() {
    //         (
    //             partition.starting_lba as u64 * mbr.sector_size as u64,
    //             (partition.starting_lba + partition.sectors) as u64 * mbr.sector_size as u64,
    //         )
    //     } else {
    //         return Err(anyhow!("No partition found"));
    //     };

    // println!(
    //     "partition_start_address: {}, partition_end_address: {}",
    //     partition_start_address, partition_end_address
    // );

    // let (partition_start_lba, partition_end_lba) = if let Some((_, partition)) = mbr.iter().next() {
    //     (
    //         partition.starting_lba,
    //         partition.starting_lba + partition.sectors,
    //     )
    // } else {
    //     return Err(anyhow!("No partition found"));
    // };

    // println!(
    //     "partition_start_lba: {}, partition_end_lba: {}",
    //     partition_start_lba, partition_end_lba
    // );

    // let mut fat_slice =
    //     fscommon::StreamSlice::new(msd, partition_start_address, partition_end_address)?;

    benchmark_raw_speed()?;

    // ls(fat_slice)?;
    // cat(fat_slice)?;
    // write(fat_slice, "hello.txt", b"Hello USB!\n")?;
    // write(fat_slice, "hello2.txt", b"Hello USB2!\n")?;

    Ok(())
}
