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

    let data = "Ping Ping Pong :)";
    let data_raw = data.as_bytes();

    match interface.descriptor().interface_protocol {
        0x01 => {
            println!("Using protocol 0x01 (Bulk)");
            loop {
                let bytes_written = arduino_usb.write_bulk(&endpoint_out, data_raw);
                println!("Sent {} bytes (bulk): {:?}", bytes_written, data);
                let data = arduino_usb.read_bulk(
                    &endpoint_in,
                    endpoint_in.descriptor().max_packet_size as u64,
                );
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Read {} bytes (bulk): {:?}", data.len(), buf_utf8);
            }
        }
        0x02 => {
            println!("Using protocol 0x02 (Interrupt)");
            loop {
                let bytes_written = arduino_usb.write_interrupt(&endpoint_out, data_raw);
                println!("Sent {} bytes (interrupt): {:?}", bytes_written, data);
                let data = arduino_usb.read_interrupt(
                    &endpoint_in,
                    endpoint_in.descriptor().max_packet_size as u64,
                );
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Read {} bytes (interrupt): {:?}", data.len(), buf_utf8);
            }
        }
        0x03 => {
            println!("Using protocol 0x03 (Isochronous)");
            loop {
                let bytes_written = arduino_usb.write_isochronous(&endpoint_out, data_raw);
                println!("Sent {} bytes (isochronous): {:?}", bytes_written, data);
                let data = arduino_usb.read_isochronous(&endpoint_in);
                let buf_utf8 = String::from_utf8_lossy(&data);
                println!("Read {} bytes (isochronous): {:?}", data.len(), buf_utf8);
            }
        }
        _ => {
            println!("Unknown protocol number");
        }
    }

    Ok(())
}
