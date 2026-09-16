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
