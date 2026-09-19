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

## Journeys from live evidence

| Journey | Status | Live evidence | Not covered |
|---|---|---|---|
| **J1 — install and do useful work** | **pass in the M2 scope** | [discovery](codex-live/discovery.json): one installation, detected, adapter recognized, version 0.155.1 supported — and **authentication `unknown`, `usable: false`** until a launch observed it, then `authenticated` / `reachable: yes` / `usable: true` with `last_verified`, with no app-server started either time. [R1](codex-live/R1.json): submit → real native turn → `calc.py` gained the `add` function, `python3 -m unittest -q` exits 0 in the fixture, output digest equal to the invocation receipt's, `completion_is_acceptance: false`. **Negative control, live:** [wrong-executable](codex-live/wrong-executable.json) refuses `opencode2` with `version_unavailable`, exit 2, no socket, no model call; and [the previous pin](codex-qualification/version-refusal-at-the-previous-pin.json) refused this binary as `unsupported_version` before any native work | Fresh package install; Claude Code; Linux; TUI |
| **J2 — terminal concurrency and permissions** | `not_evaluated` | none | All of it; M4 builds the TUI |
| **J3 — recover without duplicate work** | **pass in the M2 scope**, for the daemon-restart half | [R2](codex-live/R2.json): the daemon was **SIGKILLed** mid-turn and restarted 35.2 s later. Host process and app-server kept identical pid **and start identity**, the controller generation advanced 1 → 2, recovery recorded the delivery `ambiguous` for `dispatch_may_have_begun`, exactly **one invocation and one `turn/start`** existed throughout, replay served the same command, and the turn then completed | The client-detach half and the duplicate-launch negative control are offline only: `client_recovery.py`, the diagnostic matrix's J3 mutant and the public matrix's ordering oracle. M6 covers restore |
| **J4 — steer or cancel truthfully** | **pass in the M2 scope**, for the API and CLI paths | [R3](codex-live/R3.json): `execution.cancel` → host control → real `turn/interrupt` → Codex answered → `turn/completed` **interrupted**, app-server exit **0**, cancellation outcome `cancelled`. [R4](codex-live/R4.json): `turn/steer` acknowledged, delivery `acknowledged` with proof class `provider_ack_id`, **behavior `not_observed`** even though the agent's reply was the steered word | The suppressed-acknowledgment negative control is offline (`suppressed_ack_negative_control`); the TUI action path is M4 |
| **J5 — independent core** | **pass in the M2 scope**, except its negative control | [R1](codex-live/R1.json) records **0 CBR processes, 0 Combraton processes** and no Context client in this build, while discovery, submit, inspect and the finished result all worked; [R2](codex-live/R2.json) covers detach and reconnect across a daemon death | The negative control — a separate required-context request holding its boundary with no provider — does not exist yet and is `not_evaluated` |
| **J6 — compose with real CBR** | `not_evaluated` | none | All of it; M5 |

Two further live facts belong to the permission surface rather than to one journey. [R5](codex-live/R5.json) and [R6](codex-live/R6.json) ran under a per-thread `untrusted` approval policy and received a real `item/commandExecution/requestApproval` for `/bin/zsh -lc 'python3 -m unittest -q'`, carrying the `kind: "command"` field 0.155.1 added. Answering `decline` left Codex's own item status `declined`; answering `accept` made it `failed`, which is the command's exit status after it actually ran. The decision changed what happened, and the evidence is Codex's status rather than a PIO assertion.

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

- **A token limit is only as tight as the harness's reporting.** Codex reports usage once per model step. In R1 the observed totals were 24,128, then 48,402, then 72,911, so a 50,000 limit stopped the run at 72,911: the interrupt went out 0.7 s after the first report above the limit. No client can enforce a limit below one step's cost mid-step. R2–R6 carried the plan's 250,000 limit and none approached it.
- **R1 and R3 ended `interrupted` by design.** R1 because PIO's own budget rule fired; R3 because the run is the cancel journey. Neither is an upstream failure: `turn_error` is null and the app-server exited 0 in both.

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
- The J5 negative control must be built.
- The exact-version pin refuses every Codex that is not 0.155.1. That is correct for M2 and wrong for real users; qualifying the schema subset PIO actually uses is an M6 question, not a v0.1 promise.
- [Protocol #13](https://github.com/Combraton/protocol/issues/13) (no capacity-specific error) and [#14](https://github.com/Combraton/protocol/issues/14) (no content path for digest-only fields) remain open upstream. PIO's content extension stays a digest-verified local extension until one lands.
