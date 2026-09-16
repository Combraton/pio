# Current session state — PIO

Dated snapshot; reconcile Git with [M2 issue #5](https://github.com/Combraton/pio/issues/5) and the [M2 task packet](m2/TASK.md). Issues own live progress.

- **Updated:** 2026-09-16. **Owner:** Claude Code session, standalone PIO M2 builder (assigned by the owner). Independent review and owner acceptance remain separate.
- **Task:** M2 — Codex 0.146.0 app-server adapter with real J1/J3/J4/J5. Issue [#5](https://github.com/Combraton/pio/issues/5).
- **Branch/base/head:** `codex/m2-codex-app-server` in the single `pio-m2` worktree. **Base `9cf70474c28f549650e6b48e8be20ae88426a1b0`**, the merge of [PR #4](https://github.com/Combraton/pio/pull/4) on main. Its tree equals reviewed head `a34c408864e8f87abefd6d820518902b31b6a724`, and accepted M1 head `2048e84b0e77b258444f8f30333f4b76584cdccc` is an ancestor. The branch started at `9cf7047`; its first commit is documentation only (owner decisions, MIT LICENSE, records), and its SHA is in Git history. The earlier `pio` and `pio-m1` worktrees are preserved; no sibling repository modified.
- **Transitions done:** owner authorized the PR #4 merge; it was merged with `--match-head-commit a34c408` after both CI workflows passed on that head (push and pull_request). Issue #3 closed on merge; a successor comment links #5.

## Owner decisions recorded at M2 start

Full text: [ADR 001 amendment](../decisions/001-standalone-stack.md#amendment--owner-decisions-after-m1-acceptance-2026-09-16) and [PLAN amendment](standalone-0.1/PLAN.md#owner-amendment-after-m1-acceptance-2026-09-16).

- PIO drives each harness **as the user installed and configured it**; it never selects or injects a model or provider. A same-day kickoff draft routing MiniMax through a Codex custom provider was **withdrawn by the owner**; nothing was configured for it.
- Test scope: Codex 0.146.0 (M2), Claude Code 2.1.273 (M3), OpenCode v2.0.1 as `opencode2` and Hermes Agent v0.20.1 (PLAN row M3b, after M3, confirmed). OpenCode preferred for heavy-usage testing. **v0.1 release scope remains Codex and Claude Code**; OpenCode and Hermes do not gate v0.1 unless the owner promotes them. Test scope is not release scope.
- Codex live runs use the user's real Codex home with before/after configuration capture on every run, in a throwaway fixture repository path; never the full-access sandbox or `thread/shellCommand`; every added trusted-project entry disclosed.
- Hermes (later): only an isolated profile carrying model configuration; the owner's real Hermes home runs scheduled jobs and is never driven by PIO. If isolation is impossible, defer the Hermes adapter and say so.
- Claude Code (M3): user's own login, or API key when configured; Anthropic caveat visible; route evidence, precedence rule and missing-route refusal.
- MIT license; M4 reviewer mockup design step; M6 reviewer walkthrough; thresholds unchanged.
- Reserved: evaluation thresholds/rubric, merge, tag, publication.

## Live-run spend ledger

Counted from each harness's own usage reports; missing usage is unknown liability, not zero. Stop and report at 80 percent. At most three concurrent live sessions. Codex and Claude Code caps are separate, per harness (owner confirmation 2026-09-16).

| Cap | Limit (tokens) | Stop at | Used | Runs |
| --- | ---: | ---: | ---: | ---: |
| Codex, all tests | 1,000,000 | 800,000 | 0 | 0 |
| Claude Code, all tests | 1,000,000 | 800,000 | 0 | 0 |
| MiniMax via OpenCode + Hermes | 300,000,000 | 240,000,000 | 0 | 0 |
| GLM/Kimi via OpenCode — **proxy-counted** in the MiniMax row until the owner sets per-provider caps (required before M3b) | proxy | — | 0 | 0 |

## Accepted baseline and limits (unchanged from M1)

- M1 owner-accepted at `2048e84`: journal-backed Core/Execution slice, public durable labeled fake-process host, caller operation ledger, content-addressed output, recovery/order fences, truthful fake discovery and packaging skeleton. Evidence: [acceptance corrections](m1/ACCEPTANCE-CORRECTIONS.md), [CHECKPOINTS](m1/CHECKPOINTS.md), issue #3.
- Baseline counts: 20 Cargo tests; runner **206 pass / 73 unsupported / 1 skipped** plus two supplemental passes separately; public matrix **66** (45/6/9/6); diagnostic matrix **54** (39/3/9/3); three repetitions per case.
- **No real harness has run through PIO; all six journeys remain `not_evaluated`.** No native authentication, real-adapter support, end-to-end/TUI journey, release qualification, throughput, total-disk/spool-GC bound or universal exactly-once property. Process cancellation/workspace/usage adapters unavailable. Protocol #9, #10 and the undeclared subscription authorization-recheck barrier remain coverage limits.

## Checks at this checkpoint

This documentation-only commit changes no runtime code, schema, test, CI or evidence file. Documentation checks are recorded in the commit message and issue #5; runtime evidence is the M1 baseline above.

Read-only research before the owner's correction (not a live run, no configuration touched): pinned Codex `e363b08` accepts only `wire_api = "responses"` for custom providers. It is moot for M2 now that PIO never configures providers.

## Resources and next action

- **Active resources:** worktree `pio-m2`; no PIO daemons, hosts, user jobs or live harness sessions running; no temporary stores created yet.
- **Next:** implement and test ADR 002's **32 MiB / 32,768-record pre-commit admission bound** offline (exact boundaries, event/dedupe accounting, explicit capacity refusal, no effect or spawn on refusal); record that checkpoint with clean-clone and two-platform CI evidence before Codex qualification or any live run.
- Resuming session: read the [continuation prompt](m2/KICKOFF.md), then this file and the task packet.
