// SPDX-License-Identifier: MIT OR Apache-2.0
//! Foreground attach/detach proof. The full session lifecycle follows in step 2.

use std::{
    fs::{self, File},
    io::{self, Write},
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use libbpf_rs::{
    skel::{OpenSkel, SkelBuilder},
    MapCore, MapFlags, UprobeOpts,
};
use signal_hook::{
    consts::{SIGINT, SIGTERM},
    SigId,
};
use vp_bpf::ProbeSkelBuilder;

/// Explicit local-only test target; never populated from a URL or remote input.
pub struct ProbeTest {
    pub pid: i32,
    pub binary: PathBuf,
    pub symbol: String,
    pub ttl: Duration,
}

struct Signals(Vec<SigId>);

impl Drop for Signals {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

fn pidfd(pid: i32) -> Result<OwnedFd> {
    // SAFETY: pidfd_open takes scalar arguments and returns a new owned fd.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error()).context("cannot open target pidfd");
    }
    // SAFETY: the successful syscall returned a fresh descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
}

fn target_exited(fd: &OwnedFd, timeout: Duration) -> Result<bool> {
    let mut pollfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pollfd is valid for one element; timeout is bounded by one second.
    let timeout_ms = timeout.as_nanos().div_ceil(1_000_000).min(1000) as i32;
    let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error).context("poll target pidfd");
    }
    Ok(result > 0)
}

fn same_file(a: &Path, b: &Path) -> Result<bool> {
    let a = fs::metadata(a).context("read requested binary metadata")?;
    let b = fs::metadata(b).context("read target executable metadata")?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}

/// Load exactly one bounded map and one uprobe, count hits, then drop all FDs.
/// No socket, daemon, pins, runtime downloads, or background worker is created.
pub fn probe_test(config: ProbeTest) -> Result<()> {
    if config.pid <= 0 {
        bail!("pid must be positive; system-wide attachment is forbidden");
    }
    if config.ttl < Duration::from_secs(1) || config.ttl > Duration::from_secs(300) {
        bail!("probe-test TTL must be between 1 and 300 seconds");
    }
    if config.symbol.is_empty() || config.symbol.len() > 512 || config.symbol.contains('\0') {
        bail!("symbol must contain 1–512 bytes without NUL");
    }
    if !Path::new("/sys/kernel/btf/vmlinux").exists() {
        bail!("kernel BTF is required at /sys/kernel/btf/vmlinux (Linux 5.15+)");
    }
    let target = pidfd(config.pid)?;
    let exe = PathBuf::from(format!("/proc/{}/exe", config.pid));
    if !same_file(&config.binary, &exe)? {
        bail!("--binary must be the target process's executable for probe-test");
    }
    // Hold the executable inode across load/attach, rather than reopening a
    // potentially replaced pathname when libbpf resolves the symbol.
    let binary = File::open(&exe).context("open target executable")?;
    let attach_path = PathBuf::from(format!("/proc/self/fd/{}", binary.as_raw_fd()));
    if target_exited(&target, Duration::ZERO)? {
        bail!("target already exited");
    }

    let stop = Arc::new(AtomicBool::new(false));
    let mut signals = Signals(Vec::new());
    for signal in [SIGINT, SIGTERM] {
        signals
            .0
            .push(signal_hook::flag::register(signal, Arc::clone(&stop))?);
    }

    // Declaration order and RAII ensure error paths also close the link first,
    // then every program/map descriptor, before returning to the CLI.
    let mut storage = MaybeUninit::uninit();
    let builder = ProbeSkelBuilder::default();
    let mut open = builder
        .open(&mut storage)
        .context("open embedded BPF skeleton")?;
    open.maps
        .rodata_data
        .as_deref_mut()
        .context("missing BPF configuration")?
        .target_pid = config.pid as u32;
    let skel = open.load().context(
        "load BPF: requires Linux 5.15+, BTF and session CAP_BPF + CAP_PERFMON; kernel lockdown/security policy may also restrict BPF",
    )?;
    if stop.load(Ordering::Relaxed) || target_exited(&target, Duration::ZERO)? {
        bail!("capture cancelled or target exited before attachment");
    }
    let link = skel
        .progs
        .vp_probe
        .attach_uprobe_with_opts(
            config.pid,
            &attach_path,
            0,
            UprobeOpts {
                func_name: Some(config.symbol),
                ..Default::default()
            },
        )
        .context("attach uprobe: check the ELF symbol and perf/ptrace permissions")?;
    let start = Instant::now();
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "attached pid={} ttl={}s",
        config.pid,
        config.ttl.as_secs()
    )?;
    output.flush()?;
    let mut hits = 0_u64;
    let mut next_read = start + Duration::from_secs(1);
    let deadline = start + config.ttl;
    let reason = loop {
        if stop.load(Ordering::Relaxed) {
            break "signal";
        }
        let now = Instant::now();
        if now >= deadline {
            break "ttl";
        }
        if target_exited(
            &target,
            deadline.min(next_read).saturating_duration_since(now),
        )? {
            break "target-exit";
        }
        if stop.load(Ordering::Relaxed) || Instant::now() < next_read {
            continue;
        }
        let values = skel
            .maps
            .vp_hits
            .lookup_percpu(&0_u32.to_ne_bytes(), MapFlags::ANY)?
            .context("missing hit counter")?;
        hits = values
            .iter()
            .map(|v| {
                let bytes: [u8; 8] = v
                    .as_slice()
                    .try_into()
                    .context("unexpected hit counter size")?;
                Ok(u64::from_ne_bytes(bytes))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .sum();
        writeln!(output, "hits={hits}")?;
        output.flush()?;
        next_read = Instant::now() + Duration::from_secs(1);
    };
    drop(link);
    drop(skel);
    writeln!(output, "detached reason={reason} hits={hits}")?;
    Ok(())
}
