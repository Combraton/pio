# ADR 004 — Claude Code adapter behind the durable host

- Status: **proposed builder design**, 2026-09-20, for independent review on the M3 pull request. Scope: [issue #7](https://github.com/Combraton/pio/issues/7) and the [M3 task packet](../work/m3/TASK.md).
- Supersedes nothing. It decides what [ADR 001](001-standalone-stack.md) left open for M3 and follows the shape of [ADR 003](003-codex-app-server-adapter.md).
- Every fact marked **measured** below was observed on this workstation on 2026-09-20 against Claude Code 2.1.278, at a cost of **zero model tokens**. Facts that are not measured are named as open questions rather than assumed.

## Problem

PIO must drive the user's installed Claude Code as the user would, behind the same durable host, journal, effects and evidence discipline M2 established for Codex — without reading, copying or passing a credential, without widening a permission, and without claiming anything it cannot observe.

## Decisions

### 1. Transport: the executable's own structured input and output, not the SDK bridge

PIO spawns the user's installed executable as

```
claude --print --input-format stream-json --output-format stream-json --verbose
```

and speaks newline-delimited JSON over its stdin and stdout, the same shape the durable Codex host uses for the app-server.

ADR 001 recommended a Python `claude-agent-sdk` bridge, and said the bridge is acceptable **only if** it spawns the user's installed CLI through an explicit path and preserves the user's configuration, instructions, settings, permissions and login. The owner's later decision is that PIO drives the installed executable as the user would. The direct route satisfies that decision literally rather than through a wrapper, and on ADR 001's own criterion — fidelity to the user's configuration — it is strictly better:

- it is the same binary the user runs, resolved and hashed, with no second implementation of session setup between PIO and the harness;
- it adds no Python runtime, no SDK version to qualify, and no third process holding the conversation;
- it never needs a credential in PIO's address space. The harness authenticates itself from the user's own configuration, exactly as at the keyboard.

**PIO never reads, copies or passes credentials.** It does not read `~/.claude/.credentials.json`, it does not read the keychain, and it does not set `ANTHROPIC_API_KEY`. The environment passed to the child is an explicit allowlist, as ADR 003 §3 requires for Codex.

The Anthropic caveat recorded in the [README](../../README.md) stands unchanged: Anthropic's SDK guidance says third-party products must not offer claude.ai login and rate limits without prior approval; PIO operates the user's own installation on the user's own machine and never provisions, resells, stores or copies credentials; the owner accepts that policy risk knowingly and it is not a compliance claim. Choosing the direct executable route does not change that risk either way — the login used is the one already on the machine.

### 2. Qualification, and making re-qualification cheap

`pio claude qualify --executable PATH --work DIR` binds, before any native work:

- the **selected path** and what it resolves to. Measured: `/opt/homebrew/bin/claude` is a symlink into `/opt/homebrew/Caskroom/claude-code@latest/<version>/claude`, so the resolved path carries the version and changes on every self-update;
- the **native binary**: Mach-O arm64, 218 MB, sha256 recorded;
- the **version**, from `--version`;
- a **CLI surface identity**: the canonical digest of `--help` plus each subcommand's help. Measured: `--help` is byte-identical across runs and costs 50 ms, so this is a cheap, reproducible fingerprint of the interface PIO actually drives.

The surface identity is this adapter's analogue of the Codex schema identity. Codex publishes JSON schemas; Claude Code does not, and its stream shapes are not documented, so the interface PIO can pin is the command-line surface plus the measured stream shapes in §7.

**Re-qualification must be cheap because this target moves.** The cask tracks latest and self-updates. A refusal mid-milestone is the feature working, exactly as the Codex 0.146.0 → 0.155.1 refusal was in M2. Qualification is therefore one scripted command whose refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, `surface_drift`), never a crash, and an unqualified executable never receives Claude-specific arguments.

### 3. Credential route: observed, never read

**Measured:** `claude auth status` prints structured JSON, makes no model call, and changed neither `~/.claude.json` nor `~/.claude/settings.json` when run. With the user's real configuration it reports `loggedIn: true`, `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType: "max"`. It **also** prints `email`, `orgId` and `orgName`.

PIO records **only** `loggedIn`, `authMethod`, `apiProvider` and `subscriptionType`. The account identity fields are dropped at the boundary and never reach a receipt, a journal record or a log. The route is also observable in-band: `system/init` carries `apiKeySource`, which PIO records.

**Precedence rule.** When both a login and an API key are usable, the adapter records both as observed and reports which one the harness actually used, from `apiKeySource` in `system/init`. PIO does not choose between them and does not set either.

**Negative control, measured.** With an isolated `CLAUDE_CONFIG_DIR` and no credential variables, `auth status` reports `loggedIn: false`, `authMethod: "none"`. A launch attempted anyway emits `system/init` with `apiKeySource: "none"`, then an `assistant` message carrying `error: "authentication_failed"` and the text `Not logged in · Please run /login`, then a `result` with `is_error: true`, `terminal_reason: "api_error"` and **zero usage**. PIO refuses before spawning on the `auth status` observation; the spawned shape above is recorded so the refusal can be proven to precede it.

### 4. Permissions

- PIO **never** passes `--dangerously-skip-permissions` or `--allow-dangerously-skip-permissions`. Neither appears in the adapter, in a test, or behind a flag.
- **The guard requires the requested permission mode to equal the user's configured default.** Measured: `permissions.defaultMode` is `acceptEdits` in the user's settings, and `system/init` echoes the effective `permissionMode`, so both sides are observable.
- **The breadth ordering of the six modes is not established.** `--permission-mode` accepts `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk` and `plan`. A documentation pass produced an ordering that contradicts itself — it placed `dontAsk`, which auto-denies everything that would prompt, as *broader* than `acceptEdits` — so it is not usable as a safety property. Rather than encode a guess, the guard compares for equality and refuses every other value, `bypassPermissions` unconditionally. A mode may be added to a narrower-than set only with a measurement that shows it. This is the same discipline that caught the Codex trusted-project default being true only for already-trusted projects.
- **Every M3 run therefore requests exactly `acceptEdits`**, the configured default, and requests no change.
- **Deny and allow runs use a shell command, not an edit.** Under `acceptEdits` file edits do not prompt, so an edit cannot exercise the decision path. The command must also avoid the built-in read-only set, the filesystem commands `acceptEdits` auto-approves, and the user's 38 configured allow entries — which include `Bash(python3 -:*)`, so the M2 fixture's `python3 -m unittest -q` would be pre-approved and is not usable here. The chosen command is recorded in the run plan and verified against the allow list first.
- **Only single-use decisions are forwarded.** No session-scoped grant, no "always allow", no rule update, no write to any settings file. The wire shape of a permission response is undocumented, so the adapter sends only the minimal allow or deny form it has measured, and refuses any decision it cannot prove is single-use — the same rule that limited Codex to `accept`, `decline` and `cancel`.

### 5. Plugins, hooks, MCP servers and tools

An as-configured session loads the user's **eleven enabled plugins** with their hooks and outward-facing tools. That is part of running the harness as configured and is not disabled.

**Measured:** `system/init` names everything PIO needs: `tools`, `mcp_servers`, `plugins`, `slash_commands`, `skills`, `agents`, plus `capabilities`, `model`, `permissionMode`, `apiKeySource`, `session_id` and `claude_code_version`.

- Receipts record **plugin, hook, MCP server and tool names only** — never their arguments, content or output.
- **Briefs are written so that no plugin, hook, MCP server or slash command is needed** to complete the fixture task.
- **Every tool request whose target lies outside the fixture workspace is declined**, with the reason recorded, the way the Codex host declines requests it must not answer. An offline matrix case proves it against the labeled fake.
- `system/init` also reports a `messaging_socket_path` under `/tmp`. It is recorded as an observed outward surface; PIO neither connects to it nor exposes it.

### 6. Durable state disclosure

Snapshotted before and after every run, reported by digest with fixture labels: the **settings file**, **`~/.claude.json`**, and a **listing of the project transcript directory**. PIO edits and removes nothing.

**`~/.claude.json` is volatile and a whole-file digest is not disclosure.** Measured: it holds 109 top-level keys, most of them counters, caches and timestamps — `numStartups`, `promptQueueUseCount`, cached experiment and feature data with fetch times — that change whenever Claude Code starts, whoever started it. A before-and-after digest would report "changed" on every run and tell the reader nothing.

The diff therefore separates:

- **what the run caused**: a new entry under `projects` for the fixture path, including `hasTrustDialogAccepted`, which is this harness's analogue of the Codex trusted-project entry. Note that `--print` skips the workspace trust dialog, so a non-interactive run trusts the directory without asking;
- **bookkeeping**: counters, caches and timestamps, reported as a count of changed keys and nothing more.

**`oauthAccount` is never recorded, in any form**, not even as a list of its keys: measured, it holds the account's email address, full name, organization name and identifiers.

Project entries also carry per-project token counters — `lastTotalInputTokens`, `lastTotalOutputTokens`, the cache counters and `lastCost`. PIO records these as a **secondary** usage observation only; the stream's own report is primary.

### 7. The stream, and what PIO may claim from it

Measured shapes, from a credential-free probe that spent nothing:

- **`system/init`** — first message, contents as §5.
- **`user` with `isReplay: true`** — with `--replay-user-messages`, the harness echoes back the exact user message PIO sent. This is the **delivery acknowledgment**, the analogue of Codex's `turn/start` response, and the evidence class for a `provider_ack_id` delivery proof. Input shape, measured: `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"…"}]}}`.
- **`assistant`** — model messages, carrying a `usage` block and, on failure, `error` and `is_api_error_message`.
- **`result`** — end of turn, carrying `usage` (input, output, cache creation and read, thinking tokens), `modelUsage` per model, `total_cost_usd`, `permission_denials`, `num_turns`, `is_error`, `terminal_reason`, `duration_ms` and `session_id`.

**Open, to be measured before the stops are set** (all declared in the live-run plan on issue #7):

1. **Usage granularity.** `result` reports usage once per turn. Whether `assistant` messages carry usable incremental usage during a real turn is unmeasured. **If usage only lands at turn end, a mid-turn token stop is impossible in this transport** and the runner's limit can only prevent the next turn. M2 set a 50,000 stop against a harness reporting roughly every 24,000 tokens and stopped at 72,911; M3 will not repeat that by assuming.
2. **Interrupt.** `system/init` advertises `capabilities: ["interrupt_receipt_v1","interrupt_cancel_queued_v1","msg_lifecycle_v1"]`, which suggests an in-band interrupt exists, while the published documentation describes only SIGINT and SIGTERM. Until the in-band form is measured, PIO's cancel is **SIGINT to the child**, described as exactly that and never as an in-band control. SIGTERM is not used: it leaves the turn unfinished with no recorded result.
3. **Permission request and response shapes** under `--permission-prompts host`, which are undocumented.

Until each is measured, the corresponding property is `not_evaluated`. PIO does not claim a cancel it cannot observe, a usage number it did not receive, or a permission decision it cannot prove single-use.

## Consequences and verification

- A labeled fake Claude harness (`pio claude fake-cli`) speaks these measured shapes so the offline matrix runs in CI with no Claude Code installed, exactly as `pio codex fake-app-server` does.
- The offline matrix must cover, at minimum: a turn that completes; the replay acknowledgment; a declined out-of-fixture tool request; a permission decision denied and allowed; an unqualified executable refused at service start; the missing-credential-route refusal before any spawn; a requested permission mode that is not the configured default, refused before any spawn; and surface drift refused.
- Journeys are marked from live evidence only, in the shared model's vocabulary. Nothing here accepts a journey.

## Out of scope

The TUI (M4), CBR composition (M5), OpenCode and Hermes (M3b), and product budget enforcement, which M2 recorded as unproven and M3 does not attempt.
