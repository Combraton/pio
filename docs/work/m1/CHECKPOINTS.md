# M1 checkpoints

Issue [#3](https://github.com/Combraton/pio/issues/3) follows accepted M0 [#1](https://github.com/Combraton/pio/issues/1).

## Skeleton — 2026-09-16

Base and inspected pre-commit HEAD: `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; branch `codex/m1-core-host`. This record is part of the first skeleton commit. Rust 1.97.1 and Cargo.lock, three crate boundaries, PIO-owned Unix participant descriptor with empty actual claims and the exact PLAN target stored separately. No runtime behavior introduced.

Local macOS arm64: Cargo workspace build/test exit 0, zero behavior tests. Pinned release integrity, released runner self-test and fixture validation exit 0. All 280 fixtures selected: **279 unsupported, 1 skipped, 0 pass**. Runner exit 0 is pipeline evidence only. Local artifacts: `target/conformance-skeleton` (ignored); CI uploads the same classes and original transcripts. Documentation/diff checks exit 0. CI results remain to be collected. No real adapter or CBR invoked; no sibling changed.

Read-only MiniMax contract review completed exit 0, 113474 total tokens, USD 0.014839 reported by the dispatcher. Reviewed suggestions against accepted PLAN: reject its stdio-only recommendation and its attempt to defer backpressure from the M1 target. PIO uses Unix and retains the owner's target; actual claims advance only with implementation evidence.

M3 now explicitly requires a distinguishing credential-source observation and a missing-configured-credential control with cached login still present. No Claude credential or job was used.

## Experimental journal and host — 2026-09-16

Base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; inspected pre-commit HEAD `0a88612b2f90e4470668de900c4c0d819b826780`. This checkpoint adds behavior after the separate skeleton commit. [Draft PR #4](https://github.com/Combraton/pio/pull/4) tracks work; it is not ready for M1 acceptance.

The standalone fake diagnostic path commits admission/intent/journal/outbox atomically before its only host launch attempt. Strong digest plus payload equality rejects conflicts. The host owns the child pipes and persists across daemon death; launch/slot locks and controller fencing guard release. Kernel boot/start identity distinguishes a surviving child from PID reuse. Persistent launch guards detect rolled-back admission even within one controller generation; external generation evidence detects older controllers. Ambiguous interrupted launches are held without automatic respawn. Receipts report observed process exit and output digest, not product acceptance.

Local macOS arm64 matrix: twelve cases, three repetitions each, **36 attempted**, **33 pass**, **3 expected J3 property failures**. J3 fails `single_launch_identity_count` with `expected 1 child start; observed 2`; the positive recovery case uses the same checker. Journal write refusal is observed with independent child markers and an OS process-table check. Controller generation changes while host slot/generation and kernel child identity remain the same. Matrix artifacts include source inventory, binary digest, journal/outbox, append-only child markers and before/after observations. `cargo build --workspace --locked` and clippy with warnings denied pass. Tests and exact commands are in [verification](../../VERIFICATION.md).

The writer helper was stopped during review: its FNV dedupe hash and overwriteable marker design did not meet the invariants. Its uncommitted draft was preserved outside Git and replaced by the coordinator before any behavior commit. Dispatcher reports killed/exit -15 and approximately 1,109,268 estimated tokens; no final token/cost receipt was available. The earlier read-only helper's receipt is above. No worker result is treated as proof.

This is a lower-level experimental fake-host slice. The diagnostic JSON interface is not Protocol; no real adapter, production service, public grant/capability semantics, caller operation persistence, discovery/workspaces/usage or complete backup/restore certification is claimed. Actual participant claims remain empty. The PLAN target and full issue acceptance list remain unchanged and open.

## Verified behavior head — 2026-09-16

Base remains `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; tested head is `d1348e168a46430415ef9900d2f225ace4effa5d`. [Exact-head CI](https://github.com/Combraton/pio/actions/runs/35071561771) succeeded on macOS arm64 and Linux x86_64. Downloaded artifacts confirm 36 attempts per platform: 33 pass and 3 intended mutant property failures. Protocol outcomes on each platform remain 279 unsupported, 1 skipped, zero pass. A clean clone at the same head reproduced the documented build/test/clippy, matrix and freshly downloaded pinned-runner sequence, exit 0. [Receipt index](evidence/checkpoint.json) records source inventory/binary hashes and artifact names. Cargo's unit/doc suites have zero tests; the process matrix supplies the behavior evidence. All local fake processes were cleaned up; isolated stores remain outside Git for diagnosis.

This evidence-only successor records that behavior head. Public protocol work is the next implementation checkpoint; the draft is not ready for M1 acceptance.

## Owner-review corrections — 2026-09-16

Base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; inspected HEAD `4bc8f3719681c77fe89d7fc26d0371607a82ff55`. The owner reproduced the lower-level checkpoint and authorized continuation without accepting M1. Three distinct replay mutants now require the exact refusal from the next defense: duplicate launch guard, host phase fence, live host slot. A mismatched reason reports `wrong_reason`; its own negative control verifies that classification. The original J3 two-child mutant remains counter-checker sensitivity evidence only.

Fenced release records `known_not_released` only after no release attempt, child reap and absent release marker, with the cause persisted in journal/inspect. A deterministic barrier reproduces daemon restart before release. A separate filesystem test removes write permissions from the SQLite files and store directory and observes startup refusal plus no independent child marker. Query-only simulation remains confined to the fake diagnostic interface. Three Cargo unit tests cover replay/conflict atomicity, invalid/stale transitions and controller fencing. Participant description discloses the unimplemented launch command; PR checkout/artifact identity uses the branch head SHA.

Local single-repetition matrix: 18 attempts; 13 passes, one expected two-child property failure, three named defense refusals, one expected wrong-reason detection. These precede the public Core checkpoint; execution remains deferred.
