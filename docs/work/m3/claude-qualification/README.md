# Claude Code qualification and offline probe evidence

Evidence for [issue #7](https://github.com/Combraton/pio/issues/7), recorded 2026-09-20 on the macOS arm64 workstation. **No model call was made and no credential was read: `model_calls` is 0 and the user's configuration is byte-identical before and after.** Home-relative paths are shown as `~` and scratch paths as `<scratch>`.

Reproduce with:

```sh
python3 scripts/claude_offline_probe.py --executable /opt/homebrew/bin/claude --out FRESH_DIR
```

## What it establishes

| Fact | Observation |
| --- | --- |
| Executable | `/opt/homebrew/bin/claude` is a symlink into `/opt/homebrew/Caskroom/claude-code@latest/2.1.278/claude`. **The resolved path carries the version**, so a self-update moves it. Mach-O arm64, sha256 `bd245662…` |
| Version | `2.1.278 (Claude Code)` — the M0 readiness record's 2.1.273 is stale |
| CLI surface identity | Canonical digest over `--help` and seven subcommand helps: `0dd17ec7…`. **Byte-identical across three runs** and 50 ms to compute, so it is a cheap fingerprint of the interface PIO drives |
| Configured credential route | `loggedIn: true`, `authMethod: "claude.ai"`, `apiProvider: "firstParty"`, `subscriptionType: "max"` |
| Account identity dropped | `auth status` also prints `email`, `orgId` and `orgName`. The probe **drops them at the boundary**; they appear in no evidence file |
| Missing-route control | With an isolated `CLAUDE_CONFIG_DIR` and no credential variables: `loggedIn: false`, `authMethod: "none"` |
| User configuration untouched | `settings.json` and `~/.claude.json` digests identical before and after the whole probe |

## The stream shapes, measured without spending anything

Launching in the isolated config fails authentication **before any model call**, so the message shapes cost nothing. Sequence:

```
system/init → user (isReplay) → assistant (authentication_failed) → result/success (is_error: true)
```

- **`system/init`** carries `apiKeySource`, `permissionMode`, `model`, `claude_code_version`, `session_id`, `capabilities`, and the name lists M3 receipts must record: `tools`, `mcp_servers`, `plugins`, `slash_commands`, `skills`, `agents`. In the isolated config these were 22 tools, 0 MCP servers, 0 plugins, 46 slash commands, 17 skills, 5 agents; an as-configured run will show the user's eleven plugins instead of zero.
- **`user` with `isReplay: true`** echoes back the exact message PIO sent — verified equal to what was sent. This is the delivery acknowledgment, the analogue of Codex's `turn/start` response.
- **`result`** carries `usage`, `modelUsage`, `total_cost_usd`, `permission_denials`, `num_turns`, `is_error` and `terminal_reason`.
- The authentication failure is an `assistant` message with `error: "authentication_failed"` and `is_api_error_message: true`, then a `result` with `terminal_reason: "api_error"` and **zero usage**.

## Findings that shape the adapter

- **`capabilities` advertises `interrupt_receipt_v1` and `interrupt_cancel_queued_v1`**, while the published documentation describes only SIGINT and SIGTERM for stopping a turn. An in-band interrupt probably exists. Until it is measured, [ADR 004](../../../decisions/004-claude-code-adapter.md) uses SIGINT and says so.
- **Usage is reported at turn end in `result`.** Whether `assistant` messages carry usable incremental usage during a real turn is not measured here, because no real turn ran. If they do not, a mid-turn token stop is impossible in this transport and a limit can only prevent the next turn. This is the granularity measurement the owner asked for before the stops are set, and it is the first item in the live-run plan.
- **The permission-mode breadth ordering is not established.** A documentation pass produced an ordering that contradicted itself, placing `dontAsk` — which auto-denies everything that would prompt — as broader than `acceptEdits`. The guard therefore requires equality with the configured default rather than encoding a guess.

## Qualification against the real executable

| File | Command | Result |
| --- | --- | --- |
| [qualification.json](qualification.json) | `target/debug/pio claude qualify --executable /opt/homebrew/bin/claude --work <scratch>` | **qualified**, exit 0. Version 2.1.278, resolved into the cask directory named `2.1.278`, binary sha256 `bd245662…`, surface listing `0dd17ec7…` equal to [the checked-in identity](../../../../adapters/claude/2.1.278/surface-identity.json), zero drift |
| [wrong-executable-control.json](wrong-executable-control.json) | same, with `--executable ~/.local/bin/codex` | refused `unsupported_version`, exit 3, surface generation `skipped`, so no Claude-specific argument reached the other harness |

None of this is a live run, a real journey or a model-backed result.
