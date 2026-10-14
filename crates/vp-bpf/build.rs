// SPDX-License-Identifier: MIT OR Apache-2.0
use std::{env, path::PathBuf};

fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "native profiling requires Linux"
    );
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ARCH").unwrap(),
        "x86_64",
        "only x86_64 is supported initially"
    );
    println!("cargo:rerun-if-changed=bpf/probe.bpf.c");
    println!("cargo:rerun-if-env-changed=CLANG");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut builder = libbpf_cargo::SkeletonBuilder::new();
    builder.source("bpf/probe.bpf.c");
    let mut clang_args = vec!["-Wall".to_owned(), "-Werror".to_owned()];
    // Debian-family distributions keep asm/ UAPI headers in a multiarch
    // directory that clang's BPF target does not search automatically.
    let multiarch = PathBuf::from("/usr/include/x86_64-linux-gnu");
    if multiarch.is_dir() {
        clang_args.push(format!("-I{}", multiarch.display()));
    }
    builder.clang_args(clang_args);
    if let Some(clang) = env::var_os("CLANG") {
        builder.clang(PathBuf::from(clang));
    }
    builder.build_and_generate(out.join("probe.skel.rs"))
        .expect("BPF compilation failed: install clang with the BPF target, Linux headers, libelf and zlib development packages");
}
