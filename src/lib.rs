#![doc = include_str!("../README.md")]
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
