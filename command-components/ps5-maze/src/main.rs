// Copyright (c) 2026 IDLab Discover
// SPDX-License-Identifier: MIT

fn main() {
    if let Err(e) = ps5_maze::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
