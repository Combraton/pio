# Current session state — PIO

Dated snapshot; reconcile Git with [M2 issue #5](https://github.com/Combraton/pio/issues/5) and the [M2 task packet](m2/TASK.md). Issues own live progress.

- **Updated:** 2026-09-16. **Owner:** Claude Code session, standalone PIO M2 builder (assigned by the owner). Independent review and owner acceptance remain separate.
- **Task:** M2 — Codex 0.146.0 app-server adapter with real J1/J3/J4/J5. Issue [#5](https://github.com/Combraton/pio/issues/5).
- **Branch/base/head:** `codex/m2-codex-app-server` in the single `pio-m2` worktree. **Base `9cf70474c28f549650e6b48e8be20ae88426a1b0`**, the merge of [PR #4](https://github.com/Combraton/pio/pull/4) on main. Its tree equals reviewed head `a34c408864e8f87abefd6d820518902b31b6a724`, and accepted M1 head `2048e84b0e77b258444f8f30333f4b76584cdccc` is an ancestor. Commits: `0a26d5f` and `b1f716b` record owner decisions (docs only); **`5be40f999484e18e4b3966b898ee4be70691379e` is the tested implementation head** of the capacity-bound checkpoint; a documentation successor records its evidence (SHA in Git history). Draft PR opened at this checkpoint. The earlier `pio` and `pio-m1` worktrees are preserved; no sibling repository modified.
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

## Checkpoint: ADR 002 capacity bound (bounded, not M2 acceptance)

[Report](m2/COMMIT-BOUND.md), [receipt index](m2/evidence/commit-bound.json), [ADR 002 rules](../decisions/002-protocol-journal.md#capacity-bound-2026-09-16).

- **Implemented:** hard pre-staging limits of 32,768 projection records and 33,554,432 canonical bytes on every Protocol commit. Refusal stages nothing and is typed locally; publicly it is the frozen `unavailable` / `same_command` with nothing bound, logged once per distinct refusal. Capacity-refused background commits no longer block queries or replays. Test tooling `pio fake fill-projection` added.
- **Evidence at `5be40f9`:** [CI 35118539258](https://github.com/Combraton/pio/actions/runs/35118539258) passes on macOS 15 arm64 and Ubuntu 24.04 x86_64; the fresh-clone documented sequence runs 14 of 14 commands with exit 0. **28 Cargo tests** (+8) and 1 ignored measurement probe. Runner **206 / 73 / 1** plus 2 supplemental passes, unchanged. **Public matrix 69** (48/6/9/6), with new `capacity_refusal_no_spawn` passing ×3 in every environment. **Diagnostic 54** (39/3/9/3). All 280 fixture classes and all case classes are identical across clean clone, macOS CI and Linux CI. Five local unit mutants were killed for their named reasons.
- **Findings and limits:** at the limit the dispatch marker can commit and the child launch while later observations are refused, so delivery stalls at `pending` (safety holds, liveness does not; room reserved for admitted work would be a new decision). At 32,767 records a commit costs about 200 ms debug and about 30 ms release, and the durable tick commits once per execution, so the bound is not an operating point. No eviction or GC exists. Protocol has no capacity-specific error, and no proposal has been filed.
- Earlier read-only research (no live run, no configuration touched): pinned Codex `e363b08` accepts only `wire_api = "responses"` for custom providers. It is moot now that PIO never configures providers.

## Resources and next action

- **Active resources:** worktree `pio-m2`. No PIO daemons, hosts, user jobs or live harness sessions were running after the checks (process table and launchd list inspected). Temporary matrix stores under `/tmp` and the clean clone and CI artifacts in session scratch space remain outside Git.
- **Owner review 1 (PR #6 at `0f8685b`): continue.** Implemented and verified at tested head **`6a9d11a`**: admission headroom bench values (31,130 records / 31,876,710 bytes, new `execution.submit` only), incremental-commit prerequisite recorded in PLAN M3b/M4, [Protocol #13](https://github.com/Combraton/protocol/issues/13) filed, near-bound tick spacing plus latency evidence. [CI 35130616765](https://github.com/Combraton/pio/actions/runs/35130616765) passes on both platforms; the clean clone ran 14 of 14 commands with exit 0. **31 tests**; runner 206/73/1 + 2; public **72** (51/6/9/6); diagnostic 54. Intermediate failures at `aa7ad78` and `df069bf` are explained in the [report](m2/COMMIT-BOUND.md#owner-review-1-and-corrections-2026-09-16).
- **Codex qualification and capture (`ea6bf17`):** `pio-codex` binds wrapper, Node and native binary hashes and per-file canonical schema identity (checked-in 275-file identity). The real installed codex qualified; drift and wrong-executable controls were refused; the offline probe confirmed the trusted-project write and an app-server pre-connection. Evidence: [codex-qualification](m2/codex-qualification/README.md).
- **Adapter design:** [ADR 003](../decisions/003-codex-app-server-adapter.md), proposed. Protocol gaps filed: [#13](https://github.com/Combraton/protocol/issues/13) (capacity error) and [#14](https://github.com/Combraton/protocol/issues/14) (no content path for digest-only brief, steering and action response).
- **Adapter connected offline (`33d6827`, `5095f06`):** app-server stdio client and labeled fake app-server; the durable Codex host; `serve-codex` with qualification at start, the content extension, workspace/fixture admission, controls, actions, steering, interrupt, usage, exit, lost-host handling and observed-only discovery. The offline matrix `scripts/codex_host_matrix.py` passes 39/39 (13 cases × 3) locally and on both CI platforms. Runner 206/73/1 + 2, public 72 and diagnostic 54 unchanged; 42 tests. No real Codex turn has run.
- **Live-run plan posted** on [issue #5](https://github.com/Combraton/pio/issues/5#issuecomment-5702854789) with fixture paths, exact task texts, per-run token estimates (about 195–440k total) and stop rules (cumulative 800k; 250k per run; unknown usage). **Waiting for the owner's go and the thread-settings gate. No live run before that.**
- **Next:** on go, write the live runner and execute R1–R6 plus the live wrong-executable refusal, with evidence in `docs/work/m2/codex-live/` and usage recorded in the ledger above. Meanwhile, offline hardening only.
- Resuming session: read the [continuation prompt](m2/KICKOFF.md), then this file and the task packet.
