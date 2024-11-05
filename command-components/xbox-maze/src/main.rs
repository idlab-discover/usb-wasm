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
            write!($f, "{} ", $text.green().bold())
        } else {
            write!($f, "{} ", $text.red())
        }
    };
}

impl std::fmt::Display for XboxControllerState {
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

const WALL: &str = "\x1B[38;5;75m▓\x1B[0m";
const FOOD: &str = "\x1B[38;5;214m•\x1B[0m";
const EMPTY: &str = " ";
const PACMAN: &str = "\x1B[38;5;226m◉\x1B[0m";
const GHOST: &str = "\x1B[38;5;160m󰊠\x1B[0m";

fn print_maze(maze: &[[&str; 30]; 14]) {
    for row in maze.iter() {
        for cell in row.iter() {
            print!("{}", cell);
        }
        println!();
    }
}

pub fn main() -> anyhow::Result<()> {
    let xbox_controller = UsbDevice::request_device(&Filter {
        vendor_id: Some(0x045e),
        product_id: Some(0x02ea),
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
                && e.descriptor().endpoint_number == 0x02
        })
        .ok_or(anyhow!("Could not find IN endpoint"))?;
    let endpoint_out = interface
        .endpoints()
        .into_iter()
        .find(|e| {
            e.descriptor().direction == usb_wasm_bindings::types::Direction::Out
                && e.descriptor().endpoint_number == 0x02
        })
        .ok_or(anyhow!("Could not find OUT endpoint"))?;

    // Open device
    xbox_controller.open();
    xbox_controller.claim_interface(&interface);

    // Set up the device (https://github.com/quantus/xbox-one-controller-protocol)
    xbox_controller.write_interrupt(&endpoint_out, &[0x05, 0x20, 0x00, 0x01, 0x00]);

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

    // (y, x)
    let mut current_pos = (6, 11);
    let mut button_down = false;

    // Clear screen and home cursor
    print!("\x1B[2J");
    print!("\x1B[H");

    println!("Connected to Xbox Controller\n");
    println!("{}", XboxControllerState::default()); // Print empty values first until we get our first communication
    print_maze(&maze);
    io::stdout().flush()?;

    loop {
        // Clear screen and home cursor
        print!("\x1B[2J");
        print!("\x1B[H");

        let data =
            xbox_controller.read_interrupt(&endpoint, endpoint.descriptor().max_packet_size as u64);
        if data.len() == 18 {
            let state = parse_xbox_controller_data(&data[0..18]);

            println!("Connected to Xbox Controller\n");
            println!("{}", state);
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
    }
}
