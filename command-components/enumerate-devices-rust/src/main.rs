use usb_wasm_bindings::device::list_devices;

pub fn main() -> anyhow::Result<()> {
    let devices = list_devices().map_err(|e| anyhow::anyhow!("Failed to list devices: {:?}", e))?;

    for (_device, descriptor, _location) in devices {
        println!(
            "{:#04x}:{:#04x} (Manufacturer Index: {}, Product Index: {})",
            descriptor.vendor_id,
            descriptor.product_id,
            descriptor.manufacturer_index,
            descriptor.product_index,
        );
    }

    Ok(())
}
