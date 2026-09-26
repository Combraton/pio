//! `pio client`: the command line over PIO's public API, and the caller
//! ledger behind `submit` and `reconcile`.
//!
//! A library of its own so that the only PIO crate it can name is
//! `pio_client`: pio-cli, which links the service, calls [`run`] and
//! nothing else. `crates/pio-boundary` holds the line.
mod client_cli;
mod ledger;

pub use client_cli::{USAGE, run};
