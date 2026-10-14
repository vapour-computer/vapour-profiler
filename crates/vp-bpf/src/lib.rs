// SPDX-License-Identifier: MIT OR Apache-2.0
//! BPF objects are compiled and embedded at build time; importing this crate
//! does not open, load, or attach them. Only an explicit capture does that.

#[allow(clippy::all)]
#[allow(dead_code)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/probe.skel.rs"));
}

pub use generated::ProbeSkelBuilder;
