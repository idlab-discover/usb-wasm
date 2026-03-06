fn main() {
    if let Err(e) = ps5_maze::run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
