/// The encoding/1 value domain: the strict parse the service applies to
/// every frame, and the canonical form commands are digested in.
pub mod encoding;
mod provider;
mod schemas;
mod stream;
pub use stream::{serve, serve_claude, serve_codex, serve_fake, serve_opencode};
mod claude;
mod codex;
mod durable;

mod effects;
mod events;
mod grants;
#[cfg(test)]
mod tests;

mod persistence;

mod execution;

mod output;

pub mod transcript;

pub mod capacity;
