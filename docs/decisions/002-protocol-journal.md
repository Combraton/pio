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

## Public process host and remaining commit cost (2026-09-16)

The owner accepted this direction at `7a4659f`. Public `serve-fake` now uses the same journal database for Protocol intent and the durable host's invocation facts. The Protocol writer commits command acknowledgment, admission and non-repeatable effect intent before calling host admission. Host admission is a subsequent transaction with a deterministic execution binding; a crash between them is reconciled without inventing a new execution. SQLite serializes writer transactions; the protocol lifetime lock fences competing service caches, while the host slot lock and controller witness fence process ownership. Host processes write their own phase facts through the same Store implementation. There is no cross-transaction atomicity claim between admission and OS spawn.

The **commit path still serializes and diffs the whole retained provider state per command**. Record-level journal facts do not make that algorithm incremental. Before M2, impose hard admission/commit limits of **32 MiB canonical projected state and 32,768 projection records**, including events and dedupe outcomes, checked before staging/commit; document explicit capacity refusal and retention rules, then test the exact boundary. Until that gate is implemented this remains a small-scale checkpoint, with no bound on total provider memory or store growth. Replace the scan with tracked changed keys before claiming higher throughput. Immutable spool GC must preserve references reachable from journal/recovery evidence; no automatic GC or total-disk bound is claimed here.

Output captures are immutable SHA-256 objects written and synced before committing digest/offset/length references. The host appends capture references separately from the service's read projection; partial captures remain independently recoverable. Script inputs also live in the spool, so captured output bytes never enter journal records. Missing/corrupt objects cause explicit unavailability, not fabricated empty output. Existing inline-output checkpoint stores require an explicit migration or a fresh directory; no silent import.

The caller ledger is a separate `caller.sqlite3`, at most 1,024 operations, with at most 1 MiB request and 64 KiB selected-basis data per operation. It commits exact request identity, selected basis and binding references before network I/O. Restart queries `execution.reconcile` before fetching an existing acknowledgment through exact command replay. An empty reconciliation remains pending; it never silently starts fresh work. Credential bytes are not stored. This ledger is not a workflow scheduler or a copy of execution facts.


## Recovery and ordering correction

The sequence oracle reads authoritative journal rows rather than projection order or wall-clock time. It requires the commit containing the delivery effect's `dispatch_intent` observation to precede the correlated host `invocation.intent`. The launch-config-only reorder mutant reverses these actual commits; its expected failure is `dispatch_intent_must_precede_invocation_intent`. The separate wrong-reason control must reject any other explanation. The cut immediately after the marker commit exits 95 before host admission. On restart, marker-present/no-host-intent is conservatively ambiguous, with no automatic admission or spawn; an empty process table is not authority to retry a non-repeatable effect.

Recovery and host-change events are persisted on the public process path. Public host generation follows the fenced controller generation; an already released child keeps the original slot/host generation and kernel start identity in the local witness. Known delivery is not erased by controller restart. Unresolved pending delivery becomes ambiguous and remains so until evidence reconciles it. A child release marker has its own `child_release_marker` evidence class; recovery classifications do not substitute for delivery proof. The frozen proof enum is unchanged. This proves receipt of the fake release message only.
