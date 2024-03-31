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

    while i < (units.len() - 1) && size >= 1024.0 {
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
    let msd = {
        let mut mass_storage_devices: Vec<MassStorageDevice> = Vec::new();
        for device in UsbDevice::enumerate().into_iter() {
            let configuration = device.configurations().remove(0);
            let interface = configuration.interfaces().into_iter().find(|interface| {
                let if_descriptor = interface.descriptor();
                if if_descriptor.interface_class == 0x08 && if_descriptor.interface_protocol == 0x50
                {
                    true
                } else {
                    false
                }
            });
            if let Some(interface) = interface {
                let bulk_only_transport =
                    BulkOnlyTransportDevice::new(device, configuration, interface);
                mass_storage_devices.push(MassStorageDevice::new(bulk_only_transport).unwrap());
            }
        }

        if mass_storage_devices.is_empty() {
            return Err(anyhow!("No mass storage devices found. Exiting."));
        }

        if mass_storage_devices.len() == 1 {
            mass_storage_devices.remove(0)
        } else {
            let mut input = String::new();
            println!("Please select a device:");
            for (i, msd) in mass_storage_devices.iter().enumerate() {
                let properties = msd.get_properties();

                println!(
                    "{}. {} ({})",
                    i,
                    properties.name,
                    human_readable_file_size(properties.capacity, 2)
                );
            }
            io::stdin().read_line(&mut input)?;
            let i: usize = input.trim().parse()?;
            mass_storage_devices.remove(i)
        }
    };
    {
        let properties = msd.get_properties();
        info!(
            "Selected: {} ({})",
            properties.name,
            human_readable_file_size(properties.capacity, 2)
        );
    }
    Ok(msd)
}

fn benchmark_raw_speed(
    test_count: usize,
    seq_test_size_mb: usize,
    rnd_test_size_mb: usize,
) -> anyhow::Result<()> {
    let mut msd = get_mass_storage_device()?;
    let properties = msd.get_properties();

    let num_blocks_range = [512, 1024, 2048, 4096];
    // let num_blocks_range = [512];
    // let num_blocks_range = [4096];

    let mut rng = rand::thread_rng();

    let seq_test_size = seq_test_size_mb * 1024 * 1024;
    let rnd_test_size = seq_test_size_mb * 1024 * 1024;

    struct Report {
        num_blocks: u32,
        sequential_write_speed: f64,
        sequential_read_speed: f64,
        random_write_speed: f64,
        random_read_speed: f64,
    }

    info!("Starting benchmark");
    info!(
        "Seq Test Data: {}, Rnd Test Data: {}, ",
        human_readable_file_size(seq_test_size_mb as u64 * 1024 * 1024, 2),
        human_readable_file_size(rnd_test_size_mb as u64 * 1024 * 1024, 2),
    );
    for num_blocks in num_blocks_range {
        let mut report = Report {
            num_blocks,
            sequential_write_speed: 0.0,
            sequential_read_speed: 0.0,
            random_write_speed: 0.0,
            random_read_speed: 0.0,
        };

        let seq_num_repetitions: u32 =
            ((seq_test_size / (num_blocks * 512) as usize) as u32).max(1);

        // Benchmark SEQUENTIAL reads and writes:
        for _ in 0..test_count {
            let mut data = vec![0_u8; num_blocks as usize * 512];
            data[..].try_fill(&mut rng)?;

            let address = 0;
            // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * num_blocks);
            let start_write = std::time::Instant::now();
            for i in 0..seq_num_repetitions {
                msd.write_blocks(address + i * num_blocks, num_blocks as u16, &data);
            }
            let end_write = std::time::Instant::now();
            let write_time = end_write - start_write;
            report.sequential_write_speed += seq_test_size as f64 / write_time.as_secs_f64();
        }
        report.sequential_write_speed /= test_count as f64;

        for _ in 0..test_count {
            let address = 0;
            // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * num_blocks);
            let start_read = std::time::Instant::now();
            for i in 0..seq_num_repetitions {
                msd.read_blocks(address + i * num_blocks, num_blocks as u16);
            }
            let end_read = std::time::Instant::now();
            let read_time = end_read - start_read;
            report.sequential_read_speed += seq_test_size as f64 / read_time.as_secs_f64();
        }
        report.sequential_read_speed /= test_count as f64;

        let rnd_num_repetitions: u32 =
            ((rnd_test_size / (num_blocks * 512) as usize) as u32).max(1);
        // Benchmark RANDOM reads and writes:
        for _ in 0..test_count {
            let mut data = vec![0_u8; num_blocks as usize * 512];
            data[..].try_fill(&mut rng)?;

            let addresses: Vec<u32> = (0..rnd_num_repetitions)
                .map(|_| rng.gen_range(0..properties.total_number_of_blocks - num_blocks))
                .collect();
            let start_write = std::time::Instant::now();
            for address in addresses {
                msd.write_blocks(address, num_blocks as u16, &data);
            }
            let end_write = std::time::Instant::now();
            let write_time = end_write - start_write;
            report.random_write_speed += seq_test_size as f64 / write_time.as_secs_f64();
        }
        report.random_write_speed /= test_count as f64;

        for _ in 0..test_count {
            let addresses: Vec<u32> = (0..rnd_num_repetitions)
                .map(|_| rng.gen_range(0..properties.total_number_of_blocks - num_blocks))
                .collect();
            let start_read = std::time::Instant::now();
            for address in addresses {
                msd.read_blocks(address, num_blocks as u16);
            }
            let end_read = std::time::Instant::now();
            let read_time = end_read - start_read;
            report.random_read_speed += seq_test_size as f64 / read_time.as_secs_f64();
        }
        report.random_read_speed /= test_count as f64;

        info!(
            "Blocks: {} ({}): SEQ WRITE: {}/s, SEQ READ: {}/s, RND WRITE: {}/s, RND READ: {}/s",
            report.num_blocks,
            human_readable_file_size(report.num_blocks as u64 * 512, 2),
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

    benchmark_raw_speed(2, 8, 1)?;

    // ls(fat_slice)?;
    // cat(fat_slice)?;
    // write(fat_slice, "hello.txt", b"Hello USB!\n")?;
    // write(fat_slice, "hello2.txt", b"Hello USB2!\n")?;

    Ok(())
}
