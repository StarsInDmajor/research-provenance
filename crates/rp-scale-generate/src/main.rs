use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use rp_scale_generate::{GenerateOptions, Profile, generate};

#[derive(Debug, Parser)]
#[command(
    name = "rp-scale-generate",
    about = "Generate deterministic RP scale projects"
)]
struct Cli {
    #[arg(long, default_value = "smoke", value_parser = ["smoke", "workstation", "stress"])]
    profile: String,
    #[arg(long, default_value_t = 20_260_830)]
    seed: u64,
    #[arg(long)]
    output: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let profile = Profile::parse(&cli.profile).expect("Clap validates profile values");
    match generate(GenerateOptions {
        profile,
        seed: cli.seed,
        output: cli.output,
    }) {
        Ok(manifest) => {
            println!("{}", manifest.aggregate_sha256);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("rp-scale-generate: {error}");
            ExitCode::from(1)
        }
    }
}
