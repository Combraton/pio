# M1 checkpoints

Issue [#3](https://github.com/Combraton/pio/issues/3) follows accepted M0 [#1](https://github.com/Combraton/pio/issues/1).

## Skeleton — 2026-09-16

Base and inspected pre-commit HEAD: `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; branch `codex/m1-core-host`. This record is part of the first skeleton commit. Rust 1.97.1 and Cargo.lock, three crate boundaries, PIO-owned Unix participant descriptor with empty actual claims and the exact PLAN target stored separately. No runtime behavior introduced.

Local macOS arm64: Cargo workspace build/test exit 0, zero behavior tests. Pinned release integrity, released runner self-test and fixture validation exit 0. All 280 fixtures selected: **279 unsupported, 1 skipped, 0 pass**. Runner exit 0 is pipeline evidence only. Local artifacts: `target/conformance-skeleton` (ignored); CI uploads the same classes and original transcripts. Documentation/diff checks exit 0. CI results remain to be collected. No real adapter or CBR invoked; no sibling changed.

Read-only MiniMax contract review completed exit 0, 113474 total tokens, USD 0.014839 reported by the dispatcher. Reviewed suggestions against accepted PLAN: reject its stdio-only recommendation and its attempt to defer backpressure from the M1 target. PIO uses Unix and retains the owner's target; actual claims advance only with implementation evidence.

M3 now explicitly requires a distinguishing credential-source observation and a missing-configured-credential control with cached login still present. No Claude credential or job was used.
