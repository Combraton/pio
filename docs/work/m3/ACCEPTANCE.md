# M3 acceptance packet — Claude Code adapter

Assembled 2026-09-21 for [issue #7](https://github.com/Combraton/pio/issues/7) and [PR #9](https://github.com/Combraton/pio/pull/9). Every journey claim below is marked from the live runs in [claude-live](claude-live/), never from the labeled fake. Acceptance is the owner's to give; this packet states what was proven, in what scope, and what was not.

## The M3 scope

These runs prove behavior for:

- **Claude Code 2.1.278 only**, as installed from a Homebrew cask that tracks latest and self-updates. Both halves of the interface are bound: a command-line **surface identity** and a **stream identity**, the second because a help digest cannot see the wire. [qualification](claude-qualification/qualification.json), [re-qualification](claude-qualification/requalification-summary.json)
- **macOS 26.3.1 arm64 only.** CI builds and runs the offline matrices on Ubuntu 24.04 x86_64 too; no live Claude Code ran there.
- **The public Unix API**, with the service started from the built binary in the worktree, from a clean committed head with the binary's own SHA-256 in every receipt.

They do not prove, and this packet does not claim:

- OpenCode or Hermes Agent, which are M3b.
- A **fresh package install**; J1's install prerequisite is met only at M6.
- Any TUI path (M4) or CBR composition (M5).

## Journeys: four partial acceptances

None of the six journeys is fully accepted. Four are **partial acceptances** in the [shared verification model](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/architecture/VERIFICATION.md)'s sense: each names the properties proven live and the obligations that remain. A partial acceptance cannot open a downstream edge that requires the whole contract, and `not_evaluated` never counts as `pass`. Property results use that model's vocabulary: `pass`, `fail`, `not_evaluated`, `indeterminate`.

### J1 — install and do useful work: partial acceptance

Proven live (`pass`):

- **The installation is identified and bound before any work.** Version 2.1.278, surface identity matched, stream identity matched, `qualified: true`, **0 model calls**. The credential route is observed and never read: `loggedIn: true`, `authMethod: claude.ai`, `apiProvider: firstParty`, `subscriptionType: max`, with the account's email, organization id and organization name dropped at the boundary. [re-qualification](claude-qualification/requalification-summary.json)
- **Negative control, live:** the same qualifier refuses a Codex binary as not Claude Code, `qualified: false`, before any stream work. [wrong-executable](claude-qualification/wrong-executable-control.json)
- **A turn runs as configured and completes.** R1 passed no model and nothing else: the session loaded **12 plugins, 188 tools, 140 slash commands, 93 skills, 9 MCP servers and 8 agents**, which is the fidelity evidence — the effective model is not, because the product default is Opus with an empty configuration too, and every receipt says so with `effective_proves_fidelity: false`. [R1](claude-live/R1.json)
- **Work with a tool.** R2 read a file inside the workspace and answered; the tool use is recorded `inside_fixture` with a fixture-relative label and no out-of-fixture effect. `completion_is_acceptance` is false. [R2](claude-live/R2.json)

Remaining obligations (`not_evaluated`): a fresh package install (M6); the `execution.discovery` query itself, which **no live Claude run exercised** — qualification and the route observation stand in for it and are not the same thing; Linux x86_64, where only the offline matrices run; and the TUI entry point (M4).

### J3 — recover without duplicate work: partial acceptance

Proven live (`pass`), for a daemon kill **after native acknowledgment only**:

- The daemon was killed 10.4 s in, just after the replay echo, and a new one started 0.04 s later, **while the turn was still generating**: it completed at 12.8 s, so about **2.4 s of the turn ran after the restart** and a fraction of a second ran with no daemon alive at all.
- Across both generations: **one spawn marker and one brief release**, with the host's pid and start identity unchanged. The second daemon reattached to the running host rather than starting anything, and the brief was never re-sent.
- Delivery stayed `acknowledged` on the replay echo that had already proved it; generation advanced 1 → 2; the turn finished with exit 0. [R7](claude-live/R7.json)

That the restart landed **mid-turn** is measured from the host's own event clock and is carried in the receipt; it was added after the fact, because one spawn, one release and an unchanged identity are all equally true of a restart that happens after a turn ends.

Remaining obligations (`not_evaluated` live; offline evidence exists and is not a substitute): a kill **before** acknowledgment, where the delivery question is genuinely open; the client-detach half, covered offline by `client_recovery.py`; and restore, which is M6.

### J4 — steer or cancel truthfully: partial acceptance

Proven live (`pass`) for cancel on the API path, and **not claimed at all for steering**:

- `execution.cancel` became a durable control and the host sent **SIGINT, described as SIGINT**, not an in-band interrupt. The harness answered: a `result` arrived 0.855 s later with `terminal_reason: aborted_streaming` and `status: failed`. [R5](claude-live/R5.json)
- **Usage does not survive a cancel, and the receipt says so.** That `result` carries an empty usage block — every part zero, no iterations — so what the turn spent before the signal is **unknown**. PIO records `reported: false`, `observed_total_tokens: null`, with the reason and the terminal reason beside it; the host publishes **no usage observation**, so the protocol carries unresolved liability rather than a measurement of nothing. Unknown is never zero.
- The turn is charged a **40,000 allowance** under the owner's charge policy, with the basis recorded in the ledger and in the receipt.

**Steering is not claimed.** The Claude host implements `respond_action` and `interrupt` and no steer control, so a second mid-turn message cannot be sent through the service. The optional steering run was deferred rather than run and labelled ([deferred table](../../../scripts/claude_live_run.py), ADR 004 §10).

Remaining obligations (`not_evaluated`): the in-band `interrupt_receipt_v1` the capabilities advertise, unmeasured against 2.1.278; steering, which has no control; and the TUI action path (M4).

### J5 — independent core: partial acceptance

Proven live (`pass`):

- Submit, inspect, the action surface and finish all worked with **no CBR process and no Combraton process**, and no Context client in this build. The service is `pio serve-claude` over a private Unix socket, with its own store and credential per run.
- Detach and reconnect across a daemon death are covered by the same R7 evidence as J3.

Remaining obligations (`not_evaluated`): a packaged service (M6) and the TUI (M4).

### J2 and J6: `not_evaluated`

J2 needs the TUI (M4) and J6 needs a real CBR composition (M5). Neither has any evidence, live or offline.

## The permission surface: one story across four runs

This is the part of M3 that changed most under measurement, and it is stated as a sequence because each run only makes sense against the one before.

| run | operation | what happened | who decided |
|---|---|---|---|
| [R3](claude-live/R3.json) | `touch` inside the workspace | **ran**, marker created | nobody was asked |
| [R3](claude-live/R3.json) | a compound command | **refused** by the harness | nobody was asked |
| [R6](claude-live/R6.json) | read **outside** the workspace | **refused** by the harness | nobody was asked |
| [R3b](claude-live/R3b.json) | `git tag` | **refused** by the harness | nobody was asked |
| [R3c](claude-live/R3c.json) | `git tag`, host attached | **refused**, tag absent | **the caller**, deny |
| [R4c](claude-live/R4c.json) | `git tag`, host attached | **ran**, tag present | **the caller**, allow |

R3b is the run that explained the first four. PIO passed `--permission-prompts host` and believed it was the host; it was not. Attaching takes two things and PIO had neither: **`--permission-prompt-tool stdio`**, which is what makes the CLI send permission requests over the control protocol, and the **`initialize` handshake** that announces the host. Without them the CLI denies anything that would prompt and answers its own model with *"This command requires approval"* — which is what R3b's transcript shows, and PIO never learned it had been asked.

Both are now measured at **zero model calls**: the flag is accepted, and the handshake is answered `subtype: success` in about 0.7 s with keys `pending_permission_requests`, `pending_user_dialog_requests`, `request_id`, `response`, `subtype`. It carries no hooks, no agents and no system prompt, so attaching changes nothing about the session. One useful negative: unlike a user message, that write does **not** trigger `system/init`, so the effective permission mode still cannot be checked before delivery, and every receipt keeps `checked_after_delivery: true`.

With the host attached, R3c and R4c prove the caller's decision path end to end against the real harness: the request is classified `not_classifiable` and **surfaced, never auto-allowed**; the caller's decision is forwarded as a **single-use** allow or deny; **one suggestion was offered and none acted on, with no widening field sent**, on live evidence rather than offline; and the decision changed the world — `git tag -l` is empty after the deny and lists `pio-live-marker` after the allow.

**What this does not make true.** Containment here is the harness's own permission rules and nothing else; `os_sandbox_observed` is false in every receipt. An operation the user's configuration pre-approves — R3's `touch` — never prompts and so never reaches PIO at all. PIO is a decider only for what the harness chooses to ask about.

## Two confirmations

### 1. A request nobody answers is denied, and recorded

PIO decides nothing on the user's behalf except this. The offline case `service_denies_a_request_nobody_answers` surfaces a permission request, answers nothing, and asserts that after the **caller's own declared delivery timeout** the host sends a **single-use deny** recorded as `request_denied_by_default`, with `suggestions_offered: 1`, `suggestions_acted_on: 0` and `widening_fields_sent: []`, that the fake received `deny` with no widening field, and that the turn then exits 0 rather than hanging. The wait is the caller's number, taken from `timeouts.delivery`, not one the host picked.

Mutation-checked: with the expiry disabled, the execution sits in `runtime: requires_action` with a pending action and the case times out — which is exactly the state the default prevents.

### 2. Every receipt addendum, with its cause

Corrections were made in the open: no receipt was rewritten, and each carries a dated addendum saying what it claimed, what was true, and why.

| receipt | addendum | cause |
|---|---|---|
| [R1](claude-live/R1.json) | `false_durable_state_statement` | the snapshot keyed the transcript listing by the directory fixtures are created in, not the workspace, so it read a path the harness never writes to and reported that nothing had been written while a 194 KB session file and a `memory` directory sat on disk |
| [R3](claude-live/R3.json), [R6](claude-live/R6.json) | `tool_uses_counted_attempts_as_effects` | `result.permission_denials` names every tool use the harness refused, and PIO recorded that list in its own events while `tool_use_records` ignored it. R6 reported an out-of-fixture effect with unresolved liability for a read that was refused outright |
| [R3c](claude-live/R3c.json) | `a_refusal_was_attributed_to_the_harness_instead_of_the_caller` | that same denial list says a use was refused and never says by whom, so the deny PIO had just forwarded for the caller was recorded as the harness's own, with "PIO was not asked" beside it |
| [R7](claude-live/R7.json) | `restart_timing_measured_after_the_fact` | the receipt recorded one spawn, one release and an unchanged identity, which are equally true of a restart after the turn ends; the timing that shows it landed mid-turn was measured afterwards from the host's event clock |
| R1, R2, R3, R5, [R5-attempt-1](claude-live/R5-attempt-1.json), [R5-attempt-2](claude-live/R5-attempt-2.json) | `charge_policy_introduced` | the ledger gained `observed` and `charged` after R5, and every stop rule moved to `charged` |

Three runs are kept under their own names rather than replaced:

- [R5-attempt-1](claude-live/R5-attempt-1.json) — **cancelled nothing.** The cancel was accepted 15 s after the turn had already ended and no signal was sent. Accepting a command is not sending one; `signal_sent` and `tested_cancel` exist because of this run.
- [R5-attempt-2](claude-live/R5-attempt-2.json) — **the cancel worked and PIO counted it as zero.** The empty usage block became `basis: observed, amount: 0, liability: resolved`, the ledger counted nothing, and no stop fired because the rule tested whether a report was *present*. Its tokens are unaccounted and it is charged the allowance.
- [R4-not-run](claude-live/R4-not-run.json) and [R6b-not-run](claude-live/R6b-not-run.json) — not run, with the reason and, for R6b, the arithmetic.

## What the runs cost

The ledger carries two numbers per run. **Observed** is what the harness reported; **charged** is what the cap is measured against. They differ only where a turn reported nothing.

| Run | Observed | Charged | Turn |
|---|---|---|---|
| R1 | 33,793 | 33,793 | completed, as configured |
| R2 | 64,375 | 64,375 | completed, one tool |
| R3 | 129,945 | 129,945 | completed, three Bash uses |
| R5 attempt 1 | 32,957 | 32,957 | completed; cancelled nothing |
| R5 attempt 2 | **unknown** | 40,000 | cancelled; usage empty |
| R5 | **unknown** | 40,000 | cancelled; usage empty |
| R7 | 32,517 | 32,517 | completed across a daemon restart |
| R6 | 97,730 | 97,730 | completed; the outside read was refused |
| R3b | 64,483 | 64,483 | completed; no request reached PIO |
| R3c | 67,357 | 67,357 | completed; caller denied |
| R4c | 67,343 | 67,343 | completed; caller allowed |
| **Total** | **590,500** | **670,500** | of the 1,000,000 Claude cap; the 800,000 stop was never reached |

A turn cancelled inside its first model call is charged a flat **40,000**, the basis being the only two single-call turns measured: R1 at 33,793 and R5 attempt 1 at 32,957. That condition is **inferred, not directly measured** — an aborted result zeroes everything, so a cancel during a later model call is not distinguishable from one during the first until the host reads per-message usage.

Two shapes worth keeping:

- **An as-configured session costs about 34,000 tokens before the brief does anything.** R1 spent 33,787 of its 33,793 on cache creation for a one-word reply using no tool. A budget for this harness is really a budget for sessions.
- **A turn's cost scales with the number of model calls, not the size of the brief.** R2's second call read back a cache the first had created: 32,188 created, 32,058 read, for a 125-token answer.

## Everything else that had to hold

- Every live receipt was produced from a **clean committed head** (`dirty: false`) with the binary's SHA-256 recorded, and the binary verified newer than every source file.
- **Every run rehearsed first.** Each declares the observation it is named for, and the runner drives that run against the labeled fake and applies the predicate **before spending a token**; the result is in `preflight.dry_run_check`. Three runs had been named for an observation the runner could not make.
- **No configuration was edited.** Each run snapshots both settings files and `~/.claude.json` before and after; `settings_changed` is false everywhere. `oauthAccount` is never recorded in any form.
- What the runs **did** cause is disclosed: a transcript directory per workspace, containing a session file and a `memory` directory — two entries, counted after the fix and verified against the filesystem for R2.
- The service passed `PATH`, `HOME` and `USER` and **no credential variable**. PIO never selected a model except under the owner's dated exception, which the service refuses unless the configuration names it; every receipt carries configured, requested and effective model.
- A side measurement worth keeping: **the child's shell is the user's, with their aliases.** R3's `ls` resolved to `eza`, which is not on the child's `PATH`, and the command failed. "As configured" reaches further than the settings files.

## Offline and CI evidence behind the same code

100 Cargo tests and 1 ignored probe; the offline Claude matrix at **24 cases × 3 = 72 attempts**, eleven of them through `serve-claude`; the OpenCode matrix at 12 × 3 = 36; the Codex, public and diagnostic matrices unchanged at 51, 72 and 54. All pass on macOS and Ubuntu 24.04 x86_64 in CI. Commands are in [VERIFICATION](../../VERIFICATION.md).

## Open obligations

1. **The host does not read per-message usage.** It records usage once, from `result`, so every stop is a **next-turn** stop and a cancelled turn's usage is unknown. R1 measured that the granularity exists — `assistant` messages carry a full usage block and `result.usage.iterations` holds one entry per model call — so this is PIO's gap, not the harness's. Until it closes, the 40,000 allowance stands in for a measurement.
2. **Whether an attached host is offered an out-of-workspace read is unmeasured.** R6 showed the harness refuses it with nobody attached. R6b would have tested the other case and was not run under the budget rule.
3. **Steering is not claimed, and the in-band interrupt is unmeasured.** Cancel is SIGINT and is described as SIGINT everywhere.
4. **Containment is the harness's own rules only.** `os_sandbox_observed` is false in every receipt, and an operation the user's configuration pre-approves never reaches PIO.
5. **The guard refuses an absent configured `defaultMode`**, which is the state of most real installations. The product's own default is `default` as `init` reports it, a name `--permission-mode` does not accept, so PIO cannot today request "whatever the user would get". Resolve before release.
6. **The exact-version pin and the test-only model exception must be resolved before release.** The pin refuses every Claude Code that is not 2.1.278, which is right for M3 and wrong for real users; `test_only_model_exception` must be removed or compiled out.
7. **Still missing:** the client-detach half of J3, a kill before acknowledgment, a packaged install, Linux live, the TUI path, and **product budget enforcement** — the token stops are the live runner's rule, not the product's. Nothing in the service or the host refuses work on cost.
