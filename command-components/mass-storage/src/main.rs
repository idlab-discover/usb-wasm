use clap::{Parser, Subcommand};
use mass_storage::{benchmark, cat, ls, tree};
use tracing::Level;

use anyhow::anyhow;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Number of times to greet
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Tree { path: Vec<String> },
    Ls { path: Vec<String> },
    Cat { path: Vec<String> },
    Benchmark,
    // TODO: Copy
}

fn vec_to_opt_str(vec: Vec<String>) -> Option<String> {
    if vec.is_empty() {
        None
    } else {
        Some(vec.join(" "))
    }
}

pub fn main() -> anyhow::Result<()> {
    // Set up logging
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let args = Args::parse();

    match args.command {
        Command::Tree { path } => tree(vec_to_opt_str(path))?,
        Command::Ls { path } => ls(vec_to_opt_str(path))?,
        Command::Cat { path } => cat(vec_to_opt_str(path).ok_or(anyhow!("No file specified"))?)?,
        Command::Benchmark => benchmark(1)?
        // _ => todo!("Command not implemented"),
    }
    // benchmark_raw_speed(1, 8, 1)?;

    // ls(Some("System Volume Information".into()))?;
    // cat(fat_slice)?;
    // write(fat_slice, "hello.txt", b"Hello USB!\n")?;
    // write(fat_slice, "hello2.txt", b"Hello USB2!\n")?;

    Ok(())
}
