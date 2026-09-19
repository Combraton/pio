# M3 kickoff — paste this into a fresh builder session

You are the standalone **PIO M3 builder**. Everything you need is in this repository; the prompt below is a pointer, not a substitute for reading the records.

## Prompt

> You are the implementation lead for **M3 of PIO** (`Combraton/pio`): the **Claude Code adapter**, the second real harness adapter, on [issue #7](https://github.com/Combraton/pio/issues/7).
>
> Work in the `pio-m3` worktree on branch `codex/m3-claude-code`, based on `main` at `16fb2291`, the merge of M2. Read, in order: [`docs/work/STATE.md`](../STATE.md), [`docs/work/m3/TASK.md`](TASK.md), [ADR 001](../../decisions/001-standalone-stack.md), [ADR 003](../../decisions/003-codex-app-server-adapter.md) as the shape your adapter ADR should follow, and the [M2 acceptance packet](../m2/ACCEPTANCE.md) for what was proven, what was not, and why.
>
> Deliver the six scope items on issue #7. Do not start a live run until qualification, the offline matrix against a labeled fake, and the credential-route negative control are committed and CI is green — and never from a dirty tree.
>
> Report outcomes as they are. A failing test is a fact to state, not a thing to route around; an unproven property stays `not_evaluated`.

## What the M2 builder learned, so you do not pay for it again

- **Commit before every live run.** A receipt from a dirty tree is not evidence. M2 spent 72,935 tokens re-running R1 because a staged file rename was sitting in the worktree.
- **Run the full matrix before committing, not just the case you touched.** Adding admission headroom silently broke an older capacity case and only CI caught it.
- **Qualify what is installed, not what a document remembers.** The readiness record says Claude Code 2.1.273; 2.1.278 is installed. Codex moved 0.146.0 → 0.155.1 mid-milestone and the exact pin correctly refused the new binary — that refusal is the feature working, not a bug.
- **Measure the harness's defaults, do not infer them.** The Codex guard assumed a trusted-project default that turned out to hold only for already-trusted projects; an offline probe settled it in minutes and changed what the receipts say.
- **A token stop is only as tight as the harness's usage reports.** Codex reports roughly every 24,000 tokens, so a 50,000 limit stopped a run at 72,911. Set limits knowing that, and never call a stop "PIO's budget enforcement" — PIO has none.
- **Writing a test executable and exec'ing one race inside a single test binary.** A sibling test's fork inherits the write descriptor and Linux refuses the exec with `ETXTBSY`. Serialize those regions.
- **Check what a response schema actually requires before answering it.** Codex's permission-grant request never took a decision at all; PIO would have answered a shape the harness cannot read, and which, if understood, would have granted permissions past its own guard.

## Environment

Worktrees `pio` (readiness), `pio-m1`, `pio-m2` and `pio-m3` are siblings under `Combraton/`; never modify another one. Claude Code is at `/opt/homebrew/bin/claude`. Live evidence directories, private transcripts and the usage ledger live outside Git, mode 0700, as M2's did. The pinned toolchain is Rust 1.97.1 and Protocol is pinned in `protocol.lock.json`.

## Owner decisions that still bind

Harnesses run exactly as the user installed and configured them; PIO never selects a model or provider outside a dated, test-only exception. v0.1 release scope remains Codex and Claude Code — OpenCode and Hermes Agent are test scope (M3b, after M3) and do not gate the release. Hermes runs only under an isolated profile and never against the owner's real Hermes home. Caps are per harness: 1,000,000 for Codex, 1,000,000 for Claude Code, 300,000,000 for MiniMax through OpenCode and Hermes, with GLM and Kimi proxy-counted until per-provider caps exist. Stop and report at 80%.
