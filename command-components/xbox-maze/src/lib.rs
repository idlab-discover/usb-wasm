// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

use rusb::{Context, Direction, UsbContext};
use std::io;
use std::io::Write;
use std::time::Duration;

use anyhow::anyhow;
use byteorder::ByteOrder;
use colored::Colorize;

#[derive(Copy, Clone, Debug, Default)]
pub struct GamepadState {
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
            write!($f, "{} ", $text.green().bold())
        } else {
            write!($f, "{} ", $text.red())
        }
    };
}

impl std::fmt::Display for GamepadState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Print sticks
        writeln!(
            f,
            "LS X: {:>3.0}%\tLS Y: {:>3.0}%\nRS X: {:>3.0}%\tRS Y: {:>3.0}%\nLT  : {:>3.0}%\tRT  : {:>3.0}%\n",
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

        writeln!(f, "\n")?;

        if self.up {
            writeln!(f, " {}", "↑".green().bold())?;
        } else {
            writeln!(f, " {}", "↑".red())?;
        }

        if self.left && !self.right {
            writeln!(f, "{} {}", "←".green().bold(), "→".red())?;
        } else if !self.left && self.right {
            writeln!(f, "{} {}", "←".red(), "→".green().bold())?;
        } else if self.left && self.right {
            writeln!(f, "{} {}", "←".green().bold(), "→".green().bold())?;
        } else {
            writeln!(f, "{} {}", "←".red(), "→".red())?;
        }

        if self.down {
            writeln!(f, " {}", "↓".green().bold())?;
        } else {
            writeln!(f, " {}", "↓".red())?;
        }

        Ok(())
    }
}

pub fn parse_xbox_controller_data(data: &[u8]) -> GamepadState {
    if data.len() < 18 {
        return GamepadState::default();
    }
    let lt = byteorder::LittleEndian::read_u16(&data[6..]) as f32 / 1023.0;
    let rt = byteorder::LittleEndian::read_u16(&data[8..]) as f32 / 1023.0;

    let lstick_x = (byteorder::LittleEndian::read_i16(&data[10..]) as f32 + 0.5) / 32767.5;
    let lstick_y = (byteorder::LittleEndian::read_i16(&data[12..]) as f32 + 0.5) / 32767.5;
    let rstick_x = (byteorder::LittleEndian::read_i16(&data[14..]) as f32 + 0.5) / 32767.5;
    let rstick_y = (byteorder::LittleEndian::read_i16(&data[16..]) as f32 + 0.5) / 32767.5;
    GamepadState {
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

/// Parses a DualSense (PS5) controller report map over USB.
/// 
/// TODO (User Configurable): 
/// The indices and bitmasks here correspond to the typical mapping of a PS5 
/// over USB (Report ID 0x01). You can log `data` directly to terminal using 
/// `println!("{:?}", data)` to reverse engineer if the buttons don't match up.
pub fn parse_ps5_controller_data(data: &[u8]) -> GamepadState {
    if data.len() < 10 {
        return GamepadState::default();
    }
    
    // PS5 DualSense typically uses Report ID 1 over USB
    let offset = if data[0] == 0x01 { 0 } else { return GamepadState::default(); };
    
    // D-Pad is typically a 4-bit hat switch in byte 8 (values 0-7, 8 is neutral)
    let dpad = data[offset + 8] & 0x0F;
    let up = dpad == 0 || dpad == 1 || dpad == 7;
    let right = dpad == 1 || dpad == 2 || dpad == 3;
    let down = dpad == 3 || dpad == 4 || dpad == 5;
    let left = dpad == 5 || dpad == 6 || dpad == 7;

    let square = (data[offset + 8] & 0x10) != 0;
    let cross = (data[offset + 8] & 0x20) != 0;
    let circle = (data[offset + 8] & 0x40) != 0;
    let triangle = (data[offset + 8] & 0x80) != 0;

    GamepadState {
        a: cross,     // Map cross to A
        b: circle,    // Map circle to B
        x: square,    // Map square to X
        y: triangle,  // Map triangle to Y
        up,
        down,
        left,
        right,
        ..Default::default()
    }
}

const WALL: &str = "\x1B[38;5;75m▓\x1B[0m";
const FOOD: &str = "\x1B[38;5;214m•\x1B[0m";
const EMPTY: &str = " ";
const PACMAN: &str = "\x1B[38;5;226m◉\x1B[0m";
const GHOST: &str = "\x1B[38;5;160m⭑\x1B[0m";

fn print_maze(maze: &[[&str; 30]; 14]) {
    for row in maze.iter() {
        for cell in row.iter() {
            print!("{}", cell);
        }
        println!();
    }
}

#[derive(PartialEq)]
enum ControllerType {
    Xbox,
    PS5,
}

pub fn run() -> anyhow::Result<()> {
    let context = Context::new()?;
    let mut handle = None;
    let mut controller_type = ControllerType::Xbox;

    // TODO (User Configurable): 
    // Here we specify the typical Vendor ID and Product ID for the controllers.
    // If your PS5 controller has a different PID, update the numbers here.
    let xbox_ids = (0x045e, 0x02ea);
    let ps5_ids = (0x054c, 0x0ce6);

    for device in context.devices()?.iter() {
        let desc = device.device_descriptor()?;
        if desc.vendor_id() == xbox_ids.0 && desc.product_id() == xbox_ids.1 {
            handle = Some(device.open()?);
            controller_type = ControllerType::Xbox;
            break;
        } else if desc.vendor_id() == ps5_ids.0 && desc.product_id() == ps5_ids.1 {
            handle = Some(device.open()?);
            controller_type = ControllerType::PS5;
            break;
        }
    }

    let handle = handle.ok_or_else(|| anyhow!("No supported controller (Xbox or PS5) found!"))?;
    let device = handle.device();
    let config_desc = device.config_descriptor(0)?;

    let mut endpoint_in = None;
    let mut endpoint_out = None;
    let mut target_interface = None;

    // Find the interface with the interrupt IN endpoint
    for interface in config_desc.interfaces() {
        for alt_setting in interface.descriptors() {
            for ep_desc in alt_setting.endpoint_descriptors() {
                if ep_desc.transfer_type() == rusb::TransferType::Interrupt {
                    if ep_desc.direction() == Direction::In {
                        endpoint_in = Some(ep_desc.address());
                        target_interface = Some(interface.number());
                    } else if ep_desc.direction() == Direction::Out {
                        endpoint_out = Some(ep_desc.address());
                    }
                }
            }
        }
        if target_interface.is_some() {
            break;
        }
    }

    let interface_number = target_interface.ok_or_else(|| anyhow!("Could not find an interface with an Interrupt IN endpoint"))?;
    let endpoint_in = endpoint_in.unwrap();

    // Detach kernel driver if needed
    let _ = handle.set_auto_detach_kernel_driver(true);
    handle.claim_interface(interface_number)?;
    
    // Xbox controller Initialization Magic
    if controller_type == ControllerType::Xbox {
        let endpoint_out = endpoint_out.ok_or_else(|| anyhow!("Could not find OUT endpoint for Xbox init"))?;
        let _ = handle.write_interrupt(endpoint_out, &[0x05, 0x20, 0x00, 0x01, 0x00], Duration::from_millis(1000));
    }

    let mut maze = [
        [WALL; 30],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, WALL, WALL, WALL, WALL, FOOD,
            WALL, WALL, FOOD, WALL, WALL, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            WALL, WALL, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD,
            WALL, WALL, GHOST, FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, WALL,
        ],
        [
            WALL, WALL, WALL, FOOD, FOOD, WALL, WALL, WALL, WALL, WALL, WALL, PACMAN, EMPTY, EMPTY,
            EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, WALL, WALL, WALL, WALL, WALL, WALL, FOOD, FOOD,
            WALL, WALL, WALL,
        ],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD,
            GHOST, WALL,
        ],
        [
            WALL, FOOD, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            WALL, WALL, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, WALL, WALL, WALL, WALL, FOOD,
            FOOD, FOOD, FOOD, WALL, WALL, WALL, WALL, WALL, WALL, FOOD, WALL, WALL, WALL, WALL,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            FOOD, WALL,
        ],
        [
            WALL, FOOD, WALL, WALL, WALL, WALL, FOOD, GHOST, FOOD, FOOD, WALL, WALL, FOOD, FOOD,
            FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, WALL, WALL, WALL, WALL,
            FOOD, WALL,
        ],
        [WALL; 30],
    ];

    let mut current_pos = (6, 11);
    let mut button_down = false;
    let mut buf = [0u8; 64]; // Fits typically ~64 byte PS5 or Xbox packets

    print!("\x1B[2J");
    print!("\x1B[H");

    print_maze(&maze);
    io::stdout().flush()?;

    loop {
        // We use a short timeout so that when no data is sent, the game doesn't hang gracefully
        match handle.read_interrupt(endpoint_in, &mut buf, Duration::from_millis(10)) {
            Ok(bytes_read) => {
                let state = if controller_type == ControllerType::Xbox {
                    parse_xbox_controller_data(&buf[0..bytes_read])
                } else {
                    parse_ps5_controller_data(&buf[0..bytes_read])
                };

                // Clear screen and home cursor
                print!("\x1B[2J");
                print!("\x1B[H");
                
                // You can uncomment this to debug the controller mapping:
                // println!("{:?}", &buf[0..bytes_read]);
                
                print_maze(&maze);

                if !button_down {
                    if state.right {
                        button_down = true;

                        if maze[current_pos.0][current_pos.1 + 1] != WALL {
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos.1 += 1;
                            maze[current_pos.0][current_pos.1] = PACMAN;
                        }
                    }

                    if state.left {
                        button_down = true;

                        if maze[current_pos.0][current_pos.1 - 1] != WALL {
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos.1 -= 1;
                            maze[current_pos.0][current_pos.1] = PACMAN;
                        }
                    }

                    if state.up {
                        button_down = true;

                        if maze[current_pos.0 - 1][current_pos.1] != WALL {
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos.0 -= 1;
                            maze[current_pos.0][current_pos.1] = PACMAN;
                        }
                    }

                    if state.down {
                        button_down = true;

                        if maze[current_pos.0 + 1][current_pos.1] != WALL {
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos.0 += 1;
                            maze[current_pos.0][current_pos.1] = PACMAN;
                        }
                    }
                } else if !state.up && !state.down && !state.left && !state.right {
                    button_down = false;
                }

                io::stdout().flush()?;
            }
            Err(rusb::Error::Timeout) => {
                // Ignore timeouts, just loop around. This ensures we don't crash when inactive.
                continue;
            }
            Err(e) => {
                // Return if there was an actual communication error (e.g. disconnected)
                return Err(anyhow!("USB Read Error: {}", e));
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn exports_wasi_cli_run_run() -> bool {
    match run() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Error: {}", e);
            false
        }
    }
}

