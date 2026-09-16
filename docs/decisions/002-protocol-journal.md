# ADR 002 — one journal for Core and Execution

- Status: **accepted implementation choice**, within the owner's two permitted alternatives after review of PR #4 at `fa70a63`; not milestone acceptance.
- Date: 2026-09-16. Owner: Codex. Scope: PIO-local persistence; no Protocol schema changes.
- Authority: [ADR 001](001-standalone-stack.md), [PLAN](../work/standalone-0.1/PLAN.md), owner instruction to settle persistence before Execution.

## Decision and reason

Move Core persistence onto the `pio-core` append-only journal before adding Execution. Core subjects, grants, dedupe outcomes, effects, event positions and retained events become rebuildable read projections in the same SQLite journal database. A single writer commits the changed records, journal facts and outbox entries in one transaction. Execution admission and effect intent will use that same transaction path; only committed intent may reach the labeled fake host.

The alternative was retaining the current whole-state JSON store as a conformance-only implementation beside a separate product journal. That is permitted but would duplicate admission semantics and leave authorization, command replay and execution intent across separate transaction boundaries. Moving the verified Core semantics now avoids building Execution on that split.

The journal contains record-level changes, including deletions from retained projections, rather than snapshots of the entire provider. Replaying those changes rebuilds the protocol projections. Memory is a read/staging cache, never the recovery authority; rollback reloads committed projections. Events have durable individual records. The first migration may still scan cached records to find changed projections; this is an explicit performance limit, not a second source of truth. No claim of bounded total memory or production throughput follows from conformance.

Existing experimental `protocol.sqlite3` blob stores are not silently imported or ignored. Refuse them with an explicit fresh-store/migration requirement. Clean conformance launches need no compatibility migration; a future supported upgrade must define and verify one before changing user stores.

## Service and host boundary

The mutex-guarded provider and 25 ms per-connection polling are a **conformance service loop**, not the durable-host design. The durable host owns process identity, child pipes and launch/release evidence independently of a client socket. Socket closure must not cancel admitted work. Scripted execution is explicitly fake; it proves only the selected Protocol and recovery properties. No real adapter or native authentication claim is added.

## Verification and consequences

Before Execution: verify atomic journal/projection/outbox commits, rebuild equivalence, rollback and restart replay; rerun the existing Core fixtures. Then implement `executor.script`, the five PLAN Execution features, public effects after effect-producing fixtures run, and backpressure in that order. Retain native runner classes and per-directory coverage; unsupported controls remain explicit. Record each tested base/head in [STATE](../work/STATE.md).

This supersedes the Core checkpoint's single-row blob persistence as an implementation direction. Historical `fa70a63` evidence still describes that stopgap accurately. The durable-host design in ADR 001 remains authoritative.
