# Continuation prompt — M2 Codex adapter

Disposable prompt, rewritten 2026-09-16 after M2 started. It replaces the fresh-session kickoff that prepared M2; the decisions and results of that kickoff now live in durable records. **Check [STATE](../STATE.md), [issue #5](https://github.com/Combraton/pio/issues/5) and Git before acting**; if they have moved past this prompt, follow them and rewrite or retire this file.

## 1. Where M2 stands

- Repository `https://github.com/Combraton/pio`; branch **`codex/m2-codex-app-server`** in the single `pio-m2` worktree. Base **`9cf70474c28f549650e6b48e8be20ae88426a1b0`** (merge of PR #4). Do not create another M2 writer or branch.
- PR #4 is merged, issue #3 is closed with a successor comment, and issue #5 tracks M2. The branch's first commit records the owner decisions and MIT license and is documentation only.
- **No M2 runtime work exists yet. No real harness has run through PIO; all six journeys remain `not_evaluated`.** M1's accepted evidence and limits are unchanged; do not turn them into wider claims.

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
- **Test scope:** Codex 0.146.0 (M2), Claude Code 2.1.273 (M3), OpenCode v2.0.1 invoked as `opencode2` and Hermes Agent v0.20.1 (M3b). A separately installed `opencode` 1.18.18 is not the selected OpenCode. OpenCode is preferred for heavy-usage testing.
- **Token caps:** Codex 1,000,000 and Claude Code 1,000,000, each across all its tests; MiniMax through OpenCode and Hermes 300,000,000 combined, with GLM/Kimi counted against it until the owner says otherwise. Record per journey in STATE, stop and report at 80 percent, and never run more than three live sessions at once.
- **Codex home:** use the user's real Codex home. Capture configuration before and after every live run and disclose added trusted-project entries; change nothing else; keep raw snapshots outside Git.
- **Claude Code (M3, not now):** the user's own login, or API key when configured; per-run route evidence, precedence rule, missing-route refusal. Never log out, copy tokens or edit global settings.
- Reserved to the owner: evaluation thresholds/rubric, merge, tag and publication.

## 4. Next action

Implement and test ADR 002's **32 MiB canonical projected-state / 32,768-record pre-commit admission bound** with offline fixtures: exact boundaries, event/dedupe accounting, explicit capacity refusal and no new effect or spawn on refusal. Record the checkpoint (STATE, issue #5, clean-clone sequence, two-platform CI) **before** qualifying Codex or starting any live run. Then follow TASK's plan steps 3–5.

## 5. Protocol and Codex pins

Protocol **v0.1.0** at `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc` via `protocol.lock.json`; build the runner from the verified archive. Protocol #9, #10 and the undeclared subscription authorization-recheck barrier remain coverage limits; gaps go upstream as versioned proposals with demonstrating fixtures, never local schema widening. Codex candidate **0.146.0**, source `e363b08c9175ac1cbe5893615dd2cb9ddf95043b`. Drift checks compare canonical parsed JSON per file, never raw bytes of `codex_app_server_protocol.v2.schemas.json`.

## 6. Environment gotchas

- **Worktrees:** `pio` (old readiness checkout) and `pio-m1` (M1 branch) are preserved; neither is main. Sibling `combraton`, `protocol`, `cbr` and `benchmarks` stay untouched.
- **Unrelated `pio` on PATH:** use this checkout's `target/debug/pio` by absolute path; never overwrite or invoke the historical `pio`. Packaging defaults to the collision-checked `pio-standalone` alias.
- **Pull-request artifacts:** names use `github.event.pull_request.head.sha || github.sha`, never the synthetic merge SHA; compare artifact identity with the reviewed head.
- **Linux user manager:** the Ubuntu CI VM starts `user@$(id -u).service` and exports `XDG_RUNTIME_DIR` before the systemd user-job test. Do not run that provisioning on a workstation or replace missing manager evidence with direct launches.
- **Read-only-store fault:** SQLite files `0400`, store directory `0500`, restored in cleanup; run as a normal user. Store faults come from launch configuration, never request fields. Observer opens of an existing store must not rerun schema/meta writes.
- **Secrets and private data:** harnesses carry their own credentials; PIO work never reads provider keys. Keep credentials, raw configuration snapshots and private paths out of Git and transcripts.
