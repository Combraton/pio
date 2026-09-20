//! Claude Code's entry in the shared provider.
//!
//! There is no second provider. Both hosts emit the same normalized events, so
//! what differs for Claude Code — its delivery proof and evidence class, the
//! usage measure, the decision vocabulary, how cancel is really performed, and
//! the few event names that could not be shared without renaming the events
//! Codex's committed M2 receipts already use — lives in [`crate::codex::PROFILES`].
//!
//! See ADR 004 and `docs/work/m3/SERVICE-BINDING.md`.
