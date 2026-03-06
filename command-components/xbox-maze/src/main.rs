fn main() {
    if let Err(e) = xbox_maze::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
