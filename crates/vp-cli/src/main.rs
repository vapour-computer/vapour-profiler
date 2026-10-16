// SPDX-License-Identifier: MIT OR Apache-2.0
use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about = "Standalone local profiler and performance dashboard")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Open an explicit local dashboard session. Nothing runs before this command.
    Dashboard {
        /// Only this locally selected PID may be captured from the dashboard.
        #[arg(long, value_parser = clap::value_parser!(i32).range(1..))]
        pid: Option<i32>,
        /// Also count the PID's child processes (CPU and memory summed). For browsers, whose
        /// game work runs in renderer and GPU child processes.
        #[arg(long, requires = "pid")]
        tree: bool,
        /// Start native capture immediately instead of waiting for the Start button. Explicit
        /// opt-in; used by `vapour test`.
        #[arg(long, requires = "pid")]
        autostart: bool,
        #[arg(long, default_value_t = 6767)]
        port: u16,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=300))]
        ttl: u64,
    },
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
    /// Build a single JSON report from a .vpt trace.
    Report {
        /// Input trace path (.vpt / ndjson)
        trace: PathBuf,
        /// Output file path (stdout when omitted)
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
        /// Omit the raw trace array from output JSON
        #[arg(long)]
        no_trace: bool,
        /// Pretty-print JSON output
        #[arg(long)]
        pretty: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Dashboard {
            pid,
            tree,
            autostart,
            port,
            ttl,
        } => tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?
            .block_on(vp_agent::dashboard::serve(pid, tree, autostart, port, ttl)),
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
        Command::Report {
            trace,
            output,
            no_trace,
            pretty,
        } => {
            let text = std::fs::read_to_string(&trace)?;
            let records = vp_analyze::parse(&text)?;
            let value = vp_analyze::report(
                &records,
                vp_analyze::ReportOptions {
                    include_trace: !no_trace,
                },
            );
            let out = if pretty {
                serde_json::to_string_pretty(&value)?
            } else {
                serde_json::to_string(&value)?
            };
            if let Some(path) = output {
                std::fs::write(path, out)?;
            } else {
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(out.as_bytes())?;
                stdout.write_all(b"\n")?;
            }
            Ok(())
        }
    }
}
