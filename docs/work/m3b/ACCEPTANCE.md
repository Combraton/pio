# M3b acceptance packet — OpenCode adapter, MiniMax

- Scope: [issue #10](https://github.com/Combraton/pio/issues/10), the owner's request of 2026-09-20 and the decisions of 2026-09-21. Design: [ADR 005](../../decisions/005-opencode-adapter.md).
- **Test scope only.** OpenCode does not gate the v0.1 release; Codex and Claude Code do. Test scope is not release scope, and nothing here changes that.
- Every claim below points at a live receipt. Offline cases are named as offline.

## What was actually run

Seven live turns against the owner's installed OpenCode **2.0.11**, all on the owner's MiniMax plan, all from a clean committed head with CI green, each rehearsed against the labeled fake first.

| Run | Model | Charged | Observed | What it settles |
| --- | --- | ---: | ---: | --- |
| [R1-attempt-1](opencode-live/R1-attempt-1.json) | M2.7-highspeed | 0 | 0 | refused before delivery; **no model call** |
| [R1-attempt-2](opencode-live/R1-attempt-2.json) | M2.7-highspeed | 7,910 | 7,910 | a turn completed and PIO recorded its cost as unknown |
| [R1](opencode-live/R1.json) | M2.5-highspeed | 7,894 | 7,894 | usage granularity |
| [R2](opencode-live/R2.json) | M3 | 8,208 | 8,208 | a shell command run with no request reaching PIO |
| [R3](opencode-live/R3.json) | M3 | 8,288 | 8,288 | the same under the narrower posture |
| [R4-attempt-1](opencode-live/R4-attempt-1.json) | M2.7-highspeed | 9,528 | 9,528 | a cancel that reached a live turn and did not stop it |
| [R4](opencode-live/R4.json) | M2.7-highspeed | **12,000** | unknown | the cancel measured against a turn that was cut short |
| [R5](opencode-live/R5.json) | M3 | 8,392 | 8,392 | PIO's own decline, and the real option list |
| **Total** | | **62,220** | **50,220** | 0.02 per cent of the 300,000,000 cap |

**Correction, 2026-09-23: 86,863 charged.** OpenCode's turn usage is its **last model step's**. R2, R3 and R5 each made a tool call and so took two steps, and only the second was recorded. Their first steps were read back, read-only, from the owner's OpenCode store for these PIO-started sessions: 8,135, 8,238 and 8,270, or 24,643 together. They are charged as separate ledger lines. The figures above are the receipts as recorded. See the [M4 map](../m4/SCREEN-INTERFACE-MAP.md), L1, review 44.

Three models, as the owner asked: the fastest where the question was *how much*, the strongest where the question was whether a clean tool call happens at all, a middle one where the cost was time rather than judgement. **Per model:** M2.5-highspeed one run at 7,894; M3 three runs averaging 8,296; M2.7-highspeed three runs, 17,438 observed across two of them and one charged at the allowance. The three models differ by less than 6 per cent per turn on these briefs, because the input is dominated by a system prompt of roughly 7,870 tokens that every run pays.

## Journeys

None is fully accepted. Two are **partial acceptances** in the [shared verification model](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/architecture/VERIFICATION.md)'s sense, naming what is proven live and what remains. `not_evaluated` never counts as `pass`.

### J1 — install and do useful work: partial acceptance

Proven live (`pass`):

- **The installation is bound before any work.** OpenCode 2.0.11, surface identity matched with zero drift, `qualified: true`, **0 model calls**. [qualification](opencode-qualification/qualification.json)
- **Negative control, live:** the same qualifier refuses a Claude Code binary as `version_unavailable`, before running a single OpenCode-specific argument. [wrong-executable](opencode-qualification/wrong-executable-control.json)
- **The pin is a check that fires.** npm self-updated the install from 2.0.1 to 2.0.11 on the morning of the first live run, and qualification refused it as `unsupported_version` **before a token was spent**. Re-measured at zero tokens and re-pinned. [probe](opencode-probe-summary.json)
- **A turn runs on the requested model and completes.** R1: delivery acknowledged with evidence `native_session_update`, source `opencode-acp/host`, exit 0.
- **Work with tools, audited by its real target.** R2, R3 and R5 each used tools; under the corrected audit their targets are `<fixture>` (twice) and `<fixture>/OUTSIDE_TARGET.txt`, with the out-of-fixture read declined.

Remaining (`not_evaluated`): a fresh package install (M6); the `execution.discovery` query, which **no live OpenCode run exercised**; Linux, where only the offline matrices run; the TUI (M4). And one that is specific to this harness: **there is no fidelity discriminator at all.** ACP's `session/new` returns no lists of plugins, tools, commands or agents, so nothing in an OpenCode receipt can stand where the Claude receipt's "12 plugins, 188 tools, 140 slash commands" stands. The receipts say so rather than substituting the model for it.

### J4 — steer or cancel truthfully: partial acceptance, and the acceptance is a negative result

Proven live (`pass`):

- **A cancel is sent in band to a turn that had not finished**, and the receipt records the runtime at that moment so a cancel sent to a finished turn cannot read as a cancel.
- **The cancel does not stop the turn.** R4 cancelled four seconds into a count to 2000. The turn kept going; after the ten-second escalation window PIO killed the child, with 656 of 2000 lines streamed and no result ever returned. **PIO's only effective interrupt for this harness is killing the process.** ADR 005 had described this transport as cancelling in band "unlike the Claude adapter's signal"; it does send in band, and the practical difference is smaller than that implied.
- **A killed turn reports no usage at all** — not even the `usage_update` that arrives at the end of a turn that finishes. Recorded as unknown, never zero, and charged an allowance of 12,000 whose basis is in the receipt and in [ADR 005 §10](../../decisions/005-opencode-adapter.md).
- **The first attempt is kept**, because it shows the measurement being got wrong: a cancel at eight seconds into a count to 400 produced all 400 lines and `stop_reason: end_turn`, which looks identical to a cancel that worked.

**Steering is not claimed.** This host has no steer control, as for Claude Code.

Remaining (`not_evaluated`): the suppressed-acknowledgment control; the refused and unsupported cancel paths; whether a shorter turn or a different model is cancellable at all, which one run cannot say; TUI (M4).

### J3 — recover without duplicate work: `not_evaluated`

Restart, reattach and host-loss have **no coverage for this harness**, live or offline. The Claude plan had a run for it and this one does not. The shared lifecycle is the same code, and that is a reason to expect it to hold, not evidence that it does.

### J5 — independent core: `not_evaluated`

The runs used no Context client and no CBR, and the shared service path is the one M2 and M3 accepted this journey on. **But no OpenCode run recorded a service inventory**, and the journey asks for one. The property is likely to hold and is not claimed.

### J2 and J6: `not_evaluated`

J2 needs the TUI (M4); J6 needs a real CBR composition (M5).

## The permission surface: what three runs measured

The single most important result, and it took three runs to state correctly.

| Run | Posture | Operation | Did the harness ask? |
| --- | --- | --- | --- |
| R2 | `build` | shell command **inside** the workspace | **no** — it ran; the marker file was there afterwards |
| R3 | `plan` | the same command | **no** — the narrower posture changed nothing |
| R5 | `plan` | file read **outside** the workspace | **yes** — and PIO declined it |

So this harness **asks about some operations and not others**, and both halves matter.

**Where it does not ask, PIO observes and does not contain.** R2 and R3 ran a shell command with no request reaching PIO: `decided_by: null`, `outcome: performed`. The effect stayed inside the workspace because the command was an inside-the-workspace one, not because PIO stopped anything. The owner's configuration carries no permission rules, and `mode: plan` — the only per-session lever that does not edit it — makes no difference. This is a **recorded risk acceptance, not a containment claim**, and every receipt carries `os_sandbox_observed: false` and `mechanism: harness_permission_rules_only`.

**Where it does ask, the whole path works.** R5's out-of-fixture read produced a real `session/request_permission`. PIO classified it `outside_fixture`, selected the option whose **kind** is `reject_once`, never touched the always option, and the marker's own content never appeared in anything the harness sent. The harness ended that call `failed` with "The user declined this tool call".

**The option list, measured at last:**

```json
[{"optionId": "once",   "name": "Allow once",   "kind": "allow_once"},
 {"optionId": "always", "name": "Always allow", "kind": "allow_always"},
 {"optionId": "reject", "name": "Reject",       "kind": "reject_once"}]
```

The reviewer's reading was right. The labeled fake had invented `allow`, `allow_always` and `reject` — so **`allow` is not an id this harness uses.** A host hard-coding it would have selected nothing on a caller's allow and silently refused, while a hard-coded `reject` would have worked by coincidence. Choosing by `kind` was the difference between working and failing silently on the first allow anyone tried.

**A caller's own decision was never reached live.** The only request that arrived is one PIO declines by itself, so `respond_action` with a caller's allow or deny has offline evidence only. That is an obligation, not a gap in the runner: the runner drove it, and nothing asked.

## The corrected tool-use audit

Recomputed from each run's own stored transcript and added to each receipt as a dated addendum. The runs were not repeated.

| Run | Tool | Target | Placement | Harness's own status |
| --- | --- | --- | --- | --- |
| R2 | `execute` | `<fixture>` | inside | completed |
| R3 | `execute` | `<fixture>` | inside | completed |
| R5 | `read` | `<fixture>/OUTSIDE_TARGET.txt` | inside | completed |
| R5 | `read` | `<outside>` | **outside** | **failed** |

Before the fix all four were `not_classifiable` with a target digest that was the digest of `{}`. ACP announces a tool call at `status: pending` with an empty `rawInput` and no `locations`, and fills both in later `tool_call_update` messages; the host read only announcements. R5's outside target recomputes to `0fa3f035…`, **the same digest the permission-time classification recorded**, so the audit and the decision now agree about the same call.

## What this harness proves less of than the others

- **It acknowledges nothing.** Codex returns a turn id and Claude Code echoes the message sent, both `provider_ack_id`. OpenCode's delivery carries evidence class `native_session_update` and **no proof class**, so more of its outcomes will stay ambiguous.
- **It has no fidelity discriminator**, as above.
- **Its configuration is not isolable.** With `PATH` alone and no `HOME`, a session still finds the owner's configuration and all 127 models. PIO cannot claim to have prevented the harness from reading its own credential store; it claims only that **it does not read it, passes no key, and lets no variable whose name carries a credential marker reach a child**.
- **Its cancel is ineffective**, as above.

Conversely, one thing it proves **more** of: the provider-and-model refusal **precedes delivery**, which Claude Code's cannot. R1 attempt 1 is that refusal happening for real, at zero model calls.

## Confirmations

### 1. The owner's service was never touched

PID 20947, started Sunday 20 September 20:25:46, recorded by digest before and after **every** run and every probe, and identical each time. PIO passes no `--server`, never runs `service`, and does not need `--standalone`, because `opencode acp` starts its own private server as a child on no port. The R4 kill escalation left nothing behind: no `acp` child and no `serve --stdio` grandchild.

### 2. Nothing was deleted, and what was added is disclosed

Each run creates one session in the owner's own OpenCode history. Every receipt names the session it created and compares the session list before and after. One caveat stated rather than left to be inferred: `session list --standalone` runs with the fixture as its working directory, so the counts are scoped to that directory, not to the owner's whole history. The check that nothing was deleted holds in that scope.

### 3. Every receipt addendum, with its cause

| Receipt | Addendum | Cause |
| --- | --- | --- |
| all 16 committed receipts | absolute home paths redacted | events and receipts recorded paths as the harness reported them; the Claude ones carried the path of every installed plugin from R2 onward. Fixed at the recording boundary and gated in CI |
| R1-attempt-2 | the receipt says `unknown`, the ledger charges 7,910 | the host read `result._meta.usage` and summed two counters; 2.0.11 sends `result.usage` with four. The figure is the harness's own, read from the census in that same receipt |
| R2, R3, R5 | the corrected tool-use audit | the host read only tool-call announcements, which carry an empty `rawInput` |

## Open obligations

1. **A caller's allow or deny, live.** Offline only, because nothing asked.
2. **Restart, reattach and host loss** for this harness: no coverage at all.
3. **A service inventory per run**, which J5 needs.
4. **`execution.discovery`**, never exercised live.
5. **Whether any turn is cancellable**, given R4. One run cannot say it never is.
6. **Linux**, where only the offline matrices run.
7. **A fidelity discriminator**, which this harness may simply not offer.

## Offline evidence behind the same code

OpenCode matrix **24 cases × 3**, eleven through `serve-opencode`, against a labeled fake that now sends the shapes measured from 2.0.11 rather than the ones PIO expected — the correction that mattered twice in one day. Plus the runner self-test, the receipt-shape check that refuses a null or unvarying field, and the private-path gate over every committed file. Green on macOS 15 arm64 and Ubuntu 24.04 x86_64.

**Completion is not acceptance.** Every receipt says so.
