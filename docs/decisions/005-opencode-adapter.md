# ADR 005 — OpenCode adapter behind the durable host

- Status: **proposed builder design**, 2026-09-20, for independent review on the M3b pull request. Scope: [issue #10](https://github.com/Combraton/pio/issues/10) and the owner's request of 2026-09-20 to run MiniMax through OpenCode.
- Supersedes nothing. It follows the shape of [ADR 003](003-codex-app-server-adapter.md) and [ADR 004](004-claude-code-adapter.md).
- Every fact marked **measured** was observed on this workstation by `scripts/opencode_probe.py` at a cost of **zero model tokens**. Facts that are not measured are named as open questions rather than assumed.
- **Re-pinned to OpenCode 2.0.11 on 2026-09-21, from 2.0.1.** npm had self-updated the owner's install — open question 5 below asked whether it would, and the answer is yes. The adapter refused to qualify, which is the refusal working. Re-measured at zero tokens the same day; the deltas are in §0 and every other measured fact below held.

## 0. What the self-update moved

| | 2.0.1, 2026-09-20 | 2.0.11, 2026-09-21 |
| --- | --- | --- |
| Command-line surface | 7 commands | 7 commands; **only the top-level help digest moved**, the six subcommand helps are byte-identical |
| `initialize` result | as §1 records it | unchanged, but for `agentInfo.version` |
| Models offered to a session | 68 | 127 |
| The seven MiniMax ids | present | **present, unchanged** |
| The session's own current model, as-configured | `juspay-grid/glm-latest` | **`opencode/deepseek-v4.1-flash`** |
| How a client sets the model | never measured | **`session/set_config_option`** with `configId`/`value` (§4) |
| The isolated control's substitute | `opencode/nemotron-3.5-lightning-free` (9 models) | `opencode/jev-1.13-free` (8 models) |
| Private server child | `acp` spawning `serve --stdio --port 0` | unchanged |

**The owner's configuration file did not change.** It still declares `model: juspay-grid/glm-latest` and still has no `permission` key. What changed is that **2.0.11 does not give a new ACP session the configured model**: it reports a free `opencode/` model instead. That makes §3's hazard worse rather than better — the substitution now happens on the owner's own configuration, not only under an isolated one — and it is the reason the equality guard of §4 is a refusal and not a warning.

## Problem

PIO must drive the user's installed OpenCode against a MiniMax model, behind the same durable host and evidence discipline, **without touching the owner's running OpenCode service**, without reading or passing a credential, and without widening a permission.

## Decisions

### 1. Transport: the ACP stdio server

PIO spawns `opencode acp` and speaks Agent Client Protocol over its stdin and stdout — the same newline-delimited JSON shape the durable host already drives for Codex and Claude.

**Measured**, `initialize` answers at zero tokens with `protocolVersion: 1`, `agentInfo: {name: "OpenCode", version: "2.0.11"}`, `authMethods: [{id: "opencode-login"}]`, and `agentCapabilities` carrying `loadSession`, `promptCapabilities`, `mcpCapabilities` and session `close`, `delete`, `fork`, `list`, `resume`.

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

**Measured**, the user's configured default is `juspay-grid/glm-latest` — an OpenAI-compatible gateway at `grid.ai.juspay.net`, declared in `~/.config/opencode/opencode.jsonc`. It is **not** MiniMax. Under 2.0.11 a new ACP session does not even use it: it reports `opencode/deepseek-v4.1-flash` (§0). Neither is MiniMax, and neither is what PIO runs.

Every OpenCode run therefore passes an explicit MiniMax model. **Measured at zero tokens** from `session/new`'s own `configOptions`, the available ids are exactly:

`minimax-coding-plan/MiniMax-M2`, `MiniMax-M2.1`, `MiniMax-M2.5`, `MiniMax-M2.5-highspeed`, `MiniMax-M2.7`, `MiniMax-M2.7-highspeed`, `MiniMax-M3`.

The session also reports `effort` (`default`, `max`, `high`, `none`) and `mode` (`build`, `plan`, currently `build`).

PIO never selects a model outside a dated, owner-authorized exception, as ADR 003 §4 and ADR 004 §4 established. The M3b token is `owner-2026-09-20-m3b-opencode-fixture-runs`, refused on its own and required whenever a model is passed.

**Owner decision, 2026-09-20: no PIO run uses the Juspay Grid provider for any purpose.** The as-configured run is therefore **not evaluated, with that decision as the stated reason** — not an oversight and not a pending measurement. `service_admission` refuses a `juspay-grid/` model outright, so the exclusion is enforced rather than merely documented.

**Owner decision, 2026-09-20: the fixture model is `minimax-coding-plan/MiniMax-M2.7-highspeed`** for every run.

#### Passing a model is not selecting one — measured 2026-09-21, at zero tokens

The first live run refused here, correctly, and the refusal exposed a defect in this adapter. **A new ACP session does not start on the model the client asked for.** `session/new` reports the harness's own default and nothing in its parameters changes that; `opencode acp` has no `--model` flag either. The host had only ever *checked* the model against what the session reported, and never *set* it, so every live run would have refused before delivery for ever.

Measured, by probing the wire at zero tokens:

```
--> session/set_config_option {"sessionId": …, "configId": "model",
                               "value": "minimax-coding-plan/MiniMax-M2.7-highspeed"}
<-- {"configOptions": [{"id": "model", …,
                        "currentValue": "minimax-coding-plan/MiniMax-M2.7-highspeed"}, …]}
```

- The parameters are `configId` and `value`. `optionId`, `valueId` and `session/set_model` are all rejected; the first two produce `-32602` naming the missing field, the last `-32601`.
- The answer carries **the harness's own updated report**, and that report — not the absence of an error — is what the guard is run against. A harness that answers cleanly and changes nothing is refused.
- The selection is **per session**: a new session reverts to the harness's default.

So the order is now `session/new`, select, read the harness's own report back, guard, and only then the brief. The refusal still precedes delivery, which is the property this adapter was chosen for.

**The labeled fake hid this for the whole of M3b's offline work** by echoing the requested model back on `session/new`. A fake that agrees with PIO cannot test PIO. It now starts every session on `opencode/deepseek-v4.1-flash`, as the real harness does, and only a selection moves it — with scenario knobs for a harness that ignores the selection and one that refuses it.

### 5. Permissions

**Measured: the user's configuration has no `permission` key at all**, so there are no configured rules to compare against and the Claude adapter's equality guard has nothing to guard. Until the default behaviour is measured on the wire, the adapter carries `permission_default: not_evaluated`.

- **`--auto` is never passed**, in any form, at top level or on `run`. It auto-approves everything not explicitly denied, and with no deny rules configured that is every request.
- Only **single-use** decisions are forwarded, as for Codex and Claude. A rule update, a session-scoped grant and a mode change are all refused.
- **An option is chosen by its `kind`, never by its id.** ACP lets the agent invent the ids; the meaning is in `kind`. PIO selects the option whose kind is `allow_once` for an allow and `reject_once` for a deny or a default reject, never an `*_always` kind, and when the kind a decision needs is **not offered** it selects nothing, answers `cancelled` and records the refusal — a caller's allow that cannot be expressed single-use is refused rather than approximated with the always option sitting next to it. The ids `allow` and `reject` the host used until 2026-09-21 were the **labeled fake's own invention**; on a harness naming them anything else they would have selected nothing, silently. The fake's ids are now `opt_1`, `opt_2` and `opt_3` precisely so that a hard-coded id fails offline.
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

### 8. A refusal before delivery is a finished execution

Found by the same live run. PIO refused the session and recorded `delivery: failed_before_delivery` with evidence class `native_turn_never_sent` — and then the execution sat at `runtime: preparing` for ever, because only the harness's exit moves the runtime and there had been no turn to exit from. A caller polling the runtime could not tell a refusal from a slow start; the live runner waited until it was stopped by hand.

A host that failed **before releasing the brief** is finished: the brief never left PIO and the child is stopped. The shared projection now says so — `runtime: exited`, with `exit` left `unavailable` because no exit code was observed and none is claimed. This is in the shared projection, so it is true of all three adapters.

## Open, to be measured before any live run

1. **The `session/request_permission` request and response shapes on the wire.** ACP specifies them; they are unverified against 2.0.1, and PIO forwards no decision whose single-use form it has not measured.
2. **Usage reporting granularity**, first, exactly as for Claude. The stops cannot be set before it is known whether usage arrives during a turn or only at its end. The host now **searches** each `session/update` and the turn result for any key named `usage` or ending in `tokens`, and records a census — every update kind seen, which of them carried usage, and where — so R1 can report a place PIO did not expect rather than confirm the one it assumed. The Claude adapter summed two of four usage parts for five live runs because it looked only where it expected.
3. **Cancellation**: `session/cancel` semantics, and whether a cancelled turn still reports usage.
4. Whether `mode: plan` is a genuinely narrower posture worth requesting for fixture runs.
5. ~~The **surface and session identity** to pin, and whether npm self-update moves it, as the Homebrew cask does for Claude.~~ **Answered 2026-09-21: it moves.** npm took the install from 2.0.1 to 2.0.11 between the plan and the first live run; qualification refused, the pin was moved and the surface recaptured (§0). Expect it to move again, and expect a refusal rather than a silent run on a version nobody measured.
6. Whether any ACP option yields a **prompt acknowledgment** (§7). Until one is measured, delivery carries no proof class for this harness.

## Budget and stops (owner decision, 2026-09-20)

MiniMax cap **300,000,000 tokens total**, stop and report at **240,000,000**. Each run carries its own **2,000,000** runner limit. Granularity is measured before the stops are trusted. A handful of live runs is in scope now; repetition-heavy testing waits for incremental commits.

## Out of scope

**Hermes is deferred** (owner decision, 2026-09-21). It is **test scope only and does not gate v0.1**, and no Hermes adapter work happens unless the owner says otherwise; if it is ever picked up it runs only under an isolated profile. The as-configured Juspay Grid run, unless the owner approves it on the issue.
## The permission shape is specified, not measured

The labeled fake's `session/request_permission` — its `toolCall`, its three options and the `allow_always` among them — is taken **from the ACP specification and has never been measured against OpenCode 2.0.1**. The option **ids** are the fake's invention outright; the reviewer's own reading suggests a real dialog offers something like "once", "always" and "reject", which is a hint and not a measurement either. The host records the real option list, ids and kinds apart, from the **first live request**, and that recording is the measurement this ADR owes. Every offline case that exercises a permission decision therefore proves what PIO does with the shape it was told to expect, not what the harness sends.

**The first live request is the measurement.** Until MiniMax R2 produces one, no receipt may be read as evidence that this harness asks at all, or that it asks in this shape. The owner's configuration carries no permission rules, so it is possible that it never asks — which is the state the Claude adapter was measured in, for a different reason, across four live runs.

The fake now asks **only when a client announced itself** with the ACP handshake, and decides for itself otherwise, because a fake that always asks cannot show a harness that does not.

