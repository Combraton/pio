# M2 acceptance packet — Codex app-server adapter

Assembled 2026-09-19 for [issue #5](https://github.com/Combraton/pio/issues/5) and [PR #6](https://github.com/Combraton/pio/pull/6). Every journey claim below is marked from the live runs in [codex-live](codex-live/), never from the labeled fake. Acceptance is the owner's to give; this packet states what was proven, in what scope, and what was not.

## The M2 scope

These runs prove behavior for:

- **Codex 0.155.1 only**, the exact pinned version, resolved through the npm wrapper, with wrapper, Node and native binary hashes bound and a 312-file canonical schema identity matched with zero drift.
- **macOS 26.3.1 arm64 only.** CI builds and runs the offline matrices on Ubuntu 24.04 x86_64 too; no live Codex ran there.
- **The public Unix API and the attributed CLI**, with the service started from the built binary in the worktree.

They do not prove, and this packet does not claim:

- Claude Code, OpenCode or Hermes Agent, which are M3 and M3b.
- A **fresh package install**: `packaging_check.py` exercises the real launchd and systemd lifecycle in CI, but no live run started through a packaged service. J1's install prerequisite is met only at M6.
- Any TUI path, which is M4.
- CBR composition, which is M5.

## Journeys: four partial acceptances

None of the six journeys is fully accepted. Four are **partial acceptances** in the [shared verification model](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/architecture/VERIFICATION.md)'s sense: each names the properties proven live and the obligations that remain. A partial acceptance cannot open a downstream edge that requires the whole contract, and `not_evaluated` never counts as `pass`. Property results use that model's vocabulary: `pass`, `fail`, `not_evaluated`, `indeterminate`.

### J1 — install and do useful work: partial acceptance

Proven live (`pass`):

- Discovery reports one installation as detected, adapter recognized and version 0.155.1 supported, with **`authentication: unknown` and `usable: false`** until a launch observed it, then `authenticated`, `reachable: yes`, `usable: true` with `last_verified`. Neither query started an app-server. [discovery](codex-live/discovery.json)
- Intent persists before dispatch, the native turn runs, and the work is real: `calc.py` gained the `add` function and `python3 -m unittest -q` exits 0 in the fixture. The output digest equals the invocation receipt's, and `completion_is_acceptance` is false. [R1](codex-live/R1.json)
- Negative control, live: the service refuses `opencode2` with `version_unavailable`, exit 2, no socket, no model call ([wrong-executable](codex-live/wrong-executable.json)); and the previous pin refused this binary as `unsupported_version` before any native work ([version refusal](codex-qualification/version-refusal-at-the-previous-pin.json)).

Remaining obligations (`not_evaluated`): a fresh package install, so the journey's install prerequisite is unmet and belongs to M6; Claude Code, which is M3; Linux x86_64, where only the offline matrices run; and the TUI entry point, which is M4.

### J3 — recover without duplicate work: partial acceptance

Proven live (`pass`), for a daemon kill **after native acknowledgment only**:

- The daemon was SIGKILLed mid-turn and restarted 35.2 s later. The host process and the app-server kept identical pid **and start identity**, so the original native session continued rather than being replaced. [R2](codex-live/R2.json)
- Exactly **one invocation and one `turn/start`** existed throughout: the same operation did not release a second prompt, and replay served the same command rather than creating a new one.
- The controller generation advanced 1 → 2 and the turn then completed.

The delivery was **already `acknowledged` before the kill and stayed `acknowledged`**; it never became ambiguous. What R2 also shows is that `execution_recover` appends an `ambiguous` / `dispatch_may_have_begun` recovery decision for **any dispatched live execution**, whether or not the delivery is already determined, so that entry describes a question that was not open. The frozen `recovery_decision` enum offers only `dispatch_resumed`, `ambiguous` and `failed_before_delivery`, with reasons that are all about whether dispatch happened, so there is no value meaning "the host was reattached and the delivery is unchanged". Filed as [Protocol #15](https://github.com/Combraton/protocol/issues/15) rather than widened locally.

Remaining obligations (`not_evaluated` live; offline evidence exists and is not a substitute): a kill **before** native acknowledgment, where the delivery question is genuinely open; the client-detach half of the journey, covered offline by `client_recovery.py`; the duplicate-launch negative control, covered offline by the diagnostic matrix's J3 mutant and the public matrix's ordering oracle; and restore, which is M6.

### J4 — steer or cancel truthfully: partial acceptance

Proven live (`pass`), for the API and CLI paths:

- `execution.cancel` became a durable control, the host sent a real `turn/interrupt`, Codex answered it, and `turn/completed` reported **`interrupted`**. The app-server exited **0**: it was asked to stop, not killed. The empty interrupt response was not treated as the outcome. [R3](codex-live/R3.json)
- `turn/steer` with the expected turn id was acknowledged, recorded as delivery `acknowledged` with proof class `provider_ack_id`, and **behavior stayed `not_observed`** even though the agent's reply was the steered word. [R4](codex-live/R4.json)

Remaining obligations (`not_evaluated` live): the suppressed-acknowledgment negative control, covered offline by `suppressed_ack_negative_control`; the unsupported and refused steering paths; and the TUI action path, which is M4.

### J5 — independent core: partial acceptance

Proven live (`pass`):

- Discover, submit, inspect and finish all worked with **0 CBR processes, 0 Combraton processes** and no Context client in this build. [R1](codex-live/R1.json)
- Detach and reconnect across a daemon death are covered by the same R2 evidence as J3.

Remaining obligation (`not_evaluated`): the negative control — a separate required-context request holding its boundary when no provider is present — **does not exist yet**. Nothing in this milestone tests it.

### J2 and J6: `not_evaluated`

J2 needs the TUI (M4) and J6 needs a real CBR composition (M5). Neither has any evidence, live or offline.

### The permission surface

[R5](codex-live/R5.json) and [R6](codex-live/R6.json) ran under a per-thread `untrusted` approval policy and received a real `item/commandExecution/requestApproval` for `/bin/zsh -lc 'python3 -m unittest -q'`, carrying the `kind: "command"` field 0.155.1 added. Answering `decline` left Codex's own item status `declined`; answering `accept` made it `failed`, which is the command's exit status after it actually ran. The decision changed what happened, and the evidence is Codex's status rather than a PIO assertion. This supports J2's permission property but does not accept J2, which needs the TUI.


## What the runs cost

Observed from Codex's own usage reports, private ledger at `$HOME/pio-m2-live/private/usage-ledger.json`:

| Run | Tokens | Turn |
|---|---|---|
| R1, first attempt (dirty tree, superseded) | 72,935 | interrupted |
| R1 | 72,911 | interrupted |
| R2 | 117,970 | completed |
| R3 | 23,302 | interrupted |
| R4 | 46,588 | completed |
| R5 | 43,721 | completed |
| R6 | 44,023 | completed |
| **Total** | **421,450** | of the 1,000,000 Codex cap; the 800,000 stop was never reached |

`model-list`, `discovery` and `wrong-executable` started no turn and cost nothing. The [first R1 attempt at the old pin](codex-live/R1-blocked-at-0.146.0.json) was blocked upstream and also cost nothing.

Liability is `resolved` for every run that started a turn: each reports observed usage, never assumed zero.

## Two limits worth stating plainly

- **The token stop is the live runner's rule, not the product's.** It lives in `scripts/codex_live_run.py`, which watches observed usage and calls `execution.cancel` when a run passes its limit. PIO the product enforces no budget: nothing in the service or the host refuses or stops work on cost. That is an open obligation below, not a proven property.
- **A token limit is only as tight as the harness's reporting.** Codex reports usage once per model step. In R1 the observed totals were 24,128, then 48,402, then 72,911, so the runner's 50,000 limit stopped the run at 72,911: the cancel went out 0.7 s after the first report above the limit. No client can enforce a limit below one step's cost mid-step. R2–R6 carried the plan's 250,000 limit and none approached it.
- **R1 and R3 ended `interrupted` by design.** R1 because the runner's limit fired; R3 because the run is the cancel journey. Neither is an upstream failure: `turn_error` is null and the app-server exited 0 in both.

## Everything else that had to hold

- Every live receipt was produced from a **clean committed head** (`dirty: false`) with the binary's own hash recorded, except the first R1 attempt, which is kept under its own name precisely because it was not.
- **No configuration was edited.** Each run snapshots `$CODEX_HOME/config.toml` before and after. The only change across all runs is one trusted-project entry per new fixture repository, reported by digest with a `fixture` label; `other_changes` is false everywhere. R1's re-run added nothing at all, because its fixture was already trusted.
- The service passed `HOME`, `LANG`, `PATH` and `USER` and **no credential variables**. The Node that ran the wrapper was confirmed to be the one the service `PATH` resolves, by canonical path and hash.
- **PIO never selected a model** except under the owner's dated exception for R2–R6, which the service refuses unless the configuration names it. Every receipt carries configured, requested and effective model.
- Public results were validated against the vendored Protocol schemas on every run: 147 to 543 schema-valid results each.

## Offline and CI evidence behind the same code

46 Cargo tests and 1 ignored probe; the offline Codex matrix at **17 cases × 3 = 51 attempts**, all pass on macOS 15 arm64 and Ubuntu 24.04 x86_64 in CI with `dirty: false`; the public matrix 72 (51 pass, 6 property, 9 defense, 6 classifier); the diagnostic matrix 54; caller recovery; the pinned conformance runner 206 pass / 73 unsupported / 1 skipped plus 2 supplemental fixtures. Commands are in [VERIFICATION](../../VERIFICATION.md).

## Open obligations

- Incremental, changed-key commits before M4 and before repetition-heavy M3b testing. No throughput claim until then.
- **Product budget enforcement is not proven.** The only thing that stopped a run on cost was the live runner. Before any release claim about budgets, PIO itself has to refuse or stop work on an exhausted budget, and that has to be tested.
- **`test_only_model_exception` must be removed or compiled out before release.** It exists so the M2 fixture runs could finish on an explicit model. It is refused without the dated token today, but a release must not carry the option at all.
- **A reattach recovery decision.** PIO records `ambiguous` / `dispatch_may_have_begun` on reattach even when the delivery is already determined. [Protocol #15](https://github.com/Combraton/protocol/issues/15) asks for a decision that fits; until it resolves, PIO keeps the inaccurate entry and discloses it here rather than widening its local schemas. If #15 chooses the other route, PIO stops emitting a recovery decision once delivery is determined.
- The J5 negative control must be built.
- The exact-version pin refuses every Codex that is not 0.155.1. That is correct for M2 and wrong for real users; qualifying the schema subset PIO actually uses is an M6 question, not a v0.1 promise.
- [Protocol #13](https://github.com/Combraton/protocol/issues/13) (no capacity-specific error) and [#14](https://github.com/Combraton/protocol/issues/14) (no content path for digest-only fields) remain open upstream. PIO's content extension stays a digest-verified local extension until one lands.
- **Users must be told that Codex writes to their configuration.** Starting a thread in a workspace makes Codex add a trusted-project entry for that path in `$CODEX_HOME/config.toml`. The entries accumulate and PIO never removes them. This is in the README; it needs to be in the installation and first-run documentation too, not only here.
