use usb_wasm_bindings::{
    descriptors::{ConfigurationDescriptor, DeviceDescriptor, EndpointDescriptor, InterfaceDescriptor},
    device::{list_devices, UsbDevice},
};

fn class_to_string(class: u8) -> &'static str {
    match class {
        0x00 => "Defined at Interface Level",
        0x01 => "Audio",
        0x02 => "Communications and CDC Control",
        0x03 => "Human Interface Device",
        0x05 => "Physical",
        0x06 => "Image",
        0x07 => "Printer",
        0x08 => "Mass Storage",
        0x09 => "Hub",
        0x0a => "CDC-Data",
        0x0b => "Smart Card",
        0x0d => "Content Security",
        0x0e => "Video",
        0x0f => "Personal Healthcare",
        0x10 => "Audio/Video Devices",
        0x11 => "Billboard Device",
        0x12 => "USB Type-C Bridge",
        0x13 => "USB Bulk Display Protocol Device Class",
        0x14 => "MCTP over USB Protocol Endpoint Device Class",
        0x3c => "I3C Device Class",
        0xdc => "Diagnostic Device",
        0xe0 => "Wireless Controller",
        0xef => "Miscellaneous",
        0xfe => "Application Specific",
        0xff => "Vendor Specific",
        _ => "Unknown",
    }
}

struct Section {
    name: String,
    content: Vec<(&'static str, String, Option<String>)>,
}

impl Section {
    fn new(name: &'static str) -> Self {
        Self {
            name: name.to_owned(),
            content: Vec::new(),
        }
    }

    fn add(&mut self, name: &'static str, value: String, comment: Option<String>) {
        self.content.push((name, value, comment));
    }
}

fn print_section(section: Section, indent_level: usize) {
    let indent = "  ".repeat(indent_level);

    println!("{indent}{}", section.name);

    if section.content.is_empty() {
        return;
    }

    let (first_column_width, second_column_width) = section
        .content
        .iter()
        .map(|(name, value, _)| (name.len(), value.len()))
        .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
        .unwrap_or((0, 0));

    for (name, value, comment) in section.content {
        println!(
            "{indent}  {:<first_column_width$}   {:>second_column_width$} {}",
            name,
            value,
            comment.unwrap_or_default()
        );
    }
}

fn device_section(descriptor: &DeviceDescriptor) -> Section {
    let mut section = Section::new("Device Descriptor");
    section.add(
        "USB Version",
        format!(
            "{:#06x}",
            descriptor.usb_version_bcd
        ),
        None,
    );
    section.add(
        "Device Class",
        format!("{:#04x}", descriptor.device_class,),
        Some(class_to_string(descriptor.device_class).to_owned()),
    );
    section.add(
        "Subclass",
        format!("{:#04x}", descriptor.device_subclass),
        None,
    );
    section.add(
        "Protocol",
        format!("{:#04x}", descriptor.device_protocol),
        None,
    );
    section.add("Vendor ID", format!("{:#06x}", descriptor.vendor_id), None);
    section.add(
        "Product ID",
        format!("{:#06x}", descriptor.product_id),
        None,
    );
    section.add(
        "Device Version",
        format!(
            "{:#06x}",
            descriptor.device_version_bcd
        ),
        None,
    );
    section.add(
        "Num Configurations",
        descriptor.num_configurations.to_string(),
        None,
    );
    section
}

fn configuration_section(descriptor: &ConfigurationDescriptor) -> Section {
    let mut section = Section::new("Configuration Descriptor");
    section.add(
        "Configuration Value",
        format!("{:#04x}", descriptor.configuration_value),
        None,
    );
    section.add(
        "Attributes",
        format!("{:#04x}", descriptor.attributes),
        None,
    );
    section.add("Max Power", format!("{}mA", descriptor.max_power as u32 * 2), None);
    section
}

fn interface_section(descriptor: &InterfaceDescriptor) -> Section {
    let mut section = Section::new("Interface Descriptor");

    section.add(
        "Interface Number",
        format!("{:#04x}", descriptor.interface_number),
        None,
    );
    section.add(
        "Alternate Setting",
        format!("{:#04x}", descriptor.alternate_setting),
        None,
    );
    section.add(
        "Interface Class",
        format!("{:#04x}", descriptor.interface_class),
        Some(class_to_string(descriptor.interface_class).to_owned()),
    );
    section.add(
        "Interface Subclass",
        format!("{:#04x}", descriptor.interface_subclass),
        None,
    );
    section.add(
        "Interface Protocol",
        format!("{:#04x}", descriptor.interface_protocol),
        None,
    );

    section
}

fn endpoint_section(descriptor: &EndpointDescriptor) -> Section {
    let mut section = Section::new("Endpoint Descriptor");

    section.add(
        "Endpoint Address",
        format!("{:#04x}", descriptor.endpoint_address),
        None,
    );

    section.add(
        "Attributes",
        format!("{:#04x}", descriptor.attributes),
        None,
    );

    section.add(
        "Max Packet Size",
        format!("{:#06x}", descriptor.max_packet_size),
        None,
    );

    section.add(
        "Interval",
        format!("{}", descriptor.interval),
        None,
    );

    section
}

pub fn main() -> anyhow::Result<()> {
    let devices = list_devices().map_err(|e| anyhow::anyhow!("Failed to list devices: {:?}", e))?;
    
    let mut first = true;
    for (device, descriptor, location) in devices {
        if !first {
            println!();
        }
        first = false;
        
        println!(
            "Bus {:03} Device {:03}: ID {:04x}:{:04x} (Speed: {:?})",
            location.bus_number,
            location.device_address,
            descriptor.vendor_id,
            descriptor.product_id,
            location.speed,
        );
        
        print_section(device_section(&descriptor), 0);
        
        for i in 0..descriptor.num_configurations {
            match device.get_configuration_descriptor(i) {
                Ok(config) => {
                    print_section(configuration_section(&config), 1);
                    for iface in config.interfaces {
                        print_section(interface_section(&iface), 2);
                        for ep in iface.endpoints {
                            print_section(endpoint_section(&ep), 3);
                        }
                    }
                }
                _ => {
                    println!("  Failed to get configuration descriptor {}", i);
                }
            }
        }
    }

    Ok(())
}
