//! # eventsourcingdb
//!
//! The official Rust client SDK for [EventSourcingDB](https://www.eventsourcingdb.io) – a purpose-built database for event sourcing.
//! EventSourcingDB enables you to build and operate event-driven applications with native support for writing, reading, and observing events. This client SDK provides convenient access to its capabilities in Rust.
//! For more information on EventSourcingDB, see its [official documentation](https://docs.eventsourcingdb.io/).
//! This client SDK includes support for [Testcontainers](https://testcontainers.com/) to spin up EventSourcingDB instances in integration tests. For details, see [Using Testcontainers](#using-testcontainers).
//!
//! ## Getting Started
//!
//! Install the client SDK:
//!
//! ```shell
//! cargo add eventsourcingdb
//! ```
//!
//! Import the package and create an instance by providing the URL of your EventSourcingDB instance and the API token to use:
//!
//! ```rust
//! use eventsourcingdb::client::Client;
//! # use url::Url;
//! // ...
//!
//! let base_url: Url = "localhost:3000".parse().unwrap();
//! let api_token = "secret";
//! let client = Client::new(base_url, api_token);
//! ```
//!
//! The client works with servers that offer the v2 API, and with servers that do not, which it finds out on its first
//! request. The v2 API reads with the HTTP method `QUERY` (RFC 10008), so a proxy in front of EventSourcingDB has to
//! let it through.
//!
//! ## Examples
//!
//! Examples can be found in the [examples](https://github.com/thenativeweb/eventsourcingdb-client-rust/tree/main/examples) directory.
//!
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
