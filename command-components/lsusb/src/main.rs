use usb_wasm_bindings::{
    device::{UsbConfiguration, UsbDevice, UsbEndpoint, UsbInterface},
    types::{Direction, TransferType},
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
    let indent = "\t".repeat(indent_level);

    println!("{indent}{}", section.name);

    let (first_column_width, second_column_width) = section
        .content
        .iter()
        .map(|(name, value, _)| (name.len(), value.len()))
        .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)))
        .unwrap_or((0, 0));

    for (name, value, comment) in section.content {
        println!(
            "{indent}\t{:<first_column_width$}   {:>second_column_width$} {}",
            name,
            value,
            comment.unwrap_or_default()
        );
    }
}

fn device_section(device: &UsbDevice) -> Section {
    let descriptor = device.descriptor();
    let mut section = Section::new("Device Descriptor");
    section.add(
        "USB Version",
        format!(
            "{}.{}{}",
            descriptor.usb_version.0, descriptor.usb_version.1, descriptor.usb_version.2
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
            "{}.{}{}",
            descriptor.device_version.0, descriptor.device_version.1, descriptor.device_version.2
        ),
        None,
    );
    section.add(
        "Manufacturer",
        String::default(),
        descriptor.manufacturer_name,
    );
    section.add("Product", String::default(), descriptor.product_name);
    section.add("Serial Number", String::default(), descriptor.serial_number);
    section
}

fn configuration_section(configuration: &UsbConfiguration) -> Section {
    let descriptor = configuration.descriptor();
    let mut section = Section::new("Configuration Descriptor");
    section.add(
        "Configuration Value",
        format!("{:#04x}", descriptor.number),
        None,
    );
    section.add(
        "Configuration Description",
        String::default(),
        descriptor.description,
    );
    section.add(
        "Self Powered",
        (if descriptor.self_powered {
            "✓"
        } else {
            "✗"
        })
        .to_owned(),
        None,
    );
    section.add(
        "Remote Wakeup",
        (if descriptor.remote_wakeup {
            "✓"
        } else {
            "✗"
        })
        .to_owned(),
        None,
    );
    section.add("Max Power", format!("{}mA", descriptor.max_power), None);
    section
}

fn interface_section(interface: &UsbInterface) -> Section {
    let mut section = Section::new("Interface Descriptor");
    let descriptor = interface.descriptor();

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
    section.add(
        "Interface Name",
        String::default(),
        descriptor.interface_name,
    );

    section
}

fn endpoint_section(endpoint: &UsbEndpoint) -> Section {
    let mut section = Section::new("Endpoint Descriptor");
    let descriptor = endpoint.descriptor();

    section.add(
        "Endpoint Number",
        format!("{:#04x}", descriptor.endpoint_number),
        None,
    );

    section.add(
        "Direction",
        match descriptor.direction {
            Direction::In => "In",
            Direction::Out => "Out",
        }
        .to_owned(),
        None,
    );

    section.add(
        "Transfer Type",
        match descriptor.transfer_type {
            TransferType::Control => "Control",
            TransferType::Isochronous => "Isochronous",
            TransferType::Bulk => "Bulk",
            TransferType::Interrupt => "Interrupt",
        }
        .to_owned(),
        None,
    );

    // section.add(
    //     "Synchronization Type",
    //     match descriptor.synchronization_type {
    //         usb::SynchronizationType::None => "None",
    //         usb::SynchronizationType::Asynchronous => "Asynchronous",
    //         usb::SynchronizationType::Adaptive => "Adaptive",
    //         usb::SynchronizationType::Synchronous => "Synchronous",
    //     }
    //     .to_owned(),
    //     None,
    // );

    // section.add(
    //     "Usage Type",
    //     match descriptor.usage_type {
    //         usb::UsageType::Data => "Data",
    //         usb::UsageType::Feedback => "Feedback",
    //         usb::UsageType::ImplicitFeedbackData => "Implicit Feedback Data",
    //     }
    //     .to_owned(),
    //     None,
    // );

    section.add(
        "Max Packet Size",
        format!("{:#04x}", descriptor.max_packet_size),
        None,
    );

    section
}

pub fn main() -> anyhow::Result<()> {
    let mut first = true;
    for device in UsbDevice::enumerate() {
        if !first {
            println!();
        }
        first = false;
        let descriptor = device.descriptor();
        println!(
            "ID {:#04x}:{:#04x} - {} {} ({})",
            descriptor.vendor_id,
            descriptor.product_id,
            descriptor.manufacturer_name.unwrap_or("N/A".to_owned()),
            descriptor.product_name.unwrap_or("N/A".to_owned()),
            class_to_string(descriptor.device_class),
        );
        print_section(device_section(&device), 0);
        for configuration in device.configurations() {
            print_section(configuration_section(&configuration), 1);

            for interface in configuration.interfaces() {
                print_section(interface_section(&interface), 2);
                for endpoint in interface.endpoints() {
                    print_section(endpoint_section(&endpoint), 3);
                }
            }
        }
    }

    Ok(())
}
