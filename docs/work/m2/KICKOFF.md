# Continuation prompt — M2 Codex adapter

Disposable prompt, rewritten 2026-09-16 after M2 started. It replaces the fresh-session kickoff that prepared M2; the decisions and results of that kickoff now live in durable records. **Check [STATE](../STATE.md), [issue #5](https://github.com/Combraton/pio/issues/5) and Git before acting**; if they have moved past this prompt, follow them and rewrite or retire this file.

## 1. Where M2 stands

- Repository `https://github.com/Combraton/pio`; branch **`codex/m2-codex-app-server`** in the single `pio-m2` worktree. Base **`9cf70474c28f549650e6b48e8be20ae88426a1b0`** (merge of PR #4). Do not create another M2 writer or branch.
- PR #4 is merged, issue #3 is closed with a successor comment, and issue #5 tracks M2 with a draft PR. Owner decisions and the MIT license are recorded.
- **The ADR 002 capacity bound is implemented and checkpointed** at `5be40f9` ([report](COMMIT-BOUND.md)). **No real harness has run through PIO; all six journeys remain `not_evaluated`.** M1's accepted evidence and limits are unchanged; do not turn them into wider claims.

## 2. Read, in order

1. `AGENTS.md`
2. `docs/work/STATE.md`
3. `docs/work/m2/TASK.md` — scope, acceptance, bounds and plan
4. `docs/decisions/001-standalone-stack.md`, especially the **2026-09-16 amendment**
5. `docs/work/standalone-0.1/PLAN.md`, including its owner amendment
6. `docs/decisions/002-protocol-journal.md`
7. `docs/JOURNEYS.md`, `docs/VERIFICATION.md`, `docs/work/m1/ACCEPTANCE-CORRECTIONS.md`, `docs/work/m1/CHECKPOINTS.md`

## 3. Owner decisions that are easy to get wrong

- **PIO drives harnesses as the user configured them.** Never select or inject a model or provider, and never add a provider entry to a harness configuration. An earlier draft that put MiniMax inside Codex was withdrawn by the owner; do not revive it.
- **Test scope:** Codex 0.146.0 (M2), Claude Code 2.1.273 (M3), OpenCode v2.0.1 invoked as `opencode2` and Hermes Agent v0.20.1 (M3b, after M3). A separately installed `opencode` 1.18.18 is not the selected OpenCode. OpenCode is preferred for heavy-usage testing. **Test scope is not release scope:** v0.1 remains Codex and Claude Code.
- **Hermes (later):** only an isolated profile carrying model configuration; never the owner's real Hermes home, which runs their scheduled jobs. If isolation is impossible, defer the Hermes adapter and say so.
- **Token caps:** Codex 1,000,000 and Claude Code a separate 1,000,000, each across all its tests; MiniMax through OpenCode and Hermes 300,000,000 combined. GLM/Kimi bill separately and are proxy-counted against the MiniMax cap only until the owner sets per-provider caps before M3b. Record per journey in STATE, stop and report at 80 percent, and never run more than three live sessions at once.
- **Codex home and fixtures:** use the user's real Codex home. Capture configuration before and after every live run and disclose added trusted-project entries; change nothing else; keep raw snapshots outside Git. Live runs work in a throwaway fixture repository path. Never select the full-access sandbox or `thread/shellCommand` (nor `externalSandbox` or `process/spawn`).
- **Claude Code (M3, not now):** the user's own login, or API key when configured; per-run route evidence, precedence rule, missing-route refusal. Never log out, copy tokens or edit global settings.
- Reserved to the owner: evaluation thresholds/rubric, merge, tag and publication.

## 4. Next action

Owner review 1 is done (corrections verified at `6a9d11a`). Codex qualification (`ea6bf17`) and the offline adapter (`5095f06`, offline matrix 39/39) are in; ADR 003 is proposed. **A live-run plan is posted on issue #5 and waits for the owner's go.** Check #5 for the go and the thread-settings gate before anything live. The original step 3 text follows for reference: qualify the selected Codex executable, exact version, binary hash and per-file canonical schema identity; build before/after capture of the user's Codex configuration; connect app-server through the durable host. No live run until those exist, and none until the owner says go on issue #5 after you post the fixture path, task text, expected token cost and run plan. Then steps 4–5.

## 5. Protocol and Codex pins

Protocol **v0.1.0** at `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc` via `protocol.lock.json`; build the runner from the verified archive. Protocol #9, #10 and the undeclared subscription authorization-recheck barrier remain coverage limits; gaps go upstream as versioned proposals with demonstrating fixtures, never local schema widening. Codex candidate **0.146.0**, source `e363b08c9175ac1cbe5893615dd2cb9ddf95043b`. Drift checks compare canonical parsed JSON per file, never raw bytes of `codex_app_server_protocol.v2.schemas.json`.

## 6. Environment gotchas

- **Worktrees:** `pio` (old readiness checkout) and `pio-m1` (M1 branch) are preserved; neither is main. Sibling `combraton`, `protocol`, `cbr` and `benchmarks` stay untouched.
- **Unrelated `pio` on PATH:** use this checkout's `target/debug/pio` by absolute path; never overwrite or invoke the historical `pio`. Packaging defaults to the collision-checked `pio-standalone` alias.
- **Pull-request artifacts:** names use `github.event.pull_request.head.sha || github.sha`, never the synthetic merge SHA; compare artifact identity with the reviewed head.
- **Linux user manager:** the Ubuntu CI VM starts `user@$(id -u).service` and exports `XDG_RUNTIME_DIR` before the systemd user-job test. Do not run that provisioning on a workstation or replace missing manager evidence with direct launches.
- **Read-only-store fault:** SQLite files `0400`, store directory `0500`, restored in cleanup; run as a normal user. Store faults come from launch configuration, never request fields. Observer opens of an existing store must not rerun schema/meta writes.
- **Secrets and private data:** harnesses carry their own credentials; PIO work never reads provider keys. Keep credentials, raw configuration snapshots and private paths out of Git and transcripts.
