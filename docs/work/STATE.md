# Current session state — PIO

Dated snapshot; reconcile Git with [M3 issue #7](https://github.com/Combraton/pio/issues/7) and the [M3 task packet](m3/TASK.md). Issues own live progress.

- **Updated:** 2026-09-19. **Owner:** fresh builder session for M3 (assigned by the owner). Independent review and owner acceptance remain separate.
- **Task:** M3 — Claude Code adapter with real journeys. Issue [#7](https://github.com/Combraton/pio/issues/7). Start from the [kickoff prompt](m3/KICKOFF.md).
- **Branch/base:** `codex/m3-claude-code` in the `pio-m3` worktree, based on **`16fb22912f93…`**, the merge of [PR #6](https://github.com/Combraton/pio/pull/6). Its tree is byte-identical to the reviewed head `699bf6f`. Worktrees `pio`, `pio-m1` and `pio-m2` are preserved; never modify a sibling.

## M2 is merged and closed

**M2 delivered the first real harness adapter.** PIO drives the owner's installed Codex 0.155.1 app-server behind the durable host: qualification by wrapper, Node and native binary hash against a 312-file canonical schema identity; content-addressed briefs; native approvals as Protocol actions; steering, interrupt, usage, workspaces and observed-only discovery; a thread-settings guard that refuses anything broader than the user's configured default; and the ADR 002 capacity bound with admission headroom. Issue #5 closed; [PR #6](https://github.com/Combraton/pio/pull/6) merged as `16fb2291` after three reviews.

- **Live evidence:** R1 on the user's configured model with nothing passed, R2–R6 on an explicit model under a dated test-only exception, plus zero-token `model-list`, `discovery` and `wrong-executable` runs. **421,450 observed tokens** of the 1,000,000 Codex cap. Receipts in [codex-live](m2/codex-live/); the [acceptance packet](m2/ACCEPTANCE.md) marks J1, J3, J4 and J5 as **partial acceptances** with their obligations, and J2 and J6 `not_evaluated`.
- **Named obligations carried into M3 and beyond:** incremental changed-key commits before M3b or M4; **product budget enforcement is unproven** — the M2 stops were the live runner's, not PIO's; `test_only_model_exception` must be removed or compiled out before release; J5's negative control does not exist; the exact-version pin refuses every Codex that is not 0.155.1, which is right for M2 and wrong for real users.
- **Upstream:** [Protocol #13](https://github.com/Combraton/protocol/issues/13) capacity error, [#14](https://github.com/Combraton/protocol/issues/14) content path for digest-only fields, [#15](https://github.com/Combraton/protocol/issues/15) reattach recovery decision. All open; none widened locally.

## Owner decisions still binding (recorded at M2 start)

Full text: [ADR 001 amendment](../decisions/001-standalone-stack.md#amendment--owner-decisions-after-m1-acceptance-2026-09-16) and [PLAN amendment](standalone-0.1/PLAN.md#owner-amendment-after-m1-acceptance-2026-09-16).

- PIO drives each harness **as the user installed and configured it**; it never selects or injects a model or provider. A same-day kickoff draft routing MiniMax through a Codex custom provider was **withdrawn by the owner**; nothing was configured for it.
- Test scope: Codex 0.155.1 (M2; re-pinned from 0.146.0 on 2026-09-19), Claude Code 2.1.273 (M3), OpenCode v2.0.1 as `opencode2` and Hermes Agent v0.20.1 (PLAN row M3b, after M3, confirmed). OpenCode preferred for heavy-usage testing. **v0.1 release scope remains Codex and Claude Code**; OpenCode and Hermes do not gate v0.1 unless the owner promotes them. Test scope is not release scope.
- Codex live runs use the user's real Codex home with before/after configuration capture on every run, in a throwaway fixture repository path; never the full-access sandbox or `thread/shellCommand`; every added trusted-project entry disclosed.
- Hermes (later): only an isolated profile carrying model configuration; the owner's real Hermes home runs scheduled jobs and is never driven by PIO. If isolation is impossible, defer the Hermes adapter and say so.
- Claude Code (M3): user's own login, or API key when configured; Anthropic caveat visible; route evidence, precedence rule and missing-route refusal.
- MIT license; M4 reviewer mockup design step; M6 reviewer walkthrough; thresholds unchanged.
- Reserved: evaluation thresholds/rubric, merge, tag, publication.

## Live-run spend ledger

Counted from each harness's own usage reports; missing usage is unknown liability, not zero. Stop and report at 80 percent. At most three concurrent live sessions. Codex and Claude Code caps are separate, per harness (owner confirmation 2026-09-16).

| Cap | Limit (tokens) | Stop at | Used | Runs |
| --- | ---: | ---: | ---: | ---: |
| Codex, all tests | 1,000,000 | 800,000 | **421,450** | 7 |
| Claude Code, all tests | 1,000,000 | 800,000 | 0 | 0 |
| MiniMax via OpenCode + Hermes | 300,000,000 | 240,000,000 | 0 | 0 |
| GLM/Kimi via OpenCode — **proxy-counted** in the MiniMax row until the owner sets per-provider caps (required before M3b) | proxy | — | 0 | 0 |

## Accepted baseline and current counts

- **M1** owner-accepted at `2048e84` and merged as `9cf7047`: journal-backed Core/Execution slice, public durable labeled fake-process host, caller operation ledger, content-addressed output, recovery and order fences, truthful fake discovery, packaging skeleton. [Acceptance corrections](m1/ACCEPTANCE-CORRECTIONS.md), [CHECKPOINTS](m1/CHECKPOINTS.md).
- **M2** merged as `16fb2291`. Current counts at that head: **46 Cargo tests + 1 ignored probe**; offline Codex matrix **17 cases × 3 = 51**; public matrix **72** (51/6/9/6); diagnostic matrix **54** (39/3/9/3); caller recovery; pinned runner **206 pass / 73 unsupported / 1 skipped** plus two supplemental passes counted separately. Three repetitions per case. Green on macOS 15 arm64 and Ubuntu 24.04 x86_64.
- **What is still not established:** no TUI, no CBR composition, no packaged-install journey, no Claude Code adapter, no throughput claim, no total-disk or spool-GC bound, no universal exactly-once property, and **no product budget enforcement**. Process cancellation, workspace and usage adapters are unavailable in the fake process mode. Protocol #9, #10, #13, #14, #15 and the undeclared subscription authorization-recheck barrier remain coverage limits.

## M2 history

The narrative of M2 — the capacity-bound checkpoint, Codex qualification, the offline adapter, three owner reviews, the re-pin to Codex 0.155.1 and the six live runs — is recorded where it belongs rather than repeated here: the [acceptance packet](m2/ACCEPTANCE.md), the [capacity-bound report](m2/COMMIT-BOUND.md), [ADR 003](../decisions/003-codex-app-server-adapter.md), the [qualification evidence](m2/codex-qualification/README.md), the [live receipts](m2/codex-live/) and [issue #5](https://github.com/Combraton/pio/issues/5), which carries every receipt as it landed.

## Resources and next action

- **Active resources:** worktrees `pio`, `pio-m1`, `pio-m2` and `pio-m3` under `Combraton/`. After the M2 runs, no PIO daemon, host, user job or live harness session was left running (process table and launchd list inspected). Temporary matrix stores under `/tmp`, private live evidence under `$HOME/pio-m2-live` at mode 0700, and CI artifacts in session scratch all stay outside Git.
- **Next:** M3, the Claude Code adapter, on [issue #7](https://github.com/Combraton/pio/issues/7). A fresh builder session starts from the [kickoff prompt](m3/KICKOFF.md), then this file, then the [task packet](m3/TASK.md).
- **Before any M3 live run:** qualification, the offline matrix against a labeled fake and the credential-route negative control must be committed with CI green, and the tree must be clean. A receipt from a dirty tree is not evidence.
