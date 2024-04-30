use usb_wasm_bindings::device::UsbDevice;

pub fn main() -> anyhow::Result<()> {
    for device in UsbDevice::enumerate() {
        let descriptor = device.descriptor();
        println!(
            "{:#04x}:{:#04x} - {} {}",
            descriptor.vendor_id,
            descriptor.product_id,
            descriptor.manufacturer_name.unwrap_or("N/A".to_owned()),
            descriptor.product_name.unwrap_or("N/A".to_owned()),
        );
    }

    Ok(())
}
