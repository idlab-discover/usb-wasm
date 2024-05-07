pub fn main() {
    // Read from stdin
    println!("Press enter to continue...");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
