# Current session state — PIO

Dated snapshot; reconcile Git with [M1 issue #3](https://github.com/Combraton/pio/issues/3). Issues own live progress.

- **Updated:** 2026-09-16. **Owner:** Codex, standalone PIO implementation lead.
- **M0:** accepted at `4549c22ad9835504a0618d8fcb226afe222c0190`; owner authorized merge and continuation. [PR #2](https://github.com/Combraton/pio/pull/2) merged as `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`.
- **M1 branch/base:** `codex/m1-core-host`, one implementation worktree branched from remote main at `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`. **Inspected HEAD at this checkpoint:** same commit; the skeleton commit containing this record is its successor. Read actual HEAD on resume.
- **Scope:** [PLAN](standalone-0.1/PLAN.md), [ADR](../decisions/001-standalone-stack.md), [M1 issue acceptance](https://github.com/Combraton/pio/issues/3). Shared architecture inspected at Combraton `9af69ce966bfacf0deb03606d99f28a355d1f944`. Siblings untouched.
- **First checkpoint:** Cargo workspace, Rust 1.97.1/lockfile, empty actual participant claims, separate exact M1 target, dual-platform CI and verified-release runner command. [Verification](../VERIFICATION.md) records commands. This is a skeleton, with no behavior or real-adapter claim.
- **Protocol:** locked v0.1.0 source `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`; neither later main nor the reference provider is PIO's implementation.
- **Checks:** Rust 1.97.1 build/test exit 0 on macOS arm64 (zero behavior tests); released runner self-test/fixture check exit 0; 280 fixture outcomes: 279 unsupported, 1 skipped, 0 pass. Documentation and diff checks exit 0. CI and clean-export replication remain to run after push.
- **Carry-forward:** canonical Codex schema comparison; Codex trusted-project config side effect; exact participant/control claims; M3 distinguishing authentication evidence and missing-credential/cached-login refusal control.
- **Resources:** bounded read-only MiniMax contract reviewer completed (exit 0); its suggestions are untrusted and checked against PLAN/spec. No real adapter invoked. Historical installed `pio` is a different tool.
- **Remaining:** implement durable core, Unix participant, fake host, fault matrix, independent spawn observations and J3 mutant; verify on both platforms and update acceptance evidence. No M1 merge/tag/publication authorized.
- **Next:** commit verified skeleton first, then implement behavior with checkpoint records of base/head. User source attachment remains preserved; no sibling prompt retired.
