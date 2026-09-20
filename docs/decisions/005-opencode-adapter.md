# ADR 005 — OpenCode adapter behind the durable host

- Status: **proposed builder design**, 2026-09-20, for independent review on the M3b pull request. Scope: [issue #10](https://github.com/Combraton/pio/issues/10) and the owner's request of 2026-09-20 to run MiniMax through OpenCode.
- Supersedes nothing. It follows the shape of [ADR 003](003-codex-app-server-adapter.md) and [ADR 004](004-claude-code-adapter.md).
- Every fact marked **measured** was observed on this workstation on 2026-09-20 against OpenCode 2.0.1, at a cost of **zero model tokens**, by `scripts/opencode_probe.py`. Facts that are not measured are named as open questions rather than assumed.

## Problem

PIO must drive the user's installed OpenCode against a MiniMax model, behind the same durable host and evidence discipline, **without touching the owner's running OpenCode service**, without reading or passing a credential, and without widening a permission.

## Decisions

### 1. Transport: the ACP stdio server

PIO spawns `opencode acp` and speaks Agent Client Protocol over its stdin and stdout — the same newline-delimited JSON shape the durable host already drives for Codex and Claude.

**Measured**, `initialize` answers at zero tokens with `protocolVersion: 1`, `agentInfo: {name: "OpenCode", version: "2.0.1"}`, `authMethods: [{id: "opencode-login"}]`, and `agentCapabilities` carrying `loadSession`, `promptCapabilities`, `mcpCapabilities` and session `close`, `delete`, `fork`, `list`, `resume`.

The three candidates were weighed against the owner's constraints:

| Candidate | Why not chosen |
| --- | --- |
| `run --format json` | One-shot, and its only non-interactive answer to a permission prompt is `--auto`, which the owner forbids. It also defaults to the background service. |
| `--standalone` private HTTP server | Works, but PIO would implement an HTTP and event-stream client, manage a port and pairing, and still reach the same session API the ACP server wraps. |
| **`opencode acp`** | **Chosen.** |

The deciding reason is **isolation, and it is structural rather than a flag**. Measured process tree while an ACP session is up:

```
11135 11128 .../bin/opencode2 acp
11136 11135 .../bin/opencode.exe serve --stdio --port 0
```

`acp` starts **its own private server as a child**, on no port, and the owner's `serve --service` process is byte-identical in `ps` before and after — pid and start time unchanged. The probe asserts that and fails if it moves. PIO therefore never passes `--server`, never runs `service`, and never needs `--standalone`, because `acp` already is a private instance.

The second reason is that ACP is the only candidate with a **first-class permission request and response**, so PIO can forward single-use decisions without ever passing `--auto`.

### 2. The owner's service is untouched, and that is checked

**The owner runs `opencode serve --service`.** PIO never connects to it, never restarts it and never stops it. Every probe and every run records that process's pid and start time before and after and refuses if they changed. `--server` is never passed, and `service` is never invoked.

### 3. Credentials: never read, never passed — and isolation is weaker here than for Claude

OpenCode holds its credentials in its own store for a provider named `minimax-coding-plan`. **PIO never reads that store, never reads the Keychain and never passes a key.** The environment handed to a child is an allowlist, and a test asserts it carries no variable whose name contains `API_KEY`, `TOKEN`, `SECRET`, `PASSWORD` or `CREDENTIAL`.

**Measured, and it must be said plainly: the environment does not isolate OpenCode's configuration.** With `PATH` alone and **no `HOME`**, a session still found the user's configuration and all 68 models, including the 7 MiniMax ones — the binary resolves the user's home without consulting `$HOME`. Unlike Claude Code, PIO **cannot** prevent an OpenCode child from reading the user's credential store by clearing the environment. What PIO can honestly claim is narrower: it does not read the store itself and supplies no key. The harness authenticates itself, as at the keyboard.

`OPENCODE_CONFIG_DIR` (equivalently `XDG_CONFIG_HOME`) **does** isolate, and is the negative control: an empty directory yields 9 models, **zero** MiniMax, and the configured default disappears.

**A missing route does not refuse — it silently downgrades.** Measured: with an isolated configuration the session still initializes and the default model becomes `opencode/nemotron-3.5-lightning-free`, a built-in free model. An absent provider that quietly runs a fallback model would send fixture content to a third party with nothing in the record saying so.

**Owner decision, 2026-09-20, and it closes that hazard: refuse unless the session's reported provider and model equal the requested ones.** `session_configuration_guard` does exactly that, and refuses a session that reports no configuration or no model rather than assuming one.

**This check genuinely precedes delivery, and the Claude one cannot.** Measured: `session/new` returns `configOptions` carrying `model.currentValue` **before any prompt is sent**, so PIO reads what the session will actually use while the turn is still unstarted. Claude Code's `system/init` never arrives until the brief has already been written (ADR 004 §4), so its equivalent check is corroboration after delivery. The two are **not** equivalent guarantees and the receipts do not describe them as such.

### 4. Model: explicit, under a new dated test-only exception

**Measured**, the user's configured default is `juspay-grid/glm-latest` — an OpenAI-compatible gateway at `grid.ai.juspay.net`, declared in `~/.config/opencode/opencode.jsonc`. It is **not** MiniMax.

Every OpenCode run therefore passes an explicit MiniMax model. **Measured at zero tokens** from `session/new`'s own `configOptions`, the available ids are exactly:

`minimax-coding-plan/MiniMax-M2`, `MiniMax-M2.1`, `MiniMax-M2.5`, `MiniMax-M2.5-highspeed`, `MiniMax-M2.7`, `MiniMax-M2.7-highspeed`, `MiniMax-M3`.

The session also reports `effort` (`default`, `max`, `high`, `none`) and `mode` (`build`, `plan`, currently `build`).

PIO never selects a model outside a dated, owner-authorized exception, as ADR 003 §4 and ADR 004 §4 established. The M3b token is `owner-2026-09-20-m3b-opencode-fixture-runs`, refused on its own and required whenever a model is passed.

**Owner decision, 2026-09-20: no PIO run uses the Juspay Grid provider for any purpose.** The as-configured run is therefore **not evaluated, with that decision as the stated reason** — not an oversight and not a pending measurement. `service_admission` refuses a `juspay-grid/` model outright, so the exclusion is enforced rather than merely documented.

**Owner decision, 2026-09-20: the fixture model is `minimax-coding-plan/MiniMax-M2.7-highspeed`** for every run.

### 5. Permissions

**Measured: the user's configuration has no `permission` key at all**, so there are no configured rules to compare against and the Claude adapter's equality guard has nothing to guard. Until the default behaviour is measured on the wire, the adapter carries `permission_default: not_evaluated`.

- **`--auto` is never passed**, in any form, at top level or on `run`. It auto-approves everything not explicitly denied, and with no deny rules configured that is every request.
- Only **single-use** decisions are forwarded, as for Codex and Claude. A rule update, a session-scoped grant and a mode change are all refused.
- The out-of-fixture classification built for Claude applies unchanged: a path-bearing request resolving outside the fixture is declined by PIO with a recorded reason; anything PIO cannot classify is surfaced to the caller and never auto-allowed.

### 6. Outward behaviour observed

A private instance **watches the user's home directory and `~/.config/opencode`** while it runs. That is recorded as an observed outward surface, the way `messaging_socket_path` is for Claude. `OPENCODE_DISABLE_FILEWATCHER` exists; using it would change how the harness runs and is not used.

### 7. Delivery: this harness acknowledges nothing, and that weakens the proof

**Measured:** an ACP session's messages up to and including `session/new` are `initialize`'s result, a `session/update` carrying `available_commands_update`, and `session/new`'s result. `session/prompt` is a JSON-RPC **request whose response arrives at the end of the turn**, carrying `stopReason`. There is no acknowledgment of the prompt itself.

So OpenCode gives PIO **no delivery acknowledgment**, and this is a real difference from both existing adapters rather than a gap in the measurement:

| Harness | What proves delivery | Proof class |
| --- | --- | --- |
| Codex | the `turn/start` response, carrying a turn id | `provider_ack_id` |
| Claude Code | the replay echo of the exact message sent | `provider_ack_id` |
| **OpenCode** | **nothing the harness sends says "I received it"** | **none** |

The strongest honest statement is that the first `session/update` after a prompt shows the harness acting on it. That is evidence of receipt, but it is **not an identifier the provider returned**, so PIO records delivery as `acknowledged` with evidence class `native_session_update` and **no proof class**, rather than borrowing a proof class it has not earned.

The consequence is stated rather than worked around: on an ambiguous outcome — a host lost after release, say — OpenCode gives less to reconcile with than Codex or Claude Code, so more outcomes stay `ambiguous`. Whether a prompt acknowledgment exists under some other ACP option is an open question below, not an assumption.

## Open, to be measured before any live run

1. **The `session/request_permission` request and response shapes on the wire.** ACP specifies them; they are unverified against 2.0.1, and PIO forwards no decision whose single-use form it has not measured.
2. **Usage reporting granularity**, first, exactly as for Claude. The stops cannot be set before it is known whether usage arrives during a turn or only at its end.
3. **Cancellation**: `session/cancel` semantics, and whether a cancelled turn still reports usage.
4. Whether `mode: plan` is a genuinely narrower posture worth requesting for fixture runs.
5. The **surface and session identity** to pin, and whether npm self-update moves it, as the Homebrew cask does for Claude.
6. Whether any ACP option yields a **prompt acknowledgment** (§7). Until one is measured, delivery carries no proof class for this harness.

## Budget and stops (owner decision, 2026-09-20)

MiniMax cap **300,000,000 tokens total**, stop and report at **240,000,000**. Each run carries its own **2,000,000** runner limit. Granularity is measured before the stops are trusted. A handful of live runs is in scope now; repetition-heavy testing waits for incremental commits.

## Out of scope

Hermes, which comes after OpenCode and only under an isolated profile — and which is deferred with that said plainly if isolation turns out not to be possible. The as-configured Juspay Grid run, unless the owner approves it on the issue.
