# Standalone PIO 0.1 — implementation readiness

Status: **M0 accepted by the owner as a documentation checkpoint**, 2026-09-16, at reviewed head `85255d9` against base `e65b7c0`. Owner: Codex, PIO implementation lead. The stack and release scope are accepted with the dispositions below; no working PIO runtime or release is claimed. Deliverables: this plan, the [stack ADR](../../decisions/001-standalone-stack.md), [journey matrix](../../JOURNEYS.md), and [handoff](HANDOFF.md).

Tracking: M0 [issue #1](https://github.com/Combraton/pio/issues/1), merged [PR #2](https://github.com/Combraton/pio/pull/2); implementation [M1 issue #3](https://github.com/Combraton/pio/issues/3). The owner authorized the M0 merge and continuation. M1 branches from merged main; tag and publish remain separate authorizations.

## Outcome and fixed boundaries

Ship an installable local execution service with CLI and first-class TUI for **both Codex and Claude Code**. It preserves each harness's native investigate/edit/test loop, provides inspectable execution and workspace identities, and recovers honestly after client or service interruption. A first Codex slice is a milestone, not the full release. Core work must succeed with CBR and Combraton absent.

Keep the accepted architecture: core execution authority; separate standalone caller policy; presentation with no authoritative execution state; immutable observations; invocation/effect intent before dispatch; fenced ownership; reconcile before retry; distinct delivery, result, verification and acceptance. There is no workflow engine, automatic model selection, global hook installation, private CBR import or shared writable store.

Protocol is now released. The [verified pin](../../../protocol.lock.json) is `v0.1.0`, commit `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. Moving main is not its replacement. See [consumer mapping](PROTOCOL.md) for the exact integration surface and release limitations.

## Accepted release boundary

| Area | Full 0.1 target | Explicit boundary |
|---|---|---|
| Platforms | macOS arm64 and Linux x86_64; build and conformance CI on both from M1 onward; exact CI images and minimum OS versions fixed during M1 | Only macOS 26.3.1 arm64 inspected here; Linux is untested. Windows, remote/multi-host operation and other architectures deferred |
| Harnesses | Codex 0.146.0 and Claude Code 2.1.273 as initial exact validation candidates | Neither is supported yet. New versions require qualification; do not advertise an untested version range |
| Interfaces | Authenticated local `stream/1` Unix socket; CLI and TUI call the same public Core/Execution API | No HTTP service, desktop dependency or PTY scraping required. Public stdio proxy is a later optional binding unless needed by conformance |
| Work | Discover/configure, grant/scope, submit, inspect/watch/output, concurrent isolated workspaces, approvals, cancellation, supported steering/continuation, reconcile, checkpoint/export | Native features differ; unsupported capabilities are explicit. An opaque harness cannot supply guaranteed exactly-once remote effects or hard billed-dollar bounds |
| Lifecycle | User service plus durable execution hosts; client detach and daemon reconnect preserve host ownership and output | Host death and machine reboot are reconciliation cases, not promises to resume arbitrary in-flight model state |
| Persistence | Journal/outbox, deduplication, bounded spools, immutable receipts, usage liability, retention, backup/export/restore and versioned migration | No network-filesystem SQLite state; preserve unresolved effects and required evidence across cleanup |
| Optional CBR | Caller-selected packets, exact-byte verification, required/advisory enforcement, correction/outage handling, eligible evidence publication | Integration may be built against labeled fixtures before CBR is ready; real composition remains a separate unpassed gate |

Platform and authentication dispositions are accepted below. Library selection within the accepted stack proceeds through bounded experiments. Product licensing remains an owner decision before distribution; no license is selected here.

## Milestones and acceptance

Each milestone ends with a reviewable diff, exact build identity, evidence and a short acceptance checkpoint. Routine fixes continue inside its scope. Changing the release boundary or an accepted invariant returns to the owner.

| Milestone | Deliverable | Acceptance evidence / dependencies |
|---|---|---|
| M0 — readiness | Reconciled sources, verified Protocol pin, stack/scope decisions, journey contracts | Accepted by owner at `85255d9`; documentation and integrity checks do not accept runtime behavior |
| M1 — core and durable host | Cargo workspace, local protocol service, journal/outbox/deduplication, caller operation store, fenced launch barrier, host spool, scoped discovery and packaging skeleton | **Accepted by the owner at `2048e84`**, 2026-09-16; named participant subset/controls and acceptance evidence below, reproduced from a clean clone including workstation launchd start/stop and both CI platforms; independent reorder mutant fails on the named ordering property. macOS arm64 + Linux x86_64 CI build and conformance with pinned toolchain/lockfile. No real-adapter milestone claim |
| M2 — first real adapter | Codex native app-server behind a persistent host; CLI submits scoped repository work and records output/result | J1 and J5 with real Codex; J3/J4 for Codex; schema/version drift refusal, approval deny/allow, no weakened permissions. Authentication and chosen model recorded for live tests |
| M3 — second adapter | Claude native loop through a persistent SDK bridge or validated direct interface, with exact version and auth matrix | J1/J3/J4/J5 for real Claude; prove project instructions, permission configuration, native output and session semantics remain intact. A failure here holds full release; Codex-only is not silently accepted |
| M4 — terminal product | TUI workspace/execution navigation, output inspection, correlated approval responses, detach/reconnect and capability-aware actions | J2 with both real harnesses in independent workspaces; real terminal evidence and human usability review; deny wrong-execution approval and test narrow terminal/keyboard operation |
| M5 — optional composition | Public Context/Evidence client, immutable bindings, revalidation, publication outbox; no private stores | J6 using real CBR when available; fixtures cover faults earlier. Core no-CBR J5 rerun. Missing real CBR holds composition evidence, not independent M1–M4 work |
| M6 — release candidate | Fresh install/update, migration/restore barrier, retention/export, complete capability/support matrix, docs and comparative evidence | All required journeys for both adapters on each advertised platform; conformance coverage and exclusions; backup restore with surviving host, disk-full, stale owner and PID-reuse negative controls. Owner accepts; merge/tag/publish require separate explicit authorization |

## Bounded experiments

Each experiment gets at most one focused implementation/measurement cycle and one repair/retest before a checkpoint. An unresolved invariant becomes an explicit limitation or decision; it does not become an indefinite research project.

1. **M1 host lifetime:** run a labeled deterministic child that records invocation markers. Disconnect terminal, restart daemon, then kill host separately. Correlate process start identity and generations. Surviving-host path must not respawn; lost-host path must retain uncertainty. Force a duplicate-launch mutant and require the identity/count check to fail.
2. **M1 persistence:** kill at intent commit, host claim, prompt release and receipt publication; inject disk-full and stale store restore. Durable state must precede dispatch; unresolved effects keep their liability. Compare SQLite WAL/FULL plus a single writer with rollback journal only if deployment constraints defeat WAL.
3. **M2/M3 native fidelity:** real bounded repository edit/test, native permission deny/allow, steering, interrupt and resume. Measure observed acknowledgment classes and config/instruction loading. For pinned Codex, account for `thread/start` with `cwd` and `workspace-write` persisting a trusted-project entry in `~/.codex/config.toml`: disclose and either avoid through a validated configuration path or account for the durable effect; capture before/after state in an isolated test environment. Use canonical JSON per schema file for M2 drift checks; raw bytes of `codex_app_server_protocol.v2.schemas.json` are nondeterministic. Choose native structured transport over a custom model/tool loop. Confirm explicit API/provider authentication before paid Claude trials, with no silent cached-login fallback. Require a distinguishing observation from CLI initialization/account source or the provider proving which API/provider credential served the run. A negative control removes configured credentials while preserving an existing cached login and must refuse specifically for the missing qualified route; silent success fails M3.
4. **M3 bridge choice:** validate the pinned Python SDK's permission request and lifecycle mapping, explicit existing CLI path, exact native preset/settings, and host disconnect. Direct Rust stream adapter is the fallback only if its documented control semantics are sufficient; a TypeScript SDK bridge is the packaging alternative.
5. **M4 TUI:** Ratatui prototype over the real service; two workspaces, approval routing, disconnect while waiting, reconnect and resume output. Check keyboard-only operation, resize and terminal restoration. Textual/Bubble Tea are alternatives if measured usability or packaging makes Rust presentation unsuitable.

## Quantitative gates and product judgment

Proposed invariant gates: zero duplicate initial dispatches in a declared deterministic fault matrix; zero stale-owner mutations; every acknowledged receipt survives the tested crash cuts; every declared missing telemetry range remains visible. Report attempted cases and repetitions, not only a percentage. These are bounded observations, never a universal exactly-once claim.

For comparative product evaluation, pilot recovery time, human interventions, install time, TUI completion/error rates and resource overhead against disciplined native harness use at equal task/grant scope. Freeze sample size, thresholds and rubric after the pilot **before** confirmatory runs, with owner approval. This plan invents no performance score or threshold from absent measurements. Cross-product comparison belongs to benchmarks; PIO retains product-local evidence.

## M1 participant and acceptance contract

This is the implementation target, not an existing descriptor or coverage claim. M1's first commit must encode the subset and exact supported controls in PIO's own participant descriptor and document executable commands in VERIFICATION.md.

- Serve `core/1` and base `execution/1` over the Unix binding. Target Core features: `core.grants`, `core.events`, `core.capabilities`, `core.effects`, `core.events.backpressure`. Target Execution features: `execution.controller`, `execution.discovery`, `execution.output`, `execution.workspaces`, `execution.usage`. The test participant also serves `core-test/1` for the Core fixture subjects; this is conformance-only and never advertised by the production service. In M1 all execution behavior is from the labeled scripted/fake host, never a real native adapter. Steering, actions, continuation, context/revalidation and Evidence output features remain unadvertised until their milestones implement them.
- Declare runner `claims.test_controls` by their released names: `executor.script`, `clock.file`, `store.faults`. Document supported executor script steps/settings for this feature subset. Also implement and explicitly report launch controls `dedupe.advance_on_start`, `dedupe.retain_generations`, `events.new_epoch_on_start`, `events.unvouched_last`, `events.retain_last`, fixed/controlled clock, capability overrides and receive limits. Dedupe/event keys are launch controls, not invented named features. Follow the released launch order and failure semantics.
- The Execution checkpoint implements the `session.closed` signal and the three backpressure signals (`backpressure.stall.started`, `backpressure.limit.reached`, `backpressure.connection.closed`). The deliberately paused subscription race barrier remains undeclared. Declare only implemented `claims.test_barriers`; no blanket claim of reference-provider barriers, mutants or optional controls. Unsupported launch keys must fail startup rather than be silently ignored. Fixtures requiring undeclared controls, barriers or features appear as **unsupported coverage limits** in the M1 report, with their IDs/reasons; retain the runner's original outcome classifications and do not count exclusions as passes. Conformance control hooks stay test-environment-only, not public production endpoints.

Exact control semantics: [released conformance README](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/conformance/README.md#launch-configuration). The participant must use PIO's implementation, not launch the Protocol reference provider in its place.

M1 acceptance requires all of the following:

1. CI builds on macOS arm64 and Linux x86_64 using a pinned toolchain and lockfile; VERIFICATION.md contains the exact commands actually run.
2. Build the conformance runner from the verified pinned release archive and run it against PIO's participant descriptor. Upload descriptor, manifest and transcripts as CI artifacts; report outcomes separately as pass, fail, timeout, unsupported and any other runner class, preserving actual labels.
3. Upload the labeled fake-host fault matrix with attempted case/repetition counts, and J3's duplicate-launch negative control failing on the intended identity/count property, not a runner error.
4. Detach, restart daemon and reattach with correlated process start identity and host/controller generation recorded before and after; prove the surviving child was not replaced.
5. Prove no child spawn after journal failure using an independent spawn marker/process observation, not only a refused return code.
6. State explicitly that no real adapter has been validated. Product-native J1–J6 remain unpassed by fake-host or conformance results.

## Owner-decision dispositions

Accepted explicitly by the owner on 2026-09-16 after independent review of PR #2 head `85255d9` against base `e65b7c0`:

1. **Stack and hosting — accepted as proposed:** Rust/Tokio + SQLite + Ratatui, local user service and durable session hosts. The Python bridge remains **conditional on the M3 experiment**, not an unconditional dependency decision.
2. **Support scope — accepted:** both exact harness candidates and macOS arm64 + Linux x86_64; Codex-first M2. Linux x86_64 build and conformance are required in CI **from M1 onward**.
3. **Claude authentication — accepted:** API-key or provider route is the qualified path. **claude.ai login is unsupported until an approved route exists**; record this in the README before any release. Selecting an already-installed CLI does not override this authentication boundary.

License selection and confirmatory thresholds remain later reserved decisions. M0 acceptance authorizes this follow-up commit and marking PR #2 ready; it does not authorize merge, tag or publication. After a separately authorized merge, branch M1 from main and implement the contract above. Keep sibling repositories untouched and update STATE at every checkpoint.
