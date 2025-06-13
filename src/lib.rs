#![doc = include_str!("../README.md")]
// There is a known bug in clippy:
// https://github.com/rust-lang/rust-clippy/issues/12908
#![allow(clippy::needless_lifetimes)]
#![deny(
    ambiguous_negative_literals,
    clippy::pedantic,
    missing_debug_implementations,
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    unused_import_braces,
    unused_qualifications,
    unused_results,
    unsafe_code,
    warnings
)]

pub mod client;
#[cfg(feature = "testcontainer")]
pub mod container;
pub mod error;
pub mod event;

pub use client::{Client, Precondition, request_options};
pub use event::{Event, EventCandidate, ManagementEvent, TraceInfo};
