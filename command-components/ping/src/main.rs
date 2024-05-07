use std::io::Write;
use std::{fs::File, time::Duration};

#[cfg(target_arch = "wasm32")]
use usb_wasm_bindings::{device::UsbDevice, types::Filter};

use anyhow::anyhow;

pub fn main() -> anyhow::Result<()> {
    let data = std::env::args().nth(1).ok_or(anyhow!("Usage: ping <data>"))?;

    #[cfg(target_arch = "wasm32")]
    let arduino_usb = UsbDevice::request_device(&Filter {
        vendor_id: Some(0x2341),
        product_id: Some(0x8057),
        ..Default::default()
    })
    .ok_or(anyhow!("Arduino not found."))?;

    #[cfg(not(target_arch = "wasm32"))]
    let arduino_usb = rusb::devices()?
        .iter()
        .find(|d| {
            if let Ok(device_descriptor) = d.device_descriptor() {
                device_descriptor.vendor_id() == 0x2341 && device_descriptor.product_id() == 0x8057
            } else {
                false
            }
        })
        .ok_or(anyhow!("Arduino not found."))?;

    // Select interface
    #[cfg(target_arch = "wasm32")]
    let configuration = arduino_usb
        .configurations()
        .into_iter()
        .find(|c: &usb_wasm_bindings::device::UsbConfiguration| c.descriptor().number == 1)
        .ok_or(anyhow!("Could not find configuration"))?;
    #[cfg(target_arch = "wasm32")]
    let interface = configuration
        .interfaces()
        .into_iter()
        .find(|i| {
            i.descriptor().interface_number == 0x00 && i.descriptor().alternate_setting == 0x00
        })
        .ok_or(anyhow!("Could not find interface"))?;
    #[cfg(target_arch = "wasm32")]
    let endpoint_out = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::Out
                && e.descriptor().endpoint_number == 0x01
        })
        .ok_or(anyhow!("Could not find out endpoint"))?;
    #[cfg(target_arch = "wasm32")]
    let endpoint_in = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::In
                && e.descriptor().endpoint_number == 0x02
        })
        .ok_or(anyhow!("Could not find in endpoint"))?;

    #[cfg(target_arch = "wasm32")]
    {
        // Open device
        arduino_usb.open();
        arduino_usb.reset();
        // arduino_usb.select_configuration(&configuration);
        arduino_usb.claim_interface(&interface);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let handle = arduino_usb.open()?;
    #[cfg(not(target_arch = "wasm32"))]
    let binding = arduino_usb.active_config_descriptor()?;
    #[cfg(not(target_arch = "wasm32"))]
    let interface_descriptor = binding
        .interfaces()
        .next()
        .ok_or(anyhow!("Could not find interface"))?
        .descriptors()
        .next()
        .ok_or(anyhow!("Could not find interface"))?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        handle.reset()?;
        handle.set_auto_detach_kernel_driver(true)?;
        handle.claim_interface(0x00)?;
    }

    println!("Connected to Arduino");

    let data_raw = data.as_bytes();

    let mut latencies: Vec<std::time::Duration> = Vec::new();
    const REPEATS: usize = 100;

    #[cfg(target_arch = "wasm32")]
    match interface.descriptor().interface_protocol {
        0x01 => {
            println!("Using protocol 0x01 (Bulk)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (bulk): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                arduino_usb.write_bulk(&endpoint_out, data_raw);
                let data = arduino_usb.read_bulk(
                    &endpoint_in,
                    endpoint_in.descriptor().max_packet_size as u64,
                );
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Received {} bytes (bulk): {:?}", data.len(), buf_utf8);
            }
        }
        0x02 => {
            println!("Using protocol 0x02 (Interrupt)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (interrupt): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                arduino_usb.write_interrupt(&endpoint_out, data_raw);
                let data = arduino_usb.read_interrupt(
                    &endpoint_in,
                    endpoint_in.descriptor().max_packet_size as u64,
                );
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Received {} bytes (interrupt): {:?}", data.len(), buf_utf8);
            }
        }
        0x03 => {
            println!("Using protocol 0x03 (Isochronous)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (isochronous): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                arduino_usb.write_isochronous(&endpoint_out, data_raw);
                let data = arduino_usb.read_isochronous(&endpoint_in);
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!(
                    "Received {} bytes (isochronous): {:?}",
                    data.len(),
                    buf_utf8
                );
            }
        }
        _ => {
            println!("Unknown protocol number");
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    match interface_descriptor.protocol_code() {
        0x01 => {
            println!("Using protocol 0x01 (Bulk)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (bulk): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                handle.write_bulk(0x01, data_raw, Duration::from_secs(60))?;
                let mut data = vec![0; 512];
                handle.read_bulk(0x02, &mut data, Duration::from_secs(60))?;
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Received {} bytes (bulk): {:?}", data.len(), buf_utf8);
            }
        }
        0x02 => {
            println!("Using protocol 0x02 (Interrupt)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (interrupt): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                handle.write_interrupt(0x01, data_raw, Duration::from_secs(60))?;
                let mut data = vec![0; 512];
                handle.read_interrupt(0x02, &mut data, Duration::from_secs(60))?;
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Received {} bytes (interrupt): {:?}", data.len(), buf_utf8);
            }
        }
        0x03 => {
            println!("Using protocol 0x03 (Isochronous)");
            for _ in 0..REPEATS {
                println!("Sending {} bytes (isochronous): {:?}", data_raw.len(), data);
                let start = std::time::Instant::now();
                iso_transfer_out(&handle, 0x01, data_raw)?;
                let data = iso_transfer_in(&handle, 0x82, 512)?;
                let end = std::time::Instant::now();
                latencies.push(end.duration_since(start));
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!(
                    "Received {} bytes (isochronous): {:?}",
                    data.len(),
                    buf_utf8
                );
            }
        }
        _ => {
            println!("Unknown protocol number");
        }
    }

    latencies.sort();

    println!("Latencies:");
    for latency in latencies.iter() {
        println!("{:?}", latency);
    }

    println!(
        "Average latency: {:?}",
        latencies.iter().sum::<std::time::Duration>() / REPEATS as _
    );
    println!(
        "Median latency: {:?}",
        latencies.iter().nth(latencies.len() / 2).unwrap()
    );
    println!("Max latency: {:?}", latencies.iter().max().unwrap());
    println!("Min latency: {:?}", latencies.iter().min().unwrap());

    let mut file = File::create("latencies_in_ms.txt")?;
    for latency in latencies.iter() {
        writeln!(file, "{:?}", 1000. * latency.as_secs_f64())?;
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
extern "system" fn libusb_transfer_cb(transfer: *mut rusb::ffi::libusb_transfer) {
    unsafe {
        *((*transfer).user_data as *mut i32) = 1;
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn error_from_libusb(err: i32) -> rusb::Error {
    match err {
        rusb::ffi::constants::LIBUSB_ERROR_IO => rusb::Error::Io,
        rusb::ffi::constants::LIBUSB_ERROR_INVALID_PARAM => rusb::Error::InvalidParam,
        rusb::ffi::constants::LIBUSB_ERROR_ACCESS => rusb::Error::Access,
        rusb::ffi::constants::LIBUSB_ERROR_NO_DEVICE => rusb::Error::NoDevice,
        rusb::ffi::constants::LIBUSB_ERROR_NOT_FOUND => rusb::Error::NotFound,
        rusb::ffi::constants::LIBUSB_ERROR_BUSY => rusb::Error::Busy,
        rusb::ffi::constants::LIBUSB_ERROR_TIMEOUT => rusb::Error::Timeout,
        rusb::ffi::constants::LIBUSB_ERROR_OVERFLOW => rusb::Error::Overflow,
        rusb::ffi::constants::LIBUSB_ERROR_PIPE => rusb::Error::Pipe,
        rusb::ffi::constants::LIBUSB_ERROR_INTERRUPTED => rusb::Error::Interrupted,
        rusb::ffi::constants::LIBUSB_ERROR_NO_MEM => rusb::Error::NoMem,
        rusb::ffi::constants::LIBUSB_ERROR_NOT_SUPPORTED => rusb::Error::NotSupported,
        rusb::ffi::constants::LIBUSB_ERROR_OTHER => rusb::Error::Other,
        _ => rusb::Error::Other,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn iso_transfer_in(
    handle: &rusb::DeviceHandle<rusb::GlobalContext>,
    endpoint: u8,
    buffer_size: usize,
) -> anyhow::Result<Vec<u8>> {
    use rusb::{
        constants::LIBUSB_TRANSFER_TYPE_ISOCHRONOUS,
        ffi::{libusb_alloc_transfer, libusb_handle_events_completed, libusb_submit_transfer},
        UsbContext,
    };

    let transfer = unsafe { libusb_alloc_transfer(1) };
    let transfer_ref = unsafe { &mut *transfer };

    let mut completed = 0_i32;
    let completed_ptr = (&mut completed) as *mut i32;

    let mut buffer = vec![0; buffer_size as usize];

    transfer_ref.dev_handle = handle.as_raw();
    transfer_ref.endpoint = endpoint;
    transfer_ref.transfer_type = LIBUSB_TRANSFER_TYPE_ISOCHRONOUS;
    transfer_ref.timeout = Duration::from_secs(60).as_millis() as _;
    transfer_ref.buffer = buffer.as_mut_slice().as_ptr() as *mut _;
    transfer_ref.length = buffer.len() as _;
    transfer_ref.num_iso_packets = 1;
    transfer_ref.user_data = completed_ptr as *mut _;

    let iso_packet_descs = unsafe {
        std::slice::from_raw_parts_mut(transfer_ref.iso_packet_desc.as_mut_ptr(), 1 as usize)
    };

    let entry = iso_packet_descs.get_mut(0).unwrap();
    entry.length = buffer_size as _;
    entry.status = 0;
    entry.actual_length = 0;

    transfer_ref.callback = libusb_transfer_cb;
    let err = unsafe { libusb_submit_transfer(transfer) };
    if err != 0 {
        return Err(anyhow!(
            "Error submitting transfer: {:?}",
            error_from_libusb(err)
        ));
    }

    let mut err = 0;
    unsafe {
        while (*completed_ptr) == 0 {
            err = libusb_handle_events_completed(handle.context().as_raw(), completed_ptr);
        }
    };
    if err != 0 {
        return Err(error_from_libusb(err).into());
    }

    let entry = iso_packet_descs.get_mut(0).unwrap();
    if entry.status == 0 {
        Ok(buffer[0..entry.actual_length as usize].to_vec())
    } else {
        Err(anyhow!(
            "Error transferring data: {:?}",
            error_from_libusb(entry.status)
        ))
        // TODO: handle errors here
        // Status code meanings
        // https://libusb.sourceforge.io/api-1.0/group__libusb__asyncio.html#ga9fcb2aa23d342060ebda1d0cf7478856
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn iso_transfer_out(
    handle: &rusb::DeviceHandle<rusb::GlobalContext>,
    endpoint: u8,
    _buffer: &[u8],
) -> anyhow::Result<u64> {
    use rusb::{
        constants::LIBUSB_TRANSFER_TYPE_ISOCHRONOUS,
        ffi::{libusb_alloc_transfer, libusb_handle_events_completed, libusb_submit_transfer},
        UsbContext,
    };

    let transfer = unsafe { libusb_alloc_transfer(1) };
    let transfer_ref = unsafe { &mut *transfer };

    let mut completed = 0_i32;
    let completed_ptr = (&mut completed) as *mut i32;

    // reorder the buffers so they're continuous in memory
    let buffer: Vec<u8> = _buffer.iter().copied().collect::<Vec<u8>>();

    transfer_ref.dev_handle = handle.as_raw();
    transfer_ref.endpoint = endpoint;
    transfer_ref.transfer_type = LIBUSB_TRANSFER_TYPE_ISOCHRONOUS;
    transfer_ref.timeout = Duration::from_secs(60).as_millis() as _;
    transfer_ref.buffer = buffer.as_ptr() as *mut _;
    transfer_ref.length = buffer.len() as _;
    transfer_ref.num_iso_packets = 1;
    // It should be okay to pass in this (stack) variable, as this function will not return untill after the transfer is complete.
    transfer_ref.user_data = completed_ptr as *mut _;

    let iso_packet_descs = unsafe {
        std::slice::from_raw_parts_mut(transfer_ref.iso_packet_desc.as_mut_ptr(), _buffer.len())
    };

    let entry = iso_packet_descs.get_mut(0).unwrap();
    entry.length = buffer.len() as _;
    entry.status = 0;
    entry.actual_length = 0;

    transfer_ref.callback = libusb_transfer_cb;

    let err = unsafe { libusb_submit_transfer(transfer) };
    if err != 0 {
        return Err(error_from_libusb(err).into());
    }

    let mut err = 0;
    unsafe {
        while (*completed_ptr) == 0 {
            err = libusb_handle_events_completed(handle.context().as_raw(), completed_ptr);
        }
    };
    if err != 0 {
        return Err(error_from_libusb(err).into());
    }

    let mut bytes_written: u64 = 0;
    for i in 0.._buffer.len() {
        let entry = iso_packet_descs.get_mut(i).unwrap();
        if entry.status == 0 {
            bytes_written += entry.actual_length as u64;
        } else {
            // TODO: handle errors here
            // Status code meanings
            // https://libusb.sourceforge.io/api-1.0/group__libusb__asyncio.html#ga9fcb2aa23d342060ebda1d0cf7478856
        }
    }

    Ok(bytes_written)
}
