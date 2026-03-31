// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

fn main() {
    if let Err(e) = xbox_maze::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
