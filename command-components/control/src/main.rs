use usb_wasm_bindings::{
    device::UsbDevice,
    types::{ControlSetup, ControlSetupRecipient, ControlSetupType, Filter},
};

use anyhow::anyhow;

pub fn main() -> anyhow::Result<()> {
    let arduino_usb = UsbDevice::request_device(&Filter {
        vendor_id: Some(0x2341),
        product_id: Some(0x8057),
        ..Default::default()
    })
    .ok_or(anyhow!("Arduino not found."))?;

    // Open device
    arduino_usb.open();

    // GET_DESCRIPTOR request https://www.beyondlogic.org/usbnutshell/usb6.shtml
    let response = arduino_usb.read_control(ControlSetup {
        request_type: ControlSetupType::Standard,
        request_recipient: ControlSetupRecipient::Device,
        request: 0x06,
        value: 0x0100,
        index: 0,
    });

    println!("Device Descriptor: {:?}", response);

    Ok(())
}
