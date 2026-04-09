use usb_wasm_bindings::device::list_devices;
use usb_wasm_bindings::transfers::{await_transfer, TransferOptions, TransferSetup, TransferType};

use anyhow::anyhow;

pub fn main() -> anyhow::Result<()> {
    let devices = list_devices().map_err(|e| anyhow!("{:?}", e))?;
    let (arduino_device, _, _) = devices
        .into_iter()
        .find(|(_, desc, _)| desc.vendor_id == 0x2341 && desc.product_id == 0x8057)
        .ok_or(anyhow!("Arduino not found."))?;

    // Open device
    let handle = arduino_device.open().map_err(|e| anyhow!("{:?}", e))?;

    // GET_DESCRIPTOR request https://www.beyondlogic.org/usbnutshell/usb6.shtml
    let setup = TransferSetup {
        bm_request_type: 0x80, // Standard, Device, IN
        b_request: 0x06,
        w_value: 0x0100,
        w_index: 0,
    };
    let opts = TransferOptions {
        endpoint: 0,
        timeout_ms: 1000,
        stream_id: 0,
        iso_packets: 0,
    };
    
    let xfer = handle.new_transfer(TransferType::Control, setup, 64, opts)
        .map_err(|e| anyhow!("{:?}", e))?;
    xfer.submit_transfer(&[]).map_err(|e| anyhow!("{:?}", e))?;
    
    let response = await_transfer(xfer).map_err(|e| anyhow!("{:?}", e))?;

    println!("Device Descriptor: {:?}", response);

    Ok(())
}
