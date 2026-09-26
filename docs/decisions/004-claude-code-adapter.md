# ADR 004 — Claude Code adapter behind the durable host

- Status: **proposed builder design**, 2026-09-20, for independent review on the M3 pull request. Scope: [issue #7](https://github.com/Combraton/pio/issues/7) and the [M3 task packet](../work/m3/TASK.md).
- Supersedes nothing. It decides what [ADR 001](001-standalone-stack.md) left open for M3 and follows the shape of [ADR 003](003-codex-app-server-adapter.md).
- Every fact marked **measured** below was observed on this workstation on 2026-09-20 against Claude Code 2.1.278, at a cost of **zero model tokens**, by `scripts/claude_requalify.py`. Facts that are not measured are named as open questions rather than assumed. Facts read from the pinned SDK source are marked **from source** and are evidence of the protocol, not a measurement of 2.1.278.

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

The SDK is still **read** as documentation of the control protocol the same CLI speaks (§9). It is inspected, never installed or executed, exactly as ADR 001 recorded.

**PIO never reads, copies or passes credentials.** It does not read `~/.claude/.credentials.json`, it does not read the keychain, and it does not set `ANTHROPIC_API_KEY`. Every child process runs with a **cleared** environment and an explicit allowlist, as ADR 003 §3 requires for Codex. The allowlist is `PATH`, `HOME`, `USER`, and `CLAUDE_CONFIG_DIR` only when isolating — see §3 for why `USER` is on it and why `CLAUDE_CONFIG_DIR` is not set for an as-configured run.

The Anthropic caveat recorded in the [README](../../README.md) stands unchanged: Anthropic's SDK guidance says third-party products must not offer claude.ai login and rate limits without prior approval; PIO operates the user's own installation on the user's own machine and never provisions, resells, stores or copies credentials; the owner accepts that policy risk knowingly and it is not a compliance claim. Choosing the direct executable route does not change that risk either way — the login used is the one already on the machine.

### 2. Qualification, and making re-qualification cheap

`pio claude qualify --executable PATH --work DIR` binds, before any native work:

- the **selected path** and what it resolves to. Measured: `/opt/homebrew/bin/claude` is a symlink into `/opt/homebrew/Caskroom/claude-code@latest/<version>/claude`, so the resolved path carries the version and changes on every self-update;
- the **native binary**: Mach-O arm64, 218 MB, sha256 recorded;
- the **version**, from `--version`, which prints `2.1.278 (Claude Code)` and is parsed to its first token;
- a **CLI surface identity**: the canonical digest of `--help` plus each subcommand's help. Measured: `--help` is byte-identical across runs and costs 50 ms, so this is a cheap, reproducible fingerprint of the interface PIO actually drives.

**A help digest cannot see the wire.** Two builds can present the same command line and emit different messages, so the surface identity is not sufficient on its own. Qualification therefore pins a second artefact, the **stream identity**: the `system/init` key set, the capability list, the message sequence, the `result` key set, and the product's own default permission mode and model. Both are produced by **one command**, `scripts/claude_requalify.py`, which reruns the zero-token probe and compares both identities; `--update-baseline` regenerates them. Exit 0 clean, 3 on drift, 4 if the user's configuration moved under it.

**Re-qualification must be cheap because this target moves.** The cask tracks latest and self-updates. A refusal mid-milestone is the feature working, exactly as the Codex 0.146.0 → 0.155.1 refusal was in M2. Refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, `surface_drift`, `stream_drift`), never a crash, and an unqualified executable never receives Claude-specific arguments.

### 3. Credential route: observed, never read

**Measured:** `claude auth status` prints structured JSON, makes no model call, and changed neither `~/.claude.json` nor either settings file when run. With the user's real configuration it reports `loggedIn: true`, `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType: "max"`. It **also** prints `email`, `orgId` and `orgName`.

PIO records **only** `loggedIn`, `authMethod`, `apiProvider` and `subscriptionType`. The account identity fields are dropped at the boundary and never reach a receipt, a journal record or a log. The route is also observable in-band: `system/init` carries `apiKeySource`, which PIO records.

**Precedence rule.** When both a login and an API key are usable, the adapter records both as observed and reports which one the harness actually used, from `apiKeySource` in `system/init`. PIO does not choose between them and does not set either.

**The environment allowlist is part of the route, and getting it wrong is silent.** Two measurements forced this:

- **`USER` must be passed.** Given the user's real `HOME` but no `USER`, `auth status` reports `loggedIn: false`, `authMethod: "none"`. Adding `USER` alone restores `loggedIn: true`, `authMethod: "claude.ai"`, `subscriptionType: "max"`. Nothing else in the environment makes a difference — `SECURITYSESSIONID`, `TMPDIR` and `__CF_USER_TEXT_ENCODING` were each tried and did not.
- **`CLAUDE_CONFIG_DIR` must be left unset for an as-configured run.** Setting it to `~/.claude` makes the harness report `Claude configuration file not found at: ~/.claude/.claude.json`: the product expects that file *inside* the configured directory, while as the user has it, it sits at `~/.claude.json` beside `~/.claude/`.

The adapter committed in `85f83c1` did both wrong — it set `CLAUDE_CONFIG_DIR` to the user's `.claude` directory and passed no `USER` — so `pio claude auth-route` reported the owner as **not logged in** and exited 3, refusing a route that works. The failure was fail-safe but it was not harmless: it would have blocked every live run while looking exactly like the missing-route feature working correctly. `ChildEnv::as_configured` and `ChildEnv::isolated` now make the two cases explicit, and a test with a fake that reads `USER` fails if the variable stops being passed.

**This also repairs the negative control.** The control must differ from the real route in exactly one thing — the configuration the harness can see — or a refusal could be an artefact of the environment. Measured with `USER` present in both: isolating `HOME`, or isolating `CLAUDE_CONFIG_DIR`, each removes the route on its own. The control is sound.

**Negative control, measured.** With an isolated `CLAUDE_CONFIG_DIR` and a cleared environment, `auth status` reports `loggedIn: false`, `authMethod: "none"`. A launch attempted anyway emits `system/init` with `apiKeySource: "none"`, then an `assistant` message carrying `error: "authentication_failed"` and the text `Not logged in · Please run /login`, then a `result` with `is_error: true`, `terminal_reason: "api_error"` and **zero usage**. PIO refuses before spawning on the `auth status` observation; the spawned shape above is recorded so the refusal can be proven to precede it.

### 4. Permissions

- PIO **never** passes `--dangerously-skip-permissions` or `--allow-dangerously-skip-permissions`. Neither appears in the adapter, in a test, or behind a flag.
- **The guard requires the requested permission mode to equal the user's configured default.** Measured: `permissions.defaultMode` is `acceptEdits` in the user's settings, and `system/init` echoes the effective `permissionMode`, so both sides are observable.
- **The breadth ordering is partially documented and still not measured.** The installed `--permission-mode` accepts `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk` and `plan`. The pinned SDK's `PermissionMode` is `default`, `acceptEdits`, `plan`, `bypassPermissions`, `dontAsk`, `auto` — **the two disagree on the name of the standard mode**, and measured, with no flag, `system/init` reports `permissionMode: "default"`, a name the flag does not accept. Whether `manual` is the flag spelling of `default` is **not established**.

  From source, the SDK documents five of the six: `default` prompts for dangerous operations; `acceptEdits` auto-accepts file edits; `bypassPermissions` bypasses all checks; `plan` executes no tools; `dontAsk` does not prompt and **denies anything not pre-approved**. `auto` is listed in the type and described nowhere. That yields a partial order — `bypassPermissions` broadest, `acceptEdits` broader than `default` for edits, `dontAsk` and `plan` narrower — and it also confirms why the earlier documentation pass was unusable: it had placed `dontAsk`, which auto-denies, *above* `acceptEdits`. But this is a docstring in a bridge PIO does not use, not a measurement of 2.1.278, and one mode remains undescribed. **The guard therefore stays equality-only**, refusing every value but the configured default and refusing `bypassPermissions` unconditionally. A mode joins a narrower-than set only with a measurement.
- **Every M3 run requests exactly `acceptEdits`**, the configured default, and requests no change.
- **The requested mode cannot be verified in-band before the brief is released.** Measured: with stdin held open for 20 s and nothing written, the child emits **nothing at all** — not `system/init`. After the write, `init` arrives in 0.21 s. So the check the reviewer asked for is real but it is a check *after* delivery. The mode is therefore controlled where it is actually decided — the argument vector, checked before spawn — and the `init` echo is corroboration. If they differ, PIO has already delivered the brief: it aborts the turn, and the receipt records that delivery preceded the check rather than claiming a pre-flight guarantee.
- **Deny and allow runs use a shell command, not an edit.** Under `acceptEdits` file edits do not prompt, so an edit cannot exercise the decision path. The command must also avoid the built-in read-only set, the filesystem commands `acceptEdits` auto-approves, and the user's configured allow entries (§5) — which include `Bash(python3 -:*)` and a bare `Bash(cat)`, so the M2 fixture's `python3 -m unittest -q` would be pre-approved and is not usable here. The chosen command is recorded in the run plan and verified against both allow lists first.
- **Only single-use decisions are forwarded**: `allow` echoing the original input unchanged, or `deny` with a message. Nothing else. §9 names the three widening paths in the protocol and PIO sends none of them.

### 5. Containment, stated honestly

Claude Code **does** have a sandbox: from source, `sandbox.enabled` isolates bash commands on macOS and Linux, and its default is **`False`**.

**The user's settings have no `sandbox` key, 38 `Bash(...)` allow rules across two files, and no deny and no ask rules.** Measured: 41 allow entries in total — 38 in `~/.claude/settings.json`, 3 in `~/.claude/settings.local.json`. PIO does not enable the sandbox, because turning it on would change how the user's harness runs and M3's premise is the harness as configured.

The consequence, stated plainly: **a pre-approved command never produces a permission request, so it never reaches PIO.** Declining out-of-fixture tool requests covers **prompts only**. It is not containment of the harness. Anything the allow rules already permit — `Bash(cat)`, `Bash(chmod:*)`, `Bash(pkill -f mongod)` — runs without PIO ever seeing a decision to make.

Therefore:

- every `tool_use` block in the stream is recorded in the receipt by **tool name, target digest and a fixture-relative label** — `<fixture>/src/calc.py` or `<outside>`, never a raw path and never the absolute fixture path, as the Codex receipts did;
- any target outside the fixture workspace is flagged **`out_of_fixture_effect_observed`, with unresolved liability** — PIO observed it and did not authorize it, and calling that "declined" would be false;
- every receipt carries `containment: {"mechanism": "harness_permission_rules_only", "os_sandbox_observed": false}`.

**Placement is resolved, not matched.** A prefix test on the raw string is wrong in both directions, and the reviewer's probe showed both: `<fixture>/../../outside.txt` was called contained, and a relative `calc.py` was called outside. A target is therefore resolved the way the filesystem would resolve it, without requiring it to exist — relative paths against the session's working directory from `system/init`, `.` and `..` removed component by component, and a symlink followed wherever one exists, which is the only way a link out of the fixture is visible at all. Traversal, relative and symlink cases are tested, and a symlink loop terminates instead of hanging.

**The decline is PIO's.** `classify_permission_request` inspects each `can_use_tool` request: a path-bearing input resolving outside the fixture is **declined with a recorded reason**; anything else — including a shell command, whose targets PIO cannot resolve — is **surfaced to the caller as a Protocol action**. PIO never auto-allows; an allow is always somebody's decision. Until the service binding exists the offline matrix transports that decision to the fake, and the case says so rather than implying the host made it.

`--restricted` and `--safe-mode` are **not** used. Both change which of the user's customizations load — `--safe-mode` sets `CLAUDE_CODE_SAFE_MODE=1` — so neither is the harness as configured, and a containment claim bought by ignoring the user's settings would not be the thing M3 is measuring.

### 6. Plugins, hooks, MCP servers and tools

An as-configured session loads the user's **eleven enabled plugins** with their hooks and outward-facing tools. That is part of running the harness as configured and is not disabled.

**Measured:** `system/init` names everything PIO needs: `tools`, `mcp_servers`, `plugins`, `slash_commands`, `skills`, `agents`, plus `capabilities`, `model`, `permissionMode`, `apiKeySource`, `session_id`, `cwd`, `memory_paths` and `claude_code_version`.

- Receipts record **plugin, hook, MCP server and tool names only** — never their arguments, content or output.
- **Briefs are written so that no plugin, hook, MCP server or slash command is needed** to complete the fixture task.
- **Every tool request whose target lies outside the fixture workspace is declined**, with the reason recorded, the way the Codex host declines requests it must not answer. An offline matrix case proves it against the labeled fake. §5 states what this does and does not cover.
- `system/init` also reports a `messaging_socket_path` under `/tmp`. It is recorded as an observed outward surface; PIO neither connects to it nor exposes it.
- **The name lists are the evidence that the user's configuration was honoured** (§8), not the model field.

### 7. Durable state disclosure

Snapshotted before and after every run, reported by digest with fixture labels: **`~/.claude/settings.json`**, **`~/.claude/settings.local.json`**, **`~/.claude.json`**, and a **listing of the project transcript directory**. PIO edits and removes nothing.

**Everything here is keyed by the session's working directory, which is the workspace repository.** The harness slugs that path for its transcript directory and uses it verbatim for the `projects` entry. The directory a runner happens to create its fixtures in is a parent of the workspace and is neither: keying on it names a path the harness never writes to, so the listing is empty before and after and the diff reports that nothing was written. That is what R1 did; the disclosure below records it. The listing **descends**, because a directory that appears during a run is state the run created.

`settings.local.json` is in the set because it exists on this workstation and carries permission rules of its own; a snapshot that missed it would under-report the user's configured allow list by three entries and miss a bare `Bash(cat)`.

**`~/.claude.json` is volatile and a whole-file digest is not disclosure.** Measured: it holds 109 top-level keys, most of them counters, caches and timestamps — `numStartups`, `promptQueueUseCount`, cached experiment and feature data with fetch times — that change whenever Claude Code starts, whoever started it. A before-and-after digest would report "changed" on every run and tell the reader nothing.

The diff therefore separates:

- **what the run caused**: a new entry under `projects` for the workspace path, including `hasTrustDialogAccepted`, which is this harness's analogue of the Codex trusted-project entry. Note that `--print` skips the workspace trust dialog, so a non-interactive run trusts the directory without asking;
- **bookkeeping**: counters, caches and timestamps, reported as a count of changed keys and nothing more.

**`oauthAccount` is never recorded, in any form**, not even as a list of its keys: measured, it holds the account's email address, full name, organization name and identifiers.

Project entries also carry per-project token counters — `lastTotalInputTokens`, `lastTotalOutputTokens`, the cache counters and `lastCost`. PIO records these as a **secondary** usage observation only; the stream's own report is primary.

### 8. The stream, and what PIO may claim from it

Measured shapes, from a credential-free probe that spent nothing:

- **`system/init`** — first message, contents as §6.
- **`user` with `isReplay: true`** — with `--replay-user-messages`, the harness echoes back the exact user message PIO sent. This is the **delivery acknowledgment**, the analogue of Codex's `turn/start` response, recorded with evidence class `native_replay_echo`. **It earns no proof class** (corrected 2026-09-26, D7): the echo returns the message PIO sent and no identifier of the harness's own, so, like OpenCode's `native_session_update`, it is evidence of receipt and not a `provider_ack_id`. Receipts R1–R7 were recorded with `provider_ack_id` before the correction and are not rewritten. Input shape, measured: `{"type":"user","message":{"role":"user","content":[{"type":"text","text":"…"}]}}`.
- **`assistant`** — model messages, carrying a `usage` block and, on failure, `error` and `is_api_error_message`.
- **`result`** — end of turn, carrying `usage` (input, output, cache creation and read, thinking tokens), `modelUsage` per model, `total_cost_usd`, `permission_denials`, `num_turns`, `is_error`, `terminal_reason`, `duration_ms` and `session_id`.

**The model field is not evidence of fidelity.** Measured: with an empty configuration directory, a cleared environment and no flags, `system/init` still reports `model: "claude-opus-5[1m]"` — the product's own default, which happens to equal the owner's configured `opus[1m]`. A receipt that showed Opus would therefore prove nothing about whether the user's settings were read. The discriminator is the name lists: the same empty-configuration run reported `plugins: []` and `mcp_servers: []`, against eleven plugins as configured.

**Open, to be measured before the stops are set** (all declared in the live-run plan on issue #7):

1. **Usage granularity.** `result` reports usage once per turn. Whether `assistant` messages carry usable incremental usage during a real turn is unmeasured. **If usage only lands at turn end, a mid-turn token stop is impossible in this transport** and the runner's limit can only prevent the next turn. M2 set a 50,000 stop against a harness reporting roughly every 24,000 tokens and stopped at 72,911; M3 will not repeat that by assuming. `--include-partial-messages` and the empty `usage.iterations` array are the two candidates to measure.
2. **Interrupt**, as §9.
3. **Permission request and response shapes** on the wire under `--permission-prompts host`, against the shapes §9 reads from source.

Until each is measured, the corresponding property is `not_evaluated`. PIO does not claim a cancel it cannot observe, a usage number it did not receive, or a permission decision it cannot prove single-use.

### 9. The control protocol, read from the pinned SDK source

**From source**, `claude-agent-sdk-python` 0.2.153 at `763922b0de2c9fb5371504d73ae54cde49536fe1`, inspected and never installed or executed. These are the shapes the SDK speaks to the same CLI PIO drives. They are evidence of the protocol, **not** a measurement of 2.1.278, and each is confirmed on the wire before it is relied on.

**Interrupt** — host to CLI:

```json
{"type":"control_request","request_id":"req_<n>_<hex>","request":{"subtype":"interrupt"}}
```

answered by `{"type":"control_response","response":{"subtype":"success","request_id":"…"}}`, or `subtype: "error"` with an `error` string.

**Permission request** — CLI to host:

```json
{"type":"control_request","request_id":"…","request":{"subtype":"can_use_tool",
 "tool_name":"…","input":{…},"tool_use_id":"…","permission_suggestions":[…],
 "blocked_path":…,"decision_reason":…,"title":…,"display_name":…,"description":…}}
```

**Permission response** — host to CLI, the decision wrapped as `{"type":"control_response","response":{"subtype":"success","request_id":"…","response":<decision>}}`:

- allow — `{"behavior":"allow","updatedInput":<the original input, unchanged>}`
- deny — `{"behavior":"deny","message":"<reason>"}`

**The three widening paths, which PIO never sends:**

1. **`updatedPermissions`** on an allow. Each entry is a `PermissionUpdate` with `type` in `addRules`, `replaceRules`, `removeRules`, `setMode`, `addDirectories`, `removeDirectories` and `destination` in `userSettings`, `projectSettings`, `localSettings`, `session`. This single field is both the "always allow" path and the rule-update path, and `destination: "userSettings"` **writes the user's settings file** — which §7 forbids outright.
2. **`set_permission_mode`**, a control request that changes the mode mid-session and would defeat the §4 guard after the fact.
3. **`permission_suggestions`**, which arrives in the *request* as the CLI's offer of exactly those updates. PIO records that suggestions were offered, and their count, and acts on none.

`updatedInput` echoes the original input byte-for-byte. Altering it would change the tool call the user's harness decided to make, which is a different kind of overreach from widening but is refused for the same reason. A deny may carry `interrupt: true`; it stops the turn rather than widening anything, and M3 does not use it.

This is the same discipline that limited the Codex adapter to `accept`, `decline` and `cancel`: an allowlist of decisions that are provably single-use, and a refusal of anything outside it.

**Interrupt, and the cost of the fallback.** The in-band form above is preferred once measured against 2.1.278, and `capabilities` advertising `interrupt_receipt_v1` is consistent with it existing. Until then PIO's cancel is **SIGINT to the child**, described as exactly that. **If SIGINT remains the fallback, it is not free:** in `--print` mode a signal may end the process before the `result` message, and `result` is the only place turn usage is reported — so a signal-cancelled turn leaves **usage unknown**, and the receipt must record it as unknown rather than as zero. SIGTERM is not used: it leaves the turn unfinished with no recorded result at all.

**The wait after the signal is bounded.** A harness that ignores SIGINT must not hold the host open, so the host waits a recorded interval and then kills the child, recording the escalation from SIGINT to SIGKILL, how long it waited, and that usage is unknown. A fake that ignores the signal proves it.

**A request PIO will not answer still gets an answer.** Anything other than a tool permission request receives the control protocol's error response — `{"type":"control_response","response":{"subtype":"error","request_id":…,"error":…}}`, the shape §9 reads from the pinned SDK — because recording a decline while sending nothing would leave the harness waiting on a decision nobody will make.

### 10. Steering, recovery and resume

- **A second user message sent mid-turn** is writable: stdin stays open and another `{"type":"user",…}` line can be sent. With `--replay-user-messages` the harness echoes it, and **that echo proves delivery and nothing more**. Whether the running turn incorporates it, queues it for the next turn, or drops it is **not observed**. M3 claims no steering. The `msg_lifecycle_v1` and `interrupt_cancel_queued_v1` capabilities are consistent with a queue existing; that is a hint, not a measurement.
- **The host holds the pipes across a daemon kill**, exactly as for Codex. The conversation lives in the host process that owns the child's stdin and stdout, not in the CLI that started it, so a daemon restart replays from the last durable journal record rather than reconnecting to a live turn.
- **Session resume is not used.** `--resume`, `--continue` and `--fork-session` are never passed. Every run is a fresh session with a recorded `session_id`, so no receipt depends on state PIO did not create.

## Consequences and verification

- A labeled fake Claude harness (`pio claude fake-cli`) speaks these measured shapes so the offline matrix runs in CI with no Claude Code installed, exactly as `pio codex fake-app-server` does.
- The offline matrix must cover, at minimum: a turn that completes; the replay acknowledgment; a declined out-of-fixture tool request; a permission decision denied and allowed; a widening decision refused; an unqualified executable refused at service start; the missing-credential-route refusal before any spawn; a requested permission mode that is not the configured default, refused before any spawn; and surface or stream drift refused.
- Journeys are marked from live evidence only, in the shared model's vocabulary. Nothing here accepts a journey.

**Built:** the labeled fake (`pio claude fake-cli`), the admission decisions, the durable host on the shared lifecycle, the service binding (`pio serve-claude`), and a thirteen-case offline matrix of which two run through the service. **Not yet covered:** restart, reattach and host-loss behaviour for this harness, which the Codex matrix covers and this one does not.

### Disclosure: a defect in the first probe

The probe committed in `b5e0f6b` **filtered** the environment — dropping variables whose names contained `ANTHROPIC`, `API_KEY` or `TOKEN` — instead of clearing it, so the `CLAUDE_CODE_*` variables exported by the enclosing Claude Code session reached the child. The Rust adapter always cleared the environment; the script did not match it.

It is now controlled and fixed. A cleared-environment run and an inherited-environment run return identical `permissionMode`, `model`, `capabilities` and name-list lengths, so **no measurement in `b5e0f6b` changed**, and `scripts/claude_requalify.py` now clears the environment for every child.

The same disclosure answers the reviewer's question about that probe reporting `acceptEdits` and Opus. The mode came from a flag the probe itself passed, `--permission-mode acceptEdits`; the control run without the flag reports `default`. The model is the **product default**, reproduced with an empty configuration and a cleared environment, and it is not evidence of a leak — nor, as §8 now records, evidence of fidelity.

### Disclosure: attempts recorded as effects, and a host that never attached

Two findings from R3b, and the second corrects the first two containment runs.

**PIO was never attached as the host that answers permission prompts.** Attaching takes two things and PIO had neither: `--permission-prompt-tool stdio`, which is what makes the CLI send permission requests over the control protocol, and the `initialize` handshake that announces the host. PIO passed `--permission-prompts host`, named no prompt tool and sent no handshake, so the CLI denied anything that would prompt and answered its own model with *"This command requires approval"*. Measured at zero tokens on 2.1.278: the flag is accepted, and the handshake is answered `subtype: success` in about 0.7 s with `pending_permission_requests` among its keys. Unlike a user message, that write does **not** trigger `system/init`, so the effective mode still cannot be checked before delivery.

**A tool use the harness refused is an attempt, not an effect.** The `result` names every refusal by `tool_use_id`, and PIO recorded that list in its own event stream while `tool_use_records` ignored it. R6 therefore reported an out-of-fixture effect with unresolved liability for a read the harness had refused outright — nothing was read. R3 counted a denied compound command among its effects. Both receipts carry addenda with the record recomputed from the stored evidence.

The corrected reading of the three runs: `touch` inside the workspace **ran** unprompted, a compound command and a read outside the workspace were **refused by the harness itself**, and in every case PIO was never asked.

**A side measurement worth keeping:** the child's shell is the user's, with their aliases — R3's `ls` resolved to `eza`, which is not on the child's `PATH`, and the command failed. "As configured" reaches further than the settings files.

### The ledger carries observed and charged

Owner decision of 2026-09-20, after R5. A cancelled turn reports an empty usage block, so what it spent is unknown — and a cap kept in unknowns is not a cap. The ledger therefore carries two numbers per run: **observed**, what the harness reported, and **charged**, what the cap is measured against. They differ only where a turn reported nothing.

A turn cancelled **inside its first model call** is charged a flat **40,000**. The basis is the only two single-call turns measured: R1 at 33,793 and R5 attempt 1 at 32,957. **Every stop rule is applied to charged**, and both numbers, with this basis, appear in the ledger and in every receipt.

### Disclosure: a cancelled turn counted as zero

R5's cancel worked and PIO mis-recorded what came back. The harness answers SIGINT with a `result` whose usage block is empty; the host turned that into an observation, and the protocol was told `basis: observed, amount: 0, liability: resolved` for a turn that had spent a session's prefix plus five seconds of generation. The ledger counted nothing and no stop rule fired, because the rule tested whether a report was *present*.

Two further defects surfaced with it. The total summed `input_tokens` and `output_tokens` only, which for R2 is **129 against the 64,375 the cap actually counts** — on this harness the cache parts are most of a turn. And the per-run token limit appeared in every receipt while being enforced nowhere.

All three are fixed and covered offline: an empty usage block becomes `usage_unknown` and never reaches the protocol as a measurement, the total is the cap's four parts, and a reported zero or a run over its own limit each stop the sequence. The labeled fake now reproduces the measured abort, so the case fails if the host ever reports zero again.

### Disclosure: a false durable-state statement in R1

The R1 receipt said `new_transcript_entries: 0`. It was false. `~/.claude/projects/` held a directory for the run's workspace with a 194,375-byte session file and a `memory` directory beside it, both created during the turn.

The cause was the key, not the observation: the host passed the directory fixtures are created in to the snapshot, so the listing slugged that path instead of the workspace beneath it. The slugged directory does not exist and never will, so the before and the after were both empty and the diff subtracted one nothing from another. The same key was used for the `projects` lookup, so `workspace_project_created` was false for the same reason.

The offline matrix could not have caught it: nothing in it exercised the durable diff at all, and the labeled fake wrote no transcript. Both are fixed — the fake now writes the shape the harness writes, and a case asserts the diff reports it.

The same mistake made the **containment boundary** the fixtures area rather than the workspace, which would have called a sibling fixture contained. R1 used no tool, so nothing was misclassified; the boundary is now the workspace and a matrix case proves a sibling is outside.

[R1's receipt](../work/m3/claude-live/R1.json) carries a dated addendum rather than a rewrite, with the directory as observed after the fact.

## Named unmeasured items and obligations

- **Re-pinned to 2.1.281** (2026-09-26), at zero tokens with every child isolated; the drift from 2.1.278 and what was not re-measured are in [the qualification record](../work/m3/claude-qualification/README.md#re-pin-to-21281-2026-09-26). The measurements below remain 2.1.278's unless that record says otherwise. One of them moved: an empty configuration now reports two plugins, so §8's discriminator is the plugin names, not an empty list.
- ~~Usage-reporting granularity~~ **measured on R1**: `assistant` messages carry a full `usage` block and `result.usage.iterations` holds one entry per model call, so a mid-turn stop is possible. The host still records usage once, from `result`, so PIO's stops remain next-turn stops until it reads per-message usage. That gap is PIO's, not the harness's.
- **Obligation, before any further cancel run** (owner, 2026-09-20), in order: the host reads **per-message** usage rather than only `result`; then, with partial messages enabled, measure whether the **start of each model call** reports its input and cache tokens. If it does, a cancelled turn's unknown becomes a measured lower bound rather than an allowance.
- **Usage is not knowable on cancel** (measured on R5). SIGINT is answered: a `result` arrives with `terminal_reason: aborted_streaming`, `status: failed`, an empty `iterations` and every usage part zero. The tokens the turn spent before the signal are **unaccounted**, and PIO records them as unknown rather than zero. The defect this found is recorded in the disclosure below.
- The in-band interrupt and the permission wire shapes (§8), each `not_evaluated`.
- **The caller's own permission decision is unexercised against the real harness.** R3 measured that a mutating shell command inside the workspace runs with **no prompt at all** under the owner's `acceptEdits` default: the file it created is the evidence, and no request reached PIO. The decision path — `execution.respond_action` to a single-use `allow` or `deny` — is proven only against the labeled fake. R4 was to be the allow and is not run, because it carries the same brief; [its record](../work/m3/claude-live/R4-not-run.json) says so. Reaching it needs a tool call this harness actually asks about, which is a measurement, not a guess.
- Whether the CLI's `manual` is the flag spelling of the `default` mode `system/init` reports (§4).
- ~~**Obligation for M6:** the guard refuses an absent configured `defaultMode`~~ **Resolved 2026-09-26 (D2).** An absent `defaultMode` is the product's own default, `default` as `init` reports it on 2.1.278 and 2.1.281, taken from the pinned stream identity. The guard compares the requested mode with it like any configured value (`configured_source: product_default`). Requesting it passes **no** `--permission-mode` flag, since the flag does not accept the name, and the `init` echo is compared with it after delivery as for any mode.
- **Obligation for M6:** `--max-budget-usd`, `--task-budget` and `--max-turns` exist on this CLI. M2 recorded product budget enforcement as unproven; M3 does not use them, and whether they give PIO a real enforcement mechanism is unexamined.

## Out of scope

The TUI (M4), CBR composition (M5), OpenCode and Hermes (M3b), and product budget enforcement, which M2 recorded as unproven and M3 does not attempt.
