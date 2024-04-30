use usb_wasm_bindings::device::UsbDevice;
use usb_wasm_bindings::types::Filter;

use std::io;
use std::io::Write;

use anyhow::anyhow;
use byteorder::ByteOrder;
use colored::Colorize;

#[derive(Copy, Clone, Debug, Default)]
pub struct XboxControllerState {
    a: bool,
    b: bool,
    x: bool,
    y: bool,
    start: bool,
    select: bool,

    up: bool,
    down: bool,
    left: bool,
    right: bool,
    lb: bool,
    rb: bool,
    lstick: bool,
    rstick: bool,

    lt: f32,
    rt: f32,
    lstick_x: f32,
    lstick_y: f32,
    rstick_x: f32,
    rstick_y: f32,
}

macro_rules! render_pressed {
    ($f:expr, $text:expr, $condition:expr) => {
        if $condition {
            write!($f, " {}", $text.green().bold())
        } else {
            write!($f, " {}", $text.red().bold())
        }
    };
}

impl std::fmt::Display for XboxControllerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print sticks
        write!(
            f,
            "LSTICK X: {:>4.0}% LSTICK Y: {:>4.0}% RSTICK X: {:>4.0}% RSTICK Y: {:>4.0}% LT: {:>3.0}% RT: {:>3.0}%",
            100. * self.lstick_x,
            100. * self.lstick_y,
            100. * self.rstick_x,
            100. * self.rstick_y,
            100. * self.lt,
            100. * self.rt,
        )?;
        render_pressed!(f, "A", self.a)?;
        render_pressed!(f, "B", self.b)?;
        render_pressed!(f, "X", self.x)?;
        render_pressed!(f, "Y", self.y)?;

        render_pressed!(f, "Up", self.up)?;
        render_pressed!(f, "Down", self.down)?;
        render_pressed!(f, "Left", self.left)?;
        render_pressed!(f, "Right", self.right)?;

        render_pressed!(f, "Start", self.start)?;
        render_pressed!(f, "Select", self.select)?;
        render_pressed!(f, "LB", self.lb)?;
        render_pressed!(f, "RB", self.rb)?;
        render_pressed!(f, "LS", self.lstick)?;
        render_pressed!(f, "RS", self.rstick)?;
        Ok(())
    }
}

pub fn parse_xbox_controller_data(data: &[u8]) -> XboxControllerState {
    assert!(data.len() >= 18, "data is too short");
    let lt = byteorder::LittleEndian::read_u16(&data[6..]) as f32 / 1023.0;
    let rt = byteorder::LittleEndian::read_u16(&data[8..]) as f32 / 1023.0;

    let lstick_x = (byteorder::LittleEndian::read_i16(&data[10..]) as f32 + 0.5) / 32767.5;
    let lstick_y = (byteorder::LittleEndian::read_i16(&data[12..]) as f32 + 0.5) / 32767.5;
    let rstick_x = (byteorder::LittleEndian::read_i16(&data[14..]) as f32 + 0.5) / 32767.5;
    let rstick_y = (byteorder::LittleEndian::read_i16(&data[16..]) as f32 + 0.5) / 32767.5;
    XboxControllerState {
        a: (data[4] & 0x10) != 0,
        b: (data[4] & 0x20) != 0,
        x: (data[4] & 0x40) != 0,
        y: (data[4] & 0x80) != 0,
        start: (data[4] & 0x08) != 0,
        select: (data[4] & 0x04) != 0,

        up: (data[5] & 0x01) != 0,
        down: (data[5] & 0x02) != 0,
        left: (data[5] & 0x04) != 0,
        right: (data[5] & 0x08) != 0,
        lb: (data[5] & 0x10) != 0,
        rb: (data[5] & 0x20) != 0,
        lstick: (data[5] & 0x40) != 0,
        rstick: (data[5] & 0x80) != 0,

        lt,
        rt,
        lstick_x,
        lstick_y,
        rstick_x,
        rstick_y,
    }
}

pub fn main() -> anyhow::Result<()> {
    let xbox_controller = UsbDevice::request_device(&Filter {
        vendor_id: Some(0x045e),
        product_id: Some(0x02d1),
        ..Default::default()
    })
    .ok_or(anyhow!("No Xbox Controller found!"))?;

    // Select interface
    let configuration = xbox_controller
        .configurations()
        .into_iter()
        .find(|c| c.descriptor().number == 1)
        .ok_or(anyhow!("Could not find configuration"))?;
    let interface = configuration
        .interfaces()
        .into_iter()
        .find(|i| {
            i.descriptor().interface_number == 0x00 && i.descriptor().alternate_setting == 0x00
        })
        .ok_or(anyhow!("Could not find interface"))?;
    let endpoint = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::In
                && e.descriptor().endpoint_number == 0x01
        })
        .ok_or(anyhow!("Could not find endpoint"))?;

    // Open device
    xbox_controller.open();
    xbox_controller.select_configuration(&configuration);
    xbox_controller.claim_interface(&interface);

    println!("Connected to Xbox Controller");
    let mut previous_length = 0;

    print!("\r{} ", XboxControllerState::default()); //Print empty values first untill we get our first communication
    io::stdout().flush()?;

    loop {
        let data =
            xbox_controller.read_interrupt(&endpoint, endpoint.descriptor().max_packet_size as u64);
        if data.len() == 18 {
            let state = parse_xbox_controller_data(&data[0..18]);
            let state_str = state.to_string();
            if state_str.len() < previous_length {
                print!(
                    "\r{}{} ",
                    state,
                    " ".repeat(previous_length - state_str.len())
                );
            } else {
                print!("\r{} ", state);
            }
            io::stdout().flush()?;
            previous_length = state_str.len();
        }
    }
}
