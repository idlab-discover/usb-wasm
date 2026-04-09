// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

use usb_wasm_bindings::types::{TransferSetup, TransferOptions, TransferType};
use usb_wasm_bindings::{list_devices, await_transfer};

use std::io;
use std::io::Write;

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

pub fn parse_ps5_controller_data(data: &[u8]) -> GamepadState {
    if data.len() < 10 {
        return GamepadState::default();
    }
    
    let offset = if data[0] == 0x01 { 0 } else { return GamepadState::default(); };
    
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
        a: cross,
        b: circle,
        x: square,
        y: triangle,
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
    let xbox_ids = (0x045e, 0x02ea);
    let ps5_ids = (0x054c, 0x0ce6);

    let devices = list_devices().map_err(|e| anyhow!("Failed to list devices: {:?}", e))?;
    let mut selected_device = None;
    let mut controller_type = ControllerType::Xbox;

    for (dev, desc, _loc) in devices {
        if desc.vendor_id == xbox_ids.0 && desc.product_id == xbox_ids.1 {
            selected_device = Some(dev);
            controller_type = ControllerType::Xbox;
            break;
        } else if desc.vendor_id == ps5_ids.0 && desc.product_id == ps5_ids.1 {
            selected_device = Some(dev);
            controller_type = ControllerType::PS5;
            break;
        }
    }

    let device = selected_device.ok_or_else(|| anyhow!("No supported controller found!"))?;
    let handle = device.open().map_err(|e| anyhow!("Failed to open device: {:?}", e))?;
    
    let (interface_num, endpoint_in, endpoint_out) = if controller_type == ControllerType::Xbox {
        (0, 0x81, 0x01)
    } else {
        (3, 0x81, 0x01)
    };

    handle.claim_interface(interface_num).map_err(|e| anyhow!("Failed to claim interface: {:?}", e))?;
    
    let default_setup = TransferSetup {
        bm_request_type: 0,
        b_request: 0,
        w_value: 0,
        w_index: 0,
    };

    if controller_type == ControllerType::Xbox {
        // Initialization Magic for Xbox using OUT transfer
        let opts = TransferOptions {
            endpoint: endpoint_out,
            timeout_ms: 1000,
            stream_id: 0,
            iso_packets: 0,
        };
        let xfer = handle.new_transfer(TransferType::Interrupt, default_setup, 5, opts)
            .map_err(|e| anyhow!("Failed to create init transfer: {:?}", e))?;
        let _ = xfer.submit_transfer(&vec![0x05, 0x20, 0x00, 0x01, 0x00]);
        let _ = await_transfer(xfer);
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

    print!("\x1B[2J");
    print!("\x1B[H");

    print_maze(&maze);
    io::stdout().flush()?;

    loop {
        let opts = TransferOptions {
            endpoint: endpoint_in,
            timeout_ms: 1000,
            stream_id: 0,
            iso_packets: 0,
        };
        let xfer = handle.new_transfer(TransferType::Interrupt, default_setup, 64, opts)
            .map_err(|e| anyhow!("Failed to create transfer: {:?}", e))?;
        
        let _ = xfer.submit_transfer(&vec![]); // Submitting an empty buffer for an IN transfer
        let result = await_transfer(xfer);

        match result {
            Ok(data) => {
                let state = if controller_type == ControllerType::Xbox {
                    parse_xbox_controller_data(&data)
                } else {
                    parse_ps5_controller_data(&data)
                };

                // Clear screen and home cursor
                print!("\x1B[2J");
                print!("\x1B[H");
                
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
            Err(_) => {
                // Return if there was an actual communication error
                return Err(anyhow!("USB Read Error"));
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
