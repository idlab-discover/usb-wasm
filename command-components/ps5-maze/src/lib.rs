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
    
    // Joystick axes (Bytes 1-4)
    let ls_x = (data[offset + 1] as f32 - 128.0) / 128.0;
    let ls_y = (data[offset + 2] as f32 - 128.0) / 128.0;
    let rs_x = (data[offset + 3] as f32 - 128.0) / 128.0;
    let rs_y = (data[offset + 4] as f32 - 128.0) / 128.0;

    // Triggers (Bytes 5-6)
    let lt = data[offset + 5] as f32 / 255.0;
    let rt = data[offset + 6] as f32 / 255.0;

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
        lt,
        rt,
        lstick_x: ls_x,
        lstick_y: ls_y,
        rstick_x: rs_x,
        rstick_y: rs_y,
        ..Default::default()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Personality { Blinky, Pinky, Inky, Clyde }

struct Ghost {
    pos: (i32, i32),
    last_pos: (i32, i32),
    under_tile: &'static str,
    personality: Personality,
    home: (i32, i32),
}

const WALL: &str = "\x1B[38;5;75m▓\x1B[0m";
const FOOD: &str = "\x1B[38;5;214m•\x1B[0m";
const POWER_PELLET: &str = "\x1B[38;5;226m○\x1B[0m";
const EMPTY: &str = " ";
const PACMAN: &str = "\x1B[38;5;226m◉\x1B[0m";
const GHOST_BLINKY: &str = "\x1B[38;5;160mᗣ\x1B[0m"; // Red
const GHOST_PINKY: &str = "\x1B[38;5;213mᗣ\x1B[0m";  // Pink
const GHOST_INKY: &str = "\x1B[38;5;39mᗣ\x1B[0m";   // Cyan
const GHOST_CLYDE: &str = "\x1B[38;5;214mᗣ\x1B[0m";  // Orange
const VULNERABLE_GHOST: &str = "\x1B[38;5;33mᗣ\x1B[0m"; // Blue

fn print_maze(maze: &[[&str; 30]; 14], score: u32, lives: u32, power_time: u64, state: &GamepadState, c_type: ControllerType) {
    let title = if c_type == ControllerType::Xbox { "Xbox Controller" } else { "PS5 Controller" };
    println!("Connected to {}\x1B[K", title.cyan().bold());
    
    // Joystick percent visualization
    let ls_x_pct = (state.lstick_x * 100.0) as i32;
    let ls_y_pct = (state.lstick_y * 100.0) as i32;
    let rs_x_pct = (state.rstick_x * 100.0) as i32;
    let rs_y_pct = (state.rstick_y * 100.0) as i32;
    let lt_pct = (state.lt * 100.0) as i32;
    let rt_pct = (state.rt * 100.0) as i32;

    println!("LS X: {:>3}%        LS Y: {:>3}%\x1B[K", ls_x_pct, ls_y_pct);
    println!("RS X: {:>3}%        RS Y: {:>3}%\x1B[K", rs_x_pct, rs_y_pct);
    println!("LT  : {:>3}%        RT  : {:>3}%\x1B[K", lt_pct, rt_pct);
    
    // Buttons Row
    let btn_a = if state.a { "A".green().bold() } else { "A".truecolor(100,100,100) };
    let btn_b = if state.b { "B".red().bold() } else { "B".truecolor(100,100,100) };
    let btn_x = if state.x { "X".blue().bold() } else { "X".truecolor(100,100,100) };
    let btn_y = if state.y { "Y".yellow().bold() } else { "Y".truecolor(100,100,100) };
    let btn_u = if state.up { "Up".green() } else { "Up".truecolor(100,100,100) };
    let btn_d = if state.down { "Down".green() } else { "Down".truecolor(100,100,100) };
    let btn_l = if state.left { "Left".green() } else { "Left".truecolor(100,100,100) };
    let btn_r = if state.right { "Right".green() } else { "Right".truecolor(100,100,100) };

    println!("{} {} {} {} {} {} {} {}\x1B[K\n", btn_a, btn_b, btn_x, btn_y, btn_u, btn_d, btn_l, btn_r);

    println!("{}", "=== PACMAN WASM PS5 EDITION ===".yellow().bold());
    let power_status = if power_time > 0 {
        format!(" | POWER: {}s", power_time).yellow().bold()
    } else {
        "".clear()
    };
    print!("Score: {} | Lives: {}{}\x1B[K\n", score.to_string().cyan(), lives.to_string().red(), power_status);
    println!("{}", "-------------------------------".blue());
    for row in maze.iter() {
        for cell in row.iter() {
            print!("{}", cell);
        }
        println!();
    }
}

#[derive(Clone, Copy, PartialEq)]
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
            WALL, POWER_PELLET, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD,
            WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, FOOD, POWER_PELLET,
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
            WALL, WALL, EMPTY, FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, FOOD,
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
            EMPTY, WALL,
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
            WALL, POWER_PELLET, WALL, WALL, WALL, WALL, FOOD, EMPTY, FOOD, FOOD, WALL, WALL, FOOD, FOOD,
            FOOD, FOOD, FOOD, FOOD, WALL, WALL, FOOD, FOOD, FOOD, FOOD, WALL, WALL, WALL, WALL,
            POWER_PELLET, WALL,
        ],
        [WALL; 30],
    ];

    let mut ghosts = vec![
        Ghost { pos: (5, 16), last_pos: (5, 16), under_tile: FOOD, personality: Personality::Blinky, home: (5, 16) },
        Ghost { pos: (7, 28), last_pos: (7, 28), under_tile: EMPTY, personality: Personality::Pinky, home: (7, 28) },
        Ghost { pos: (5, 2), last_pos: (5, 2), under_tile: FOOD, personality: Personality::Clyde, home: (5, 2) },
    ];

    for g in ghosts.iter() {
        let (r, c) = (g.pos.0 as usize, g.pos.1 as usize);
        maze[r][c] = match g.personality {
            Personality::Blinky => GHOST_BLINKY,
            Personality::Pinky => GHOST_PINKY,
            Personality::Inky => GHOST_INKY,
            Personality::Clyde => GHOST_CLYDE,
        };
    }

    let mut current_pos = (6, 11);
    let mut buf = [0u8; 64]; 
    let mut score = 0;
    let mut lives = 3;
    let mut power_timer = Duration::from_secs(0);
    
    let mut last_move = std::time::Instant::now();
    let mut last_frame = std::time::Instant::now();
    let move_delay = Duration::from_millis(150);
    
    // Timing for ghosts
    let mut last_ghost_move = std::time::Instant::now();
    let ghost_delay = Duration::from_millis(400);

    let mut last_processed_state = GamepadState::default();
    print!("\x1B[2J\x1B[H"); // Clear screen and home

    loop {
        // We use a short timeout so that the game loop stays active for ghost movement
        match handle.read_interrupt(endpoint_in, &mut buf, Duration::from_millis(50)) {
            Ok(bytes_read) => {
                let state = if controller_type == ControllerType::Xbox {
                    parse_xbox_controller_data(&buf[0..bytes_read])
                } else {
                    parse_ps5_controller_data(&buf[0..bytes_read])
                };

                last_processed_state = state;
                let now = std::time::Instant::now();
                
                // Pacman Movement
                if now.duration_since(last_move) > move_delay {
                    let mut dy = 0;
                    let mut dx = 0;

                    if state.up || state.lstick_y < -0.5 { dy = -1; }
                    else if state.down || state.lstick_y > 0.5 { dy = 1; }
                    else if state.left || state.lstick_x < -0.5 { dx = -1; }
                    else if state.right || state.lstick_x > 0.5 { dx = 1; }

                    if dy != 0 || dx != 0 {
                        let new_y = (current_pos.0 as i32 + dy) as usize;
                        let new_x = (current_pos.1 as i32 + dx) as usize;

                        if maze[new_y][new_x] != WALL {
                            if maze[new_y][new_x] == FOOD {
                                score += 10;
                            } else if maze[new_y][new_x] == POWER_PELLET {
                                score += 50;
                                power_timer = Duration::from_secs(10);
                            } else if maze[new_y][new_x] == GHOST_BLINKY || maze[new_y][new_x] == GHOST_PINKY || maze[new_y][new_x] == GHOST_INKY || maze[new_y][new_x] == GHOST_CLYDE {
                                lives -= 1;
                                maze[current_pos.0][current_pos.1] = EMPTY;
                                current_pos = (6, 11);
                                maze[current_pos.0][current_pos.1] = PACMAN;
                                // Reset all ghosts to home on death
                                for g in ghosts.iter_mut() {
                                    maze[g.pos.0 as usize][g.pos.1 as usize] = g.under_tile;
                                    g.pos = g.home;
                                    g.last_pos = g.home;
                                    g.under_tile = maze[g.pos.0 as usize][g.pos.1 as usize];
                                }
                            } else if maze[new_y][new_x] == VULNERABLE_GHOST {
                                score += 200;
                                // Find and reset the specific caught ghost
                                for g in ghosts.iter_mut() {
                                    if (g.pos.0 as usize, g.pos.1 as usize) == (new_y, new_x) {
                                        g.pos = g.home;
                                        g.last_pos = g.home;
                                        g.under_tile = EMPTY;
                                        break;
                                    }
                                }
                            }
                            
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos = (new_y, new_x);
                            maze[current_pos.0][current_pos.1] = PACMAN;
                            last_move = now;
                        }
                    }
                }
            }
            Err(rusb::Error::Timeout) => {
                // Timeouts are fine, we just want to check ghost movement
            }
            Err(e) => {
                return Err(anyhow!("USB Read Error: {}", e));
            }
        }

        // Update power timer
        let loop_now = std::time::Instant::now();
        let frame_delta = loop_now.duration_since(last_frame);
        last_frame = loop_now;

        if power_timer > Duration::from_secs(0) {
            if power_timer > frame_delta {
                power_timer -= frame_delta;
            } else {
                power_timer = Duration::from_secs(0);
            }
        }

        // Ghost Movement
        let is_frightened = power_timer > Duration::from_secs(0);
        let current_ghost_delay = if is_frightened { Duration::from_millis(800) } else { ghost_delay };

        if loop_now.duration_since(last_ghost_move) > current_ghost_delay {
            // 1. Clear all ghosts from maze first to prevent overlap issues
            for g in ghosts.iter_mut() {
                maze[g.pos.0 as usize][g.pos.1 as usize] = g.under_tile;
            }

            // 2. Calculate and move each ghost
            let mut collision_occurred = false;
            for i in 0..ghosts.len() {
                let mut target_tile; 

                // A. Calculate Target
                if is_frightened {
                    let pseudo_rand = (loop_now.duration_since(last_move).as_millis() as usize) + i;
                    target_tile = ( (pseudo_rand % 14) as i32, (pseudo_rand % 30) as i32 );
                } else {
                    match ghosts[i].personality {
                        Personality::Blinky => {
                            target_tile = (current_pos.0 as i32, current_pos.1 as i32);
                        }
                        Personality::Pinky => {
                            target_tile = (current_pos.0 as i32, current_pos.1 as i32);
                            target_tile.0 += 2;
                            target_tile.1 += 2;
                        }
                        Personality::Inky => {
                            let blinky_pos = ghosts.iter().find(|g| g.personality == Personality::Blinky).map(|g| g.pos).unwrap_or((0,0));
                            let pac_pos = (current_pos.0 as i32, current_pos.1 as i32);
                            target_tile = (pac_pos.0 + (pac_pos.0 - blinky_pos.0), pac_pos.1 + (pac_pos.1 - blinky_pos.1));
                        }
                        Personality::Clyde => {
                            let dist_sq = (ghosts[i].pos.0 - current_pos.0 as i32).pow(2) + (ghosts[i].pos.1 - current_pos.1 as i32).pow(2);
                            if dist_sq > 64 { target_tile = (current_pos.0 as i32, current_pos.1 as i32); }
                            else { target_tile = (13, 0); }
                        }
                    }
                }

                // B. Decide Direction (Prevent 180 and pick closest to target)
                let dirs = [(0, 1), (0, -1), (1, 0), (-1, 0)];
                let mut best_dir = None;
                let mut min_dist = f32::MAX;

                for &(dr, dc) in dirs.iter() {
                    let nr = ghosts[i].pos.0 + dr;
                    let nc = ghosts[i].pos.1 + dc;

                    if nr >= 0 && nr < 14 && nc >= 0 && nc < 30 {
                        if maze[nr as usize][nc as usize] != WALL && (nr, nc) != ghosts[i].last_pos {
                            let dist = (((nr - target_tile.0).pow(2) + (nc - target_tile.1).pow(2)) as f32).sqrt();
                            if dist < min_dist {
                                min_dist = dist;
                                best_dir = Some((nr, nc));
                            }
                        }
                    }
                }

                if let Some((nr, nc)) = best_dir {
                    if (nr as usize, nc as usize) == (current_pos.0, current_pos.1) {
                        if is_frightened {
                            score += 200;
                            ghosts[i].pos = ghosts[i].home;
                            ghosts[i].last_pos = ghosts[i].home;
                            ghosts[i].under_tile = EMPTY;
                        } else {
                            lives -= 1;
                            maze[current_pos.0][current_pos.1] = EMPTY;
                            current_pos = (6, 11);
                            maze[current_pos.0][current_pos.1] = PACMAN;
                            collision_occurred = true;
                            // Reset ALL ghosts on collision later
                            break; 
                        }
                    } else {
                        ghosts[i].last_pos = ghosts[i].pos;
                        ghosts[i].pos = (nr, nc);
                        ghosts[i].under_tile = maze[nr as usize][nc as usize];
                    }
                }
            }

            if collision_occurred {
                // Reset all ghosts to home
                for g in ghosts.iter_mut() {
                    g.pos = g.home;
                    g.last_pos = g.home;
                    g.under_tile = maze[g.pos.0 as usize][g.pos.1 as usize];
                }
            }

            // 3. Update all ghosts visuals in maze after movements are finalized
            for i in 0..ghosts.len() {
                let (r, c) = (ghosts[i].pos.0 as usize, ghosts[i].pos.1 as usize);
                maze[r][c] = if is_frightened { VULNERABLE_GHOST } else {
                    match ghosts[i].personality {
                        Personality::Blinky => GHOST_BLINKY,
                        Personality::Pinky => GHOST_PINKY,
                        Personality::Inky => GHOST_INKY,
                        Personality::Clyde => GHOST_CLYDE,
                    }
                };
            }
            last_ghost_move = loop_now;
        }

        // Win condition check
        let mut food_left = false;
        for r in 0..14 {
            for c in 0..30 {
                if maze[r][c] == FOOD || maze[r][c] == POWER_PELLET {
                    food_left = true;
                    break;
                }
            }
        }
        if !food_left {
            print!("\x1B[H");
            print_maze(&maze, score, lives, power_timer.as_secs(), &last_processed_state, controller_type);
            println!("\n{}", "YOU WIN! ALL PELLETS CONSUMED!".green().bold());
            break;
        }

        // Render Frame
        print!("\x1B[H"); // Home cursor
        print_maze(&maze, score, lives, power_timer.as_secs(), &last_processed_state, controller_type);
        io::stdout().flush()?;

        if lives == 0 {
            println!("{}", "GAME OVER!".red().bold());
            break;
        }
    }
    Ok(())
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

