# Standalone PIO 0.1 — implementation readiness

Status: proposed for owner review, 2026-09-16. Owner: Codex, PIO implementation lead. This checkpoint authorizes no release and claims no working PIO runtime. The requested first deliverable is this plan, the [proposed stack](../../decisions/001-standalone-stack.md), the [journey matrix](../../JOURNEYS.md), and the [reconciled handoff](HANDOFF.md).

## Outcome and fixed boundaries

Ship an installable local execution service with CLI and first-class TUI for **both Codex and Claude Code**. It preserves each harness's native investigate/edit/test loop, provides inspectable execution and workspace identities, and recovers honestly after client or service interruption. A first Codex slice is a milestone, not the full release. Core work must succeed with CBR and Combraton absent.

Keep the accepted architecture: core execution authority; separate standalone caller policy; presentation with no authoritative execution state; immutable observations; invocation/effect intent before dispatch; fenced ownership; reconcile before retry; distinct delivery, result, verification and acceptance. There is no workflow engine, automatic model selection, global hook installation, private CBR import or shared writable store.

Protocol is now released. The [verified pin](../../../protocol.lock.json) is `v0.1.0`, commit `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. Moving main is not its replacement. See [consumer mapping](PROTOCOL.md) for the exact integration surface and release limitations.

## Proposed release boundary

| Area | Full 0.1 target | Explicit boundary |
|---|---|---|
| Platforms | macOS arm64 and Linux x86_64; exact CI images and minimum OS versions fixed during M1 packaging experiment | Only macOS 26.3.1 arm64 inspected here; Linux is untested. Windows, remote/multi-host operation and other architectures deferred pending owner agreement |
| Harnesses | Codex 0.146.0 and Claude Code 2.1.273 as initial exact validation candidates | Neither is supported yet. New versions require qualification; do not advertise an untested version range |
| Interfaces | Authenticated local `stream/1` Unix socket; CLI and TUI call the same public Core/Execution API | No HTTP service, desktop dependency or PTY scraping required. Public stdio proxy is a later optional binding unless needed by conformance |
| Work | Discover/configure, grant/scope, submit, inspect/watch/output, concurrent isolated workspaces, approvals, cancellation, supported steering/continuation, reconcile, checkpoint/export | Native features differ; unsupported capabilities are explicit. An opaque harness cannot supply guaranteed exactly-once remote effects or hard billed-dollar bounds |
| Lifecycle | User service plus durable execution hosts; client detach and daemon reconnect preserve host ownership and output | Host death and machine reboot are reconciliation cases, not promises to resume arbitrary in-flight model state |
| Persistence | Journal/outbox, deduplication, bounded spools, immutable receipts, usage liability, retention, backup/export/restore and versioned migration | No network-filesystem SQLite state; preserve unresolved effects and required evidence across cleanup |
| Optional CBR | Caller-selected packets, exact-byte verification, required/advisory enforcement, correction/outage handling, eligible evidence publication | Integration may be built against labeled fixtures before CBR is ready; real composition remains a separate unpassed gate |

The proposed platform and authentication scope need owner judgment. Library selection within an accepted stack can proceed through the bounded experiments below. Product licensing also remains an owner decision before distribution; no license is silently selected here.

## Milestones and acceptance

Each milestone ends with a reviewable diff, exact build identity, evidence and a short acceptance checkpoint. Routine fixes continue inside its scope. Changing the release boundary or an accepted invariant returns to the owner.

| Milestone | Deliverable | Acceptance evidence / dependencies |
|---|---|---|
| M0 — readiness | Reconciled sources, verified Protocol pin, proposed stack/scope, journey contracts | This documentation checkpoint; owner settles questions below. Documentation and integrity checks do not accept runtime behavior |
| M1 — core and durable host | Cargo workspace, local protocol service, journal/outbox/deduplication, caller operation store, fenced launch barrier, host spool, scoped discovery and packaging skeleton | Protocol Core/Execution fixtures for implemented surface; fake-host faults clearly labeled; no dispatch after journal failure; detach/reattach primitive; minimum OS/toolchain and dependency lock fixed. No real-adapter milestone claim |
| M2 — first real adapter | Codex native app-server behind a persistent host; CLI submits scoped repository work and records output/result | J1 and J5 with real Codex; J3/J4 for Codex; schema/version drift refusal, approval deny/allow, no weakened permissions. Authentication and chosen model recorded for live tests |
| M3 — second adapter | Claude native loop through a persistent SDK bridge or validated direct interface, with exact version and auth matrix | J1/J3/J4/J5 for real Claude; prove project instructions, permission configuration, native output and session semantics remain intact. A failure here holds full release; Codex-only is not silently accepted |
| M4 — terminal product | TUI workspace/execution navigation, output inspection, correlated approval responses, detach/reconnect and capability-aware actions | J2 with both real harnesses in independent workspaces; real terminal evidence and human usability review; deny wrong-execution approval and test narrow terminal/keyboard operation |
| M5 — optional composition | Public Context/Evidence client, immutable bindings, revalidation, publication outbox; no private stores | J6 using real CBR when available; fixtures cover faults earlier. Core no-CBR J5 rerun. Missing real CBR holds composition evidence, not independent M1–M4 work |
| M6 — release candidate | Fresh install/update, migration/restore barrier, retention/export, complete capability/support matrix, docs and comparative evidence | All required journeys for both adapters on each advertised platform; conformance coverage and exclusions; backup restore with surviving host, disk-full, stale owner and PID-reuse negative controls. Owner accepts; merge/tag/publish require separate explicit authorization |

## Bounded experiments

Each experiment gets at most one focused implementation/measurement cycle and one repair/retest before a checkpoint. An unresolved invariant becomes an explicit limitation or decision; it does not become an indefinite research project.

1. **M1 host lifetime:** run a labeled deterministic child that records invocation markers. Disconnect terminal, restart daemon, then kill host separately. Correlate process start identity and generations. Surviving-host path must not respawn; lost-host path must retain uncertainty. Force a duplicate-launch mutant and require the identity/count check to fail.
2. **M1 persistence:** kill at intent commit, host claim, prompt release and receipt publication; inject disk-full and stale store restore. Durable state must precede dispatch; unresolved effects keep their liability. Compare SQLite WAL/FULL plus a single writer with rollback journal only if deployment constraints defeat WAL.
3. **M2/M3 native fidelity:** real bounded repository edit/test, native permission deny/allow, steering, interrupt and resume. Measure observed acknowledgment classes and config/instruction loading. Choose native structured transport over a custom model/tool loop. Confirm supported authentication before paid trials.
4. **M3 bridge choice:** validate the pinned Python SDK's permission request and lifecycle mapping, explicit existing CLI path, exact native preset/settings, and host disconnect. Direct Rust stream adapter is the fallback only if its documented control semantics are sufficient; a TypeScript SDK bridge is the packaging alternative.
5. **M4 TUI:** Ratatui prototype over the real service; two workspaces, approval routing, disconnect while waiting, reconnect and resume output. Check keyboard-only operation, resize and terminal restoration. Textual/Bubble Tea are alternatives if measured usability or packaging makes Rust presentation unsuitable.

## Quantitative gates and product judgment

Proposed invariant gates: zero duplicate initial dispatches in a declared deterministic fault matrix; zero stale-owner mutations; every acknowledged receipt survives the tested crash cuts; every declared missing telemetry range remains visible. Report attempted cases and repetitions, not only a percentage. These are bounded observations, never a universal exactly-once claim.

For comparative product evaluation, pilot recovery time, human interventions, install time, TUI completion/error rates and resource overhead against disciplined native harness use at equal task/grant scope. Freeze sample size, thresholds and rubric after the pilot **before** confirmatory runs, with owner approval. This plan invents no performance score or threshold from absent measurements. Cross-product comparison belongs to benchmarks; PIO retains product-local evidence.

## Owner decisions before substantial implementation

1. **Stack and hosting:** adopt the proposed Rust/Tokio + SQLite + Ratatui stack, with an isolated official Python SDK bridge for Claude if its experiment passes; local user service and one durable host per active native session. Alternative: a TypeScript-oriented runtime reduces the SDK language boundary but changes the packaging/persistence preference.
2. **Support scope:** keep both exact harness candidates and macOS arm64 + Linux x86_64 for full 0.1, with Codex-first M2. A macOS-only release would be an explicit scope decision, not an incidental omission.
3. **Claude authentication:** qualify an API-key/provider route documented for SDK products. Existing claude.ai login support remains unresolved without an approved route; do not promise subscription reuse. This is a material product limitation, not a missing executable.

License selection and confirmatory quality thresholds are reserved later decisions, required before their dependent release/evaluation transitions. Next action after these three decisions: implement M1 and its experiments; preserve this proposal and record its accepted or amended disposition.
