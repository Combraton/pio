//! PIO's public client: a typed client for Protocol core/1 and execution/1
//! over the authenticated local Unix socket (stream/1).
//!
//! **Public wire only.** This crate depends on no PIO service crate — not the
//! store, not the provider, not a host — so anything built on it (the `pio
//! client` commands and the M4 terminal screen) reads and writes only
//! through the public API. `tests/dependencies.rs` holds that line.
pub mod blocks;
pub mod board;
pub mod client;
pub mod replay;
pub mod walk;
pub mod wire;

pub use client::{
    CONTENT, Client, Command, Fence, Options, OutputChunk, Pinned, Position, Reconcile,
    decision_word, execution,
};
pub use wire::{Credential, Failure, Refusal, Reply, canonical, digest};
