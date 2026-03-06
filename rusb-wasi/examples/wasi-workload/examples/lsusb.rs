use rusb::{Context, Device, DeviceDescriptor, Direction, TransferType, UsbContext};
use std::time::Duration;

#[no_mangle]
#[export_name = "cabi_realloc"]
pub unsafe extern "C" fn cabi_realloc(
    old_ptr: *mut u8,
    old_size: usize,
    _align: usize,
    new_size: usize,
) -> *mut u8 {
    if new_size == 0 {
        return std::ptr::null_mut();
    }
    if old_ptr.is_null() {
        std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(new_size, _align))
    } else {
        std::alloc::realloc(
            old_ptr,
            std::alloc::Layout::from_size_align_unchecked(old_size, _align),
            new_size,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn exports_wasi_cli_run_run() -> bool {
    let _ = cabi_realloc as usize;
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        return false;
    }
    true
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> rusb::Result<()> {
    // Initialize libusb context
    let context = Context::new()?;
    
    // Get list of devices
    let devices = context.devices()?;
    println!("Bus | Device | VID:PID | Description");
    println!("-------------------------------------");

    for device in devices.iter() {
        let device_desc = match device.device_descriptor() {
            Ok(d) => d,
            Err(_) => continue,
        };

        println!(
            "{:03} | {:03}    | {:04x}:{:04x} | Device",
            device.bus_number(),
            device.address(),
            device_desc.vendor_id(),
            device_desc.product_id()
        );

        print_device_hierarchy(&device, &device_desc)?;
    }

    Ok(())
}

fn print_device_hierarchy<T: UsbContext>(device: &Device<T>, device_desc: &DeviceDescriptor) -> rusb::Result<()> {
    let num_configs = device_desc.num_configurations();
    
    for i in 0..num_configs {
        let config_desc = match device.config_descriptor(i) {
            Ok(c) => c,
            Err(_) => continue,
        };

        println!("  |__ Config {:02}: MaxPower {}mA", i, config_desc.max_power());

        for interface in config_desc.interfaces() {
            println!("      |__ Interface {:02}", interface.number());

            for interface_desc in interface.descriptors() {
                println!(
                    "          |__ Alt {:02}: Class {:02x} SubClass {:02x} Protocol {:02x}",
                    interface_desc.setting_number(),
                    interface_desc.class_code(),
                    interface_desc.sub_class_code(),
                    interface_desc.protocol_code()
                );

                for endpoint_desc in interface_desc.endpoint_descriptors() {
                    println!(
                        "              |__ Endpoint {:02x}: {:?} {:?} MaxPacket {}",
                        endpoint_desc.address(),
                        endpoint_desc.direction(),
                        endpoint_desc.transfer_type(),
                        endpoint_desc.max_packet_size()
                    );
                }
            }
        }
    }
    
    Ok(())
}
