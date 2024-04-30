use std::io::{self, Read, Seek, Write};

use bulk_only::BulkOnlyTransportDevice;

use chrono::{DateTime, Local};
use fatfs::{Dir, FileSystem, FsOptions, ReadWriteSeek};
use mass_storage::MassStorageDevice;
use rand::{Fill, Rng};
use tracing::{debug, info};
#[cfg(target_arch = "wasm32")]
use usb_wasm_bindings::device::UsbDevice;

use anyhow::anyhow;

pub mod bulk_only;
pub mod mass_storage;

pub fn tree(path: Option<String>) -> anyhow::Result<()> {
    fn _tree(dir: Dir<'_, impl ReadWriteSeek>, depth: usize) -> Result<Vec<String>, io::Error> {
        debug!(depth, "build_fs_tree_");
        if depth > 10 {
            return Ok(vec![]);
        }

        let mut lines: Vec<String> = Vec::new();
        for entry in dir.iter() {
            let entry = entry.unwrap();
            if entry.file_name().starts_with('.') {
                continue;
            }
            lines.push(format!("{}|_ {}", "  ".repeat(depth), entry.file_name()));
            if entry.is_dir() {
                lines.extend(_tree(entry.to_dir(), depth + 1)?);
            }
        }
        Ok(lines)
    }

    let fs = get_filesystem()?;
    let root_dir = fs.root_dir();
    let dir = match path {
        None => root_dir,
        Some(ref path) if path == "." => root_dir,
        Some(ref path) => root_dir.open_dir(path)?,
    };

    let lines = [vec!["\\.".to_string()], _tree(dir, 0)?].concat();
    println!("{}", lines.join("\n"));
    Ok(())
}

pub fn ls(dir: Option<String>) -> anyhow::Result<()> {
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

    let fs = get_filesystem()?;
    let root_dir = fs.root_dir();
    let dir = match dir {
        None => root_dir,
        Some(ref path) if path == "." => root_dir,
        Some(ref path) => root_dir.open_dir(path)?,
    };
    for r in dir.iter() {
        let e = r?;
        let modified = DateTime::<Local>::from(e.modified())
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

pub fn cat(file: String) -> anyhow::Result<()> {
    let fs = get_filesystem()?;
    let root_dir = fs.root_dir();
    let mut file = root_dir.open_file(&file)?;
    let mut buf = vec![];
    file.read_to_end(&mut buf)?;
    print!("{}", String::from_utf8_lossy(&buf));
    Ok(())
}

pub fn write(path: &str, contents: &[u8]) -> anyhow::Result<()> {
    let fs = get_filesystem()?;
    let mut file = fs.root_dir().create_file(path)?;
    file.truncate()?;
    file.write_all(contents)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn get_mass_storage_device() -> anyhow::Result<MassStorageDevice> {
    // Find device
    let msd = {
        let mut mass_storage_devices: Vec<MassStorageDevice> = Vec::new();
        for device in UsbDevice::enumerate().into_iter() {
            let configuration = device.configurations().remove(0);
            let interface = configuration.interfaces().into_iter().find(|interface| {
                let if_descriptor = interface.descriptor();
                if_descriptor.interface_class == 0x08 && if_descriptor.interface_protocol == 0x50
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

#[cfg(not(target_arch = "wasm32"))]
fn get_mass_storage_device() -> anyhow::Result<MassStorageDevice> {
    // Find device

    use rusb::UsbContext;
    let msd = {
        let mut mass_storage_devices: Vec<MassStorageDevice> = Vec::new();

        for device in rusb::GlobalContext::default().devices()?.iter() {
            let configuration = device.config_descriptor(0)?;
            let interface = configuration.interfaces().find(|interface| {
                let if_descriptor = interface.descriptors().next().unwrap();
                if_descriptor.class_code() == 0x08 && if_descriptor.protocol_code() == 0x50
            });
            if let Some(interface) = interface {
                let bulk_only_transport =
                    BulkOnlyTransportDevice::new(device, 0, interface.number());
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

fn get_filesystem() -> anyhow::Result<FileSystem<impl ReadWriteSeek>> {
    let mut msd = get_mass_storage_device().unwrap();
    // let mut msd =
    //     BufStream::with_capacities(24576, 24576, get_mass_storage_device().unwrap());
    let mbr = mbrman::MBR::read_from(&mut msd, 512)?;
    let (_, partition) = mbr.iter().next().ok_or(anyhow!("No partition found"))?;
    let starting_lba = partition.starting_lba;
    let sectors = partition.sectors;
    let sector_size = mbr.sector_size;

    println!("starting_lba: {}", starting_lba);
    println!("sectors: {}", sectors);

    let fat_slice = fscommon::StreamSlice::new(
        msd,
        (starting_lba * sector_size).into(),
        (starting_lba + sectors) as u64 * sector_size as u64,
    )
    .unwrap();

    debug!("Initialized Filesystem");
    Ok(FileSystem::new(fat_slice, FsOptions::new())?)
}

// WARNING: This will probably break your filesystem, as this function just writes random blocks to the device
// Breaks the USB when test_count > 1 for some reason?
pub fn benchmark_raw_speed(
    test_count: usize,
    seq_test_size_mb: usize,
    rnd_test_size_mb: usize,
) -> anyhow::Result<()> {
    let mut msd = get_mass_storage_device()?;
    let properties = msd.get_properties();

    let mut rng = rand::thread_rng();

    let seq_test_size = seq_test_size_mb * 1024 * 1024;
    let rnd_test_size = seq_test_size_mb * 1024 * 1024;

    struct Report {
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
    const NUM_BLOCKS: u32 = 2048;
    let mut report = Report {
        sequential_write_speed: 0.0,
        sequential_read_speed: 0.0,
        random_write_speed: 0.0,
        random_read_speed: 0.0,
    };

    let seq_num_repetitions: u32 = ((seq_test_size / (NUM_BLOCKS * 512) as usize) as u32).max(1);

    // Benchmark SEQUENTIAL reads and writes:
    for _ in 0..test_count {
        let mut data = vec![0_u8; NUM_BLOCKS as usize * 512];
        data[..].try_fill(&mut rng)?;

        let address = 8192;
        // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * NUM_BLOCKS);
        let start_write = std::time::Instant::now();
        for i in 0..seq_num_repetitions {
            msd.write_blocks(address + i * NUM_BLOCKS, NUM_BLOCKS as u16, &data);
        }
        let end_write = std::time::Instant::now();
        let write_time = end_write - start_write;
        report.sequential_write_speed += seq_test_size as f64 / write_time.as_secs_f64();
    }
    report.sequential_write_speed /= test_count as f64;

    for _ in 0..test_count {
        let address = 8192;
        // rng.gen_range(0..properties.total_number_of_blocks - NUM_REPETITIONS * NUM_BLOCKS);
        let start_read = std::time::Instant::now();
        for i in 0..seq_num_repetitions {
            msd.read_blocks(address + i * NUM_BLOCKS, NUM_BLOCKS as u16);
        }
        let end_read = std::time::Instant::now();
        let read_time = end_read - start_read;
        report.sequential_read_speed += seq_test_size as f64 / read_time.as_secs_f64();
    }
    report.sequential_read_speed /= test_count as f64;

    let rnd_num_repetitions: u32 = ((rnd_test_size / (NUM_BLOCKS * 512) as usize) as u32).max(1);
    // Benchmark RANDOM reads and writes:
    for _ in 0..test_count {
        let mut data = vec![0_u8; NUM_BLOCKS as usize * 512];
        data[..].try_fill(&mut rng)?;

        let addresses: Vec<u32> = (0..rnd_num_repetitions)
            .map(|_| rng.gen_range(8192..properties.total_number_of_blocks - NUM_BLOCKS))
            .collect();
        let start_write = std::time::Instant::now();
        for address in addresses {
            msd.write_blocks(address, NUM_BLOCKS as u16, &data);
        }
        let end_write = std::time::Instant::now();
        let write_time = end_write - start_write;
        report.random_write_speed += seq_test_size as f64 / write_time.as_secs_f64();
    }
    report.random_write_speed /= test_count as f64;

    for _ in 0..test_count {
        let addresses: Vec<u32> = (0..rnd_num_repetitions)
            .map(|_| rng.gen_range(8192..properties.total_number_of_blocks - NUM_BLOCKS))
            .collect();
        let start_read = std::time::Instant::now();
        for address in addresses {
            msd.read_blocks(address, NUM_BLOCKS as u16);
        }
        let end_read = std::time::Instant::now();
        let read_time = end_read - start_read;
        report.random_read_speed += seq_test_size as f64 / read_time.as_secs_f64();
    }
    report.random_read_speed /= test_count as f64;

    info!(
        "SEQ WRITE: {}/s, SEQ READ: {}/s, RND WRITE: {}/s, RND READ: {}/s",
        human_readable_file_size(report.sequential_write_speed as u64, 2),
        human_readable_file_size(report.sequential_read_speed as u64, 2),
        human_readable_file_size(report.random_write_speed as u64, 2),
        human_readable_file_size(report.random_read_speed as u64, 2),
    );

    // for report in reports {

    // }

    Ok(())
}

pub fn benchmark(seq_test_size_mb: usize) -> anyhow::Result<()> {
    let fs = get_filesystem()?;

    let root_dir = fs.root_dir();
    let mut temp_file = root_dir.create_file("temp.bin")?;
    temp_file.truncate()?;

    let mut rng = rand::thread_rng();

    let seq_test_size = seq_test_size_mb * 1024 * 1024;

    struct Report {
        sequential_write_speed: f64,
        sequential_read_speed: f64,
    }

    println!("Starting benchmark");
    println!(
        "Seq Test Data: {}",
        human_readable_file_size(seq_test_size as u64, 2),
    );

    let mut report = Report {
        sequential_write_speed: 0.0,
        sequential_read_speed: 0.0,
    };

    // Benchmark SEQUENTIAL reads and writes:
    {
        let mut data = vec![0_u8; seq_test_size];
        data[..].try_fill(&mut rng)?;

        temp_file.seek(io::SeekFrom::Start(0))?;
        let start_write = std::time::Instant::now();
        temp_file.write_all(&data)?;
        let end_write = std::time::Instant::now();
        let write_time = end_write - start_write;
        report.sequential_write_speed = seq_test_size as f64 / write_time.as_secs_f64();
    }

    {        
        let mut data = Vec::new();
        temp_file.seek(io::SeekFrom::Start(0))?;
        let start_read = std::time::Instant::now();
        temp_file.read_to_end(&mut data)?;
        let end_read = std::time::Instant::now();
        let read_time = end_read - start_read;
        report.sequential_read_speed = data.len() as f64 / read_time.as_secs_f64();
    }

    println!(
        "SEQ WRITE: {}/s, SEQ READ: {}/s",
        human_readable_file_size(report.sequential_write_speed as u64, 2),
        human_readable_file_size(report.sequential_read_speed as u64, 2),
    );
    temp_file.flush()?;
    std::mem::drop(temp_file);
    root_dir.remove("temp.bin")?;

    Ok(())
}

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
