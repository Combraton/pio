# Claude Code qualification, re-qualification and the zero-token probe

## Re-pin to 2.1.281 (2026-09-26)

The cask self-updated and the qualified 2.1.278 binary is gone, so main refused the installed 2.1.281 (`unsupported_version`, D12). PIO is now pinned to **2.1.281**, with its identities in [`adapters/claude/2.1.281/`](../../../../adapters/claude/2.1.281/). The 2.1.278 identities and the records below the line stay as they were.

**How it was measured: zero tokens, fully isolated.** `scripts/claude_requalify.py` gained `--isolated`, and all three runs used it: **no child saw the owner's configuration**. The version, the helps and `auth status` ran with a scratch `HOME` and `CLAUDE_CONFIG_DIR`, the same environment `pio claude qualify` gives them, and every stream launch failed authentication before any model call. So the owner's credential route was **not** observed this time (`configured_route.skipped`) and the owner's files were not snapshotted (`user_configuration_unchanged: "not_observed"`), rather than reporting a comparison that was never made.

```sh
python3 scripts/claude_requalify.py --isolated --executable /opt/homebrew/bin/claude --out FRESH_DIR [--adapters DIR] [--update-baseline]
```

| File | Run | Result |
| --- | --- | --- |
| [drift-2.1.278-to-2.1.281.json](drift-2.1.278-to-2.1.281.json) | against a copy of the 2.1.278 identities | exit 3, the four findings below |
| (baseline run, not committed) | `--update-baseline` into scratch | wrote the two identities now in `adapters/claude/2.1.281/` |
| [requalification-2.1.281.json](requalification-2.1.281.json) | against the committed 2.1.281 identities | **qualified**, exit 0, `findings: []`, `model_calls: 0` |
| [wrong-executable-control-2.1.281.json](wrong-executable-control-2.1.281.json) | `target/debug/pio claude qualify --executable ~/.local/bin/codex --work <scratch>`, built from base `f051063` plus the re-pin | refused `unsupported_version` against `pinned: 2.1.281`, exit 3, surface `skipped`, so no Claude-specific argument reached the other harness |

The three runs agree: identical binary digest, surface listing and stream identity.

**Executable.** `/opt/homebrew/bin/claude` resolves into the cask directory `2.1.281`; `2.1.281 (Claude Code)`; Mach-O sha256 `a922981f6f3b55a2…`. Surface listing `3f6807e14e3840dc…`, top help byte-identical over three runs.

**Drift from 2.1.278, measured:**

| Field | 2.1.278 | 2.1.281 |
| --- | --- | --- |
| surface | listing `0dd17ec7…` | listing `3f6807e1…`: **only the top-level `--help` changed** (`ae85d661…` → `06f5c456…`); the seven subcommand helps are byte-identical |
| `init_keys` | 24 keys | 26: **added `per_turn_effort_active` and `view_mode`**, none removed |
| `capabilities` | `interrupt_receipt_v1`, `interrupt_cancel_queued_v1`, `msg_lifecycle_v1` | the same three, then **`mcp_read_resource_v1`, `mcp_tool_ui_meta_v1`** |
| `product_default_model` | `claude-opus-5[1m]` | **`claude-opus-5-5[1m]`** |

Unchanged: `message_sequence` (`system/init → user → assistant → result/success`), the 21 `result_keys`, `product_default_permission_mode: default`, `init_waits_for_stdin: true` (nothing emitted in 20 s before a write), and the attachment (`--permission-prompt-tool stdio` accepted; the handshake answered `success` with the same five keys). The requested `--permission-mode acceptEdits` is echoed as `acceptEdits`, and the replay echo still equals what was sent.

**Two observations beyond the identity.** The isolated, empty-configuration run now reports **`plugins: 2`** (2.1.278: 0) and 47 slash commands (46). ADR 004 §8's discriminator — an empty configuration shows no plugins, the owner's shows eleven — therefore needs the plugin **names**, not an empty list, from 2.1.281 on. The help text itself is not recorded (only digests), and 2.1.278's binary is gone, so *what* changed in the top-level help is not known.

**Not measured at zero tokens, and still not:** anything about a real turn — per-message usage, the in-band interrupt, the permission wire shapes, the owner's route on 2.1.281. `pio claude qualify` was **not** run against the real 2.1.281: by rule only the script ran the real binary. Both digest the same eight helps with an isolated `HOME` and `CLAUDE_CONFIG_DIR`, and on 2.1.278 the two agreed byte for byte, but that agreement is **not re-measured** for 2.1.281. The unit test `pin_names_the_identity_directory_and_the_requalification_record` ties the pin, both identities and this record together.

---

The rest of this file is the **2.1.278** record of 2026-09-20.

Evidence for [issue #7](https://github.com/Combraton/pio/issues/7), recorded 2026-09-20 on the macOS arm64 workstation. **No model call was made and no credential was read: `model_calls` is 0 and the user's configuration is byte-identical before and after.** Home-relative paths are shown as `~` and scratch paths as `<scratch>`.

Re-qualification is **one command**, because the cask tracks latest and self-updates:

```sh
python3 scripts/claude_requalify.py --executable /opt/homebrew/bin/claude --out FRESH_DIR
```

It exits 0 clean, 3 on drift, 4 if the user's configuration moved under it, and `--update-baseline` regenerates the pinned identities. Every child process runs with a **cleared** environment carrying only `PATH`, `HOME` and `CLAUDE_CONFIG_DIR`.

## What it establishes

| Fact | Observation |
| --- | --- |
| Executable | `/opt/homebrew/bin/claude` is a symlink into `/opt/homebrew/Caskroom/claude-code@latest/2.1.278/claude`. **The resolved path carries the version**, so a self-update moves it. Mach-O arm64, sha256 `bd245662…` |
| Version | `2.1.278 (Claude Code)`, parsed to its first token — the M0 readiness record's 2.1.273 is stale |
| CLI surface identity | Canonical digest over `--help` and seven subcommand helps: `0dd17ec7…`. **Byte-identical across three runs** and 50 ms to compute, so it is a cheap fingerprint of the interface PIO drives |
| Stream identity | The `system/init` key set (24 keys), the capability list, the message sequence and the `result` key set (21 keys), pinned in [stream-identity.json](../../../../adapters/claude/2.1.278/stream-identity.json). **A help digest cannot see the wire**, so both halves are compared |
| Configured credential route | `loggedIn: true`, `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType: "max"` |
| Account identity dropped | `auth status` also prints `email`, `orgId` and `orgName`. They are **dropped at the boundary**; they appear in no evidence file |
| Missing-route control | With an isolated `CLAUDE_CONFIG_DIR` and a cleared environment: `loggedIn: false`, `authMethod: "none"` |
| User configuration untouched | `~/.claude/settings.json`, `~/.claude/settings.local.json` and `~/.claude.json` digests identical before and after |

## The stream shapes, measured without spending anything

Launching in an isolated config fails authentication **before any model call**, so the message shapes cost nothing. Sequence:

```
system/init → user (isReplay) → assistant (authentication_failed) → result/success (is_error: true)
```

- **`system/init`** carries `apiKeySource`, `permissionMode`, `model`, `claude_code_version`, `session_id`, `cwd`, `memory_paths`, `capabilities`, and the name lists M3 receipts must record: `tools`, `mcp_servers`, `plugins`, `slash_commands`, `skills`, `agents`. In the isolated config these were 22 tools, 0 MCP servers, 0 plugins, 46 slash commands, 17 skills, 5 agents; an as-configured run will show the user's eleven plugins instead of zero.
- **`user` with `isReplay: true`** echoes back the exact message PIO sent — verified equal to what was sent. This is the delivery acknowledgment, the analogue of Codex's `turn/start` response.
- **`result`** carries `usage`, `modelUsage`, `total_cost_usd`, `permission_denials`, `num_turns`, `is_error`, `terminal_reason`, and also `stop_reason`, `queued_turn_count` and `subagent_stats`.
- The authentication failure is an `assistant` message with `error: "authentication_failed"` and `is_api_error_message: true`, then a `result` with `terminal_reason: "api_error"` and **zero usage**. Note the `result` subtype is still `success`: a failed turn is not signalled by the subtype.

## Three measurements that changed the design

**`system/init` does not arrive until something is written to stdin.** Held open for 20 s with nothing written, the child emitted **nothing at all**. After the write, `init` arrived in 0.21 s. So the effective permission mode **cannot be checked in-band before the brief is released**: the mode is controlled in the argument vector before spawn, and the `init` echo is corroboration after delivery. [ADR 004](../../../decisions/004-claude-code-adapter.md) §4 says so rather than claiming a pre-flight guarantee.

**The product's own default model is `claude-opus-5[1m]`.** Measured with an empty configuration directory, a cleared environment and no flags. It happens to equal the owner's configured `opus[1m]`, so **a receipt showing Opus proves nothing about whether the user's settings were honoured**. The discriminator is the name lists: the same run reported `plugins: []` and `mcp_servers: []` against eleven plugins as configured.

**The product's own default permission mode reports as `default`** — a name `--permission-mode` does not accept, since the flag takes `manual` instead. The guard's refusal of an absent configured default is therefore not merely cautious: PIO cannot today request "whatever the user would get". Recorded as an obligation for M6.

## Findings that shape the adapter

- **`capabilities` advertises `interrupt_receipt_v1` and `interrupt_cancel_queued_v1`.** The pinned SDK source shows the in-band interrupt control request these correspond to. Until it is measured against 2.1.278, ADR 004 uses SIGINT and records that a signal-cancelled turn in `--print` mode may end before `result` and therefore leave **usage unknown**.
- **Usage is reported at turn end in `result`.** Whether `assistant` messages carry usable incremental usage during a real turn is not measured here, because no real turn ran. If they do not, a mid-turn token stop is impossible in this transport and a limit can only prevent the next turn. This is the granularity measurement the owner asked for before the stops are set, and it is the first item in the live-run plan. `--include-partial-messages` and the empty `usage.iterations` array are the candidates.
- **The permission-mode breadth ordering is partially documented and still not measured.** The pinned SDK documents five of six modes and confirms `dontAsk` **denies** what is not pre-approved — which is why the earlier documentation pass, that had placed `dontAsk` above `acceptEdits`, was unusable. `auto` is listed in the type and described nowhere, and the CLI and SDK disagree on the standard mode's name. The guard therefore stays equality-only.
- **Containment is the permission rules only.** The user's settings carry no `sandbox` key, and the pinned SDK documents `sandbox.enabled` as defaulting to `False`. With 38 `Bash(...)` allow rules across two settings files and no deny rules, **a pre-approved command never prompts and so never reaches PIO**. ADR 004 §5 states this, and every receipt carries `containment.os_sandbox_observed: false`.

## Qualification against the real executable

| File | Command | Result |
| --- | --- | --- |
| [qualification.json](qualification.json) | `target/debug/pio claude qualify --executable /opt/homebrew/bin/claude --work <scratch>` | **qualified**, exit 0. Version 2.1.278, resolved into the cask directory named `2.1.278`, binary sha256 `bd245662…`, surface listing `0dd17ec7…` equal to [the checked-in identity](../../../../adapters/claude/2.1.278/surface-identity.json), zero drift |
| [wrong-executable-control.json](wrong-executable-control.json) | same, with `--executable ~/.local/bin/codex` | refused `unsupported_version`, exit 3, surface generation `skipped`, so no Claude-specific argument reached the other harness |
| [requalification-summary.json](requalification-summary.json) | `python3 scripts/claude_requalify.py --out <scratch>` | **qualified**, exit 0, `findings: []` against both pinned identities |

## Disclosure: the adapter refused a route that works

`pio claude auth-route` as committed in `85f83c1` reported the owner as **not logged in** and exited 3. Two causes compounded, and both are environment, not credentials:

- it passed no `USER`. Measured: given the user's real `HOME` but no `USER`, `auth status` reports `loggedIn: false`, `authMethod: "none"`; adding `USER` alone restores `loggedIn: true`, `claude.ai`, `max`. `SECURITYSESSIONID`, `TMPDIR` and `__CF_USER_TEXT_ENCODING` were each tried and make no difference;
- it set `CLAUDE_CONFIG_DIR` to `~/.claude`, and the product expects `.claude.json` **inside** the configured directory, while as the user has it that file sits at `~/.claude.json` beside `~/.claude/`.

The failure was fail-safe — it refused rather than proceeding — but it would have blocked every live run while looking exactly like the missing-route feature working. `ChildEnv::as_configured` and `ChildEnv::isolated` now separate the two cases, and a fake that reads `USER` fails the suite if the variable stops being passed.

It also repairs the negative control, which must differ from the real route in **exactly one** thing or a refusal could be an artefact of the environment. Measured with `USER` present in both: isolating `HOME`, or isolating `CLAUDE_CONFIG_DIR`, each removes the route on its own.

## Disclosure: a defect in the first probe

The probe committed in `b5e0f6b` **filtered** the environment — dropping variables whose names contained `ANTHROPIC`, `API_KEY` or `TOKEN` — instead of clearing it, so the `CLAUDE_CODE_*` variables exported by the enclosing Claude Code session reached the child. The Rust adapter always cleared the environment; the script did not match it.

It is controlled and fixed. A cleared-environment run and an inherited-environment run return identical `permissionMode`, `model`, `capabilities` and name-list lengths, so **no measurement in `b5e0f6b` changed**. The same disclosure answers where that probe's `acceptEdits` and Opus came from: the mode was a flag the probe itself passed, and the model is the product default above.

None of this is a live run, a real journey or a model-backed result.
