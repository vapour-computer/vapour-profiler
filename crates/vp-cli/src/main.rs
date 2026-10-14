// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    version,
    about = "Standalone local profiler — step 1 BPF lifecycle proof"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Explicitly load a bounded test uprobe; stop on TTL, target exit or signal.
    ProbeTest {
        #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
        pid: i32,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        symbol: String,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=300))]
        ttl: u64,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::ProbeTest {
            pid,
            binary,
            symbol,
            ttl,
        } => vp_agent::probe_test(vp_agent::ProbeTest {
            pid,
            binary,
            symbol,
            ttl: Duration::from_secs(ttl),
        }),
    }
}
