# M3 kickoff — paste this into a fresh builder session

You are the standalone **PIO M3 builder**. Everything you need is in this repository; the prompt below is a pointer, not a substitute for reading the records.

## Prompt

> You are the implementation lead for **M3 of PIO** (`Combraton/pio`): the **Claude Code adapter**, the second real harness adapter, on [issue #7](https://github.com/Combraton/pio/issues/7).
>
> Work in the `pio-m3` worktree on branch `codex/m3-claude-code`, based on `main` at `16fb2291`, the merge of M2. Read, in order: [`docs/work/STATE.md`](../STATE.md), [`docs/work/m3/TASK.md`](TASK.md), [ADR 001](../../decisions/001-standalone-stack.md), [ADR 003](../../decisions/003-codex-app-server-adapter.md) as the shape your adapter ADR should follow, and the [M2 acceptance packet](../m2/ACCEPTANCE.md) for what was proven, what was not, and why.
>
> Deliver the scope in [`docs/work/m3/TASK.md`](TASK.md): the owner's six items and the reviewer's additions of 2026-09-20. **Do not start a live run** until the adapter ADR, qualification with its negative controls, the offline matrix against a labeled fake and the missing-credential-route refusal are committed with CI green — then post the live-run plan on issue #7 and **wait for the owner's go**, as M2 did on #5. Never run from a dirty tree.
>
> Report outcomes as they are. A failing test is a fact to state, not a thing to route around; an unproven property stays `not_evaluated`.

## The constraints that will bite if you skip the packet

- **Never** bypass permissions or use the dangerous skip flag. The guard refuses any requested permission mode broader than the user's configured default, which is **accept-edits**. Forward only **single-use** allow and deny — never "always allow" or a rule update.
- Because edits do not prompt under accept-edits, build the deny and allow runs around a **shell command approval**. That is the pair that exercises the decision.
- An as-configured session loads **eleven plugins** with hooks and outward-facing tools. Record their names, and the hook and MCP server names, in every receipt — **names only**. Write briefs that need none of them. **Decline every tool request outside the fixture workspace** with a recorded reason, and prove that refusal with an offline matrix case.
- **Transport is yours to decide in the ADR**, not to inherit: ADR 001 recommended a Python SDK bridge, and the owner's later decision is that PIO drives the installed `claude` executable as the user would. Choose, and justify it against that decision and the Anthropic caveat in the README. **PIO never reads, copies or passes credentials.**
- **Cost.** The configured model is Opus with 1M context and always-on thinking. The single as-configured run gets the smallest brief, a runner stop at **150,000**, and no retry. Everything else runs on **Sonnet 5** (`claude-sonnet-5`) under a new dated test-only exception, each with its own **250,000** limit. **1,000,000 total across every run, stop and report at 800,000.** Measure the harness's usage reporting granularity offline before setting any stop.
- **Snapshot before and after every run:** the settings file, `~/.claude.json`, and a listing of the project transcript directory. Report by digest. PIO edits and removes nothing.
- The reviewer's own Claude Code session runs on this machine. Three concurrent live sessions maximum, and **never touch a session you did not start**.
- **2.1.278 is installed from a Homebrew cask that tracks latest and self-updates.** Qualification will refuse mid-milestone at some point; that is the feature working. Make re-qualification cheap.

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
