use clap::{Parser, Subcommand};
use mass_storage::{benchmark, benchmark_raw_speed, cat, ls, tree};
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
    Tree {
        path: Vec<String>,
    },
    Ls {
        path: Vec<String>,
    },
    Cat {
        path: Vec<String>,
    },
    Benchmark {
        megabytes: usize,
    },
    RawBenchmark {
        seq_megabytes: usize,
        rnd_megabytes: usize,
    },
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
        Command::Benchmark { megabytes } => benchmark(megabytes)?,
        Command::RawBenchmark {
            seq_megabytes,
            rnd_megabytes,
        } => benchmark_raw_speed(1, seq_megabytes, rnd_megabytes)?,
    }

    Ok(())
}
