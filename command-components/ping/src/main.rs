use usb_wasm_bindings::{device::UsbDevice, types::Filter};

use anyhow::anyhow;

pub fn main() -> anyhow::Result<()> {
    let arduino_usb = UsbDevice::request_device(&Filter {
        vendor_id: Some(0x2341),
        product_id: Some(0x8057),
        ..Default::default()
    })
    .ok_or(anyhow!("Arduino not found."))?;

    // Select interface
    let configuration = arduino_usb
        .configurations()
        .into_iter()
        .find(|c: &usb_wasm_bindings::device::UsbConfiguration| c.descriptor().number == 1)
        .ok_or(anyhow!("Could not find configuration"))?;
    let interface = configuration
        .interfaces()
        .into_iter()
        .find(|i| {
            i.descriptor().interface_number == 0x00 && i.descriptor().alternate_setting == 0x00
        })
        .ok_or(anyhow!("Could not find interface"))?;
    let endpoint_out = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::Out
                && e.descriptor().endpoint_number == 0x01
        })
        .ok_or(anyhow!("Could not find out endpoint"))?;
    let endpoint_in = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::In
                && e.descriptor().endpoint_number == 0x02
        })
        .ok_or(anyhow!("Could not find in endpoint"))?;

    // Open device
    arduino_usb.open();
    arduino_usb.reset();
    // arduino_usb.select_configuration(&configuration);
    arduino_usb.claim_interface(&interface);

    println!("Connected to Arduino");

    let data = "PING";
    let data_raw = data.as_bytes();

    let mut latencies: Vec<std::time::Duration> = Vec::new();
    const REPEATS: usize = 100;

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

    Ok(())
}
