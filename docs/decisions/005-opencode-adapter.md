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

**Owner decision, 2026-09-20: the fixture model is `minimax-coding-plan/MiniMax-M2.7-highspeed`** for every run. **Widened 2026-09-21: the exception covers the provider `minimax-coding-plan`, not one model id**, so a run may use whichever model on the owner's plan suits the evidence it is for.

It is an allowlist of exactly one provider, not a relaxation. Admission refuses everything else by name, and the record says which rule: `provider_excluded_by_the_owner` for Juspay Grid, `provider_not_covered_by_the_exception` for anything else — including `opencode/` free models, which is precisely what a silently downgraded session reports, so the admission refuses what the session guard would. The seven ids were confirmed from the session's own option list at zero tokens and recorded in [`opencode-model-ids.json`](../work/m3b/opencode-model-ids.json).

**The freedom is used for evidence, not volume.** R1 runs on the fastest model, because its question is *how much*; the two decision runs use the strongest, because their question is whether a clean single tool call happens at all; the cancel run uses a middle model, because what it costs is time rather than judgement. Three models across five runs, and the packet reports cost and behaviour per model. Every receipt records **configured, requested and reported** model, plus what the session started on before PIO selected anything.

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

**Measured: the user's configuration has no `permission` key at all**, so there are no configured rules to compare against and the Claude adapter's equality guard has nothing to guard.

**Measured across R2, R3 and R5: this harness asks about some operations and not others.** That is narrower and more useful than either "it always asks" or "it never does", and the first two runs alone would have supported the wrong one of those.

| Run | Posture | Operation | Did it ask? |
| --- | --- | --- | --- |
| R2 | `build` | shell command **inside** the workspace | **no** — it ran, and the marker file was there afterwards |
| R3 | `plan` | the same shell command | **no** — the narrower posture changed nothing |
| R5 | `plan` | file read **outside** the workspace | **yes** — and PIO declined it |

The honest statement has two halves.

**For an operation it does not ask about, PIO observes; it does not contain.** R2 and R3 ran a shell command with no request reaching PIO at all — `decided_by: null`, `outcome: performed`. The effect stayed inside the workspace because the command was an inside-the-workspace one, not because PIO stopped anything. The narrower posture makes no difference to this.

**For an operation it does ask about, the whole path works.** R5's out-of-fixture read produced a real `session/request_permission`; PIO classified it `outside_fixture`, declined it by selecting the option whose kind is `reject_once`, and the marker's own content never appeared in anything the harness sent.

An earlier draft of this section, written after R2 and R3 and before R5, said `permission_default` was "measured, it acts without asking". That was too broad on two runs, and R5 falsified it. What is measured is the table above.

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
| Claude Code | the replay echo of the exact message sent | none (corrected 2026-09-26, D7: the echo returns no identifier; evidence class `native_replay_echo`) |
| **OpenCode** | **nothing the harness sends says "I received it"** | **none** |

The strongest honest statement is that the first `session/update` after a prompt shows the harness acting on it. That is evidence of receipt, but it is **not an identifier the provider returned**, so PIO records delivery as `acknowledged` with evidence class `native_session_update` and **no proof class**, rather than borrowing a proof class it has not earned.

The consequence is stated rather than worked around: on an ambiguous outcome — a host lost after release, say — OpenCode gives less to reconcile with than Codex or Claude Code, so more outcomes stay `ambiguous`. Whether a prompt acknowledgment exists under some other ACP option is an open question below, not an assumption.

### 8. A refusal before delivery is a finished execution

Found by the same live run. PIO refused the session and recorded `delivery: failed_before_delivery` with evidence class `native_turn_never_sent` — and then the execution sat at `runtime: preparing` for ever, because only the harness's exit moves the runtime and there had been no turn to exit from. A caller polling the runtime could not tell a refusal from a slow start; the live runner waited until it was stopped by hand.

A host that failed **before releasing the brief** is finished: the brief never left PIO and the child is stopped. The shared projection now says so — `runtime: exited`, with `exit` left `unavailable` because no exit code was observed and none is claimed. This is in the shared projection, so it is true of all three adapters.

### 9. Where OpenCode reports usage — measured by R1, not assumed

The first completed live turn answered the question this adapter's stops were waiting on, and found the host reading the wrong place while doing it.

**During the turn**, a `session/update` of kind **`usage_update`**:

```json
{"sessionUpdate": "usage_update", "used": 7910, "size": 204800,
 "cost": {"amount": 0, "currency": "USD"}}
```

`used` is a running total, `size` the context window, `cost` a money amount that is zero on a subscription plan. **None of those field names contains `usage` or ends in `tokens`.** A census searching field names alone would have missed the one update kind that carries usage; it was found by the update's own kind, and the whole update is recorded. Exactly one arrived on this short turn. Whether more arrive on a long one is R4's question.

**At the end of the turn**, the `session/prompt` result carries `usage` — **at the top level, not under `_meta`** — with `inputTokens`, `outputTokens`, **`thoughtTokens`** and its own `totalTokens`.

The host was reading `result._meta.usage` and summing input and output only. So a turn that really cost **7,910 tokens** produced no usage event at all, the receipt recorded `unknown`, and the stop rule fired — correctly, because unknown is never zero, but on a turn whose cost the harness had reported plainly. The two-part measure would also have dropped 38 `thoughtTokens` had the path been right.

Both are now searched for rather than looked up: the usage object is found wherever it is, and **every** counter the harness reports is summed. `totalTokens` is the harness's own total and is kept beside PIO's sum rather than added to it, so a disagreement between them is something PIO reports and not something it resolves. The 7,910 tokens are charged to the ledger under `R1-attempt-2`.

### 10. Cancel: in band, and ineffective — measured by R4

The reason this transport was described as cancelling *in band*, unlike the Claude adapter's signal, is that ACP has a `session/cancel` request. R4 measured what it does.

**Attempt 1** cancelled eight seconds into a count to 400. The turn produced every one of the 400 lines and finished with `stop_reason: end_turn`. That says nothing either way: a cancel that did nothing and one that worked look identical in the stop reason, which is why the receipt now reads the streamed work back from the run's own spool.

**Attempt 2** cancelled four seconds into a count to 2000, with the turn plainly mid-work.

- The cancel was sent in band to an active turn and was accepted at the transport.
- **The turn did not stop.** After the ten-second escalation window PIO killed the child. 656 of 2000 lines had been streamed.
- No result ever arrived: `stop_reason` null, `turn_completed: false`.
- **No usage of any kind was reported** — not even the `usage_update` that arrives at the end of a turn that finishes. `report_count: 0`.

Two things follow, and both are narrower claims than this adapter began with.

**PIO's only effective interrupt for this harness is killing the process.** The in-band cancel is sent, is accepted, and changes nothing. The escalation is not a fallback for an unusual case; on this evidence it is the mechanism.

**A cancelled turn costs an unknown amount, and unknown is never zero.** Owner decision of 2026-09-21: the Claude allowance rule applies with a figure of this sequence's own. It is **12,000 tokens**, built from this sequence's own completed turns — every one reported 7,866 to 7,874 input tokens in this fixture, and attempt 1 produced 400 lines for 800 output and 854 thought tokens, so 656 lines works out at about 10,040 — rounded up by the same margin the Claude rule took when it rounded 33,793 and 32,957 up to 40,000. The ledger carries **`observed` and `charged`**, and **every stop rule uses `charged`**.

A cancel run's missing usage report is now the expected outcome rather than a halt. **An unplanned one still halts the sequence.**

## Open, to be measured before any live run

1. **The `session/request_permission` request and response shapes on the wire.** ACP specifies them; they are unverified against 2.0.1, and PIO forwards no decision whose single-use form it has not measured.
2. ~~**Usage reporting granularity**, first, exactly as for Claude.~~ **Answered by R1, §9.** Usage arrives **both** during the turn, as a `usage_update` session update carrying a running `used` total, and at the end, in the `session/prompt` result. The census that answered it is kept, because it is what found the host reading the wrong place, and because R4 still has to say whether a longer turn reports more than once.
3. ~~**Cancellation**: `session/cancel` semantics, and whether a cancelled turn still reports usage.~~ **Answered by R4, §10.** The in-band cancel did not stop the turn, and a turn PIO had to kill reports no usage at all.
4. Whether `mode: plan` is a genuinely narrower posture worth requesting for fixture runs. **Implemented 2026-09-21 after R2**, because it is the only per-session lever that does not edit the owner's configuration. PIO selects it with `session/set_config_option` and **refuses unless the session reports the mode back**, exactly as for the model. Only narrowing values are requestable: `build` is the harness's own default, so asking for it could only ever loosen a session that was already narrower, and admission refuses it as `mode_is_not_a_narrowing_one`. Whether it actually changes what prompts is R3's question.
5. ~~The **surface and session identity** to pin, and whether npm self-update moves it, as the Homebrew cask does for Claude.~~ **Answered 2026-09-21: it moves.** npm took the install from 2.0.1 to 2.0.11 between the plan and the first live run; qualification refused, the pin was moved and the surface recaptured (§0). Expect it to move again, and expect a refusal rather than a silent run on a version nobody measured.
6. Whether any ACP option yields a **prompt acknowledgment** (§7). Until one is measured, delivery carries no proof class for this harness.

## Budget and stops (owner decision, 2026-09-20)

MiniMax cap **300,000,000 tokens total**, stop and report at **240,000,000**. Each run carries its own **2,000,000** runner limit. Granularity is measured before the stops are trusted. A handful of live runs is in scope now; repetition-heavy testing waits for incremental commits.

## Out of scope

**Hermes is deferred** (owner decision, 2026-09-21). It is **test scope only and does not gate v0.1**, and no Hermes adapter work happens unless the owner says otherwise; if it is ever picked up it runs only under an isolated profile. The as-configured Juspay Grid run, unless the owner approves it on the issue.
## The permission shape, now measured

**R5 produced the first real `session/request_permission` from OpenCode**, and it settles what this section used to hold open. The options, exactly as the harness offered them:

```json
[{"optionId": "once",   "name": "Allow once",   "kind": "allow_once"},
 {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
 {"optionId": "reject", "name": "Reject",       "kind": "reject_once"}]
```

Three things follow.

**The reviewer's reading was right and the fake's invention was wrong.** The hint was that a real dialog offers "once", "always" and "reject"; it does. The fake's ids were `allow`, `allow_always` and `reject`, so **`allow` was never an id this harness uses.** A host hard-coding it would have selected *nothing* on a caller's allow and silently refused — while a hard-coded `reject` would have worked by coincidence, which is the worst way for a defect to hide. Choosing by `kind` is not a stylistic preference; it is the difference between working and failing silently on the first allow anyone tried.

**The always-allow option is real and is offered on every request.** PIO never selects it, and `always_option_taken` is computed from the option actually chosen rather than asserted.

**The fake keeps its different ids on purpose.** Now that the real ones are known, `opt_1`, `opt_2` and `opt_3` are more useful than before: a host that hard-codes an id — the real one or the old one — fails offline rather than in front of the owner's harness.

What the offline cases still cannot show is *when* this harness asks. That is measured in section 5, and it is not the same for every operation.

The fake asks **only when a client announced itself** with the ACP handshake, and decides for itself otherwise, because a fake that always asks cannot show a harness that does not.

