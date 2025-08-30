// modbuild/src/main.rs

mod build;
mod crate_info;
mod utils;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Cross-platform mod builder
#[derive(Parser)]
#[command(
  name = "modbuild",
  version,
  about = "Cross-platform mod builder for Freven"
)]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Build mods for one or more targets
  Build {
    /// Path to the mod project
    #[arg(short, long, default_value = ".")]
    path: PathBuf,

    /// Output directory
    #[arg(short, long, default_value = "dist")]
    out: PathBuf,

    /// Comma-separated list of targets (linux, windows-gnu, windows-msvc, mac-intel, mac-arm64)
    #[arg(short, long)]
    targets: Option<String>,
  },

  /// List available targets
  ListTargets,
}

fn main() {
  let cli = Cli::parse();

  match cli.command {
    Commands::Build { path, out, targets } => {
      let path = std::fs::canonicalize(&path).unwrap_or(path);

      let targets = build::select_targets(targets);

      if let Err(e) = crate_info::ensure_cdylib(&path) {
        eprintln!("Invalid mod at {}: {e}", path.display());
        std::process::exit(1);
      }

      for target in targets {
        if let Err(e) = build::build_for_target(&out, &target, &path) {
          eprintln!("{e}");
        }
      }
    }
    Commands::ListTargets => {
      println!("Available targets:");
      for t in build::all_targets() {
        println!(" - {} ({})", t.name, t.triple);
      }
    }
  }
}
