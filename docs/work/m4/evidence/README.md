# M4b evidence

## `lead-tool-probe.json`

The record of `scripts/lead_tool_probe.py`, run under owner approval
`owner-2026-09-22-m4b-lead-tool` item 1 as amended: **one** `session/new`
against OpenCode and **one** `thread/start` against Codex, on private
instances with PIO's own config and home, **no prompt and no model call**,
the owner's background service recorded before and after and asserted
unmoved. Redacted through the same boundary as every other artefact.

The question it answers is not whether a registration is *accepted* — that
is readable from OpenCode's shipped `session/new` validator and Codex's
generated schema. It is whether either harness **launches** the server, and
the only witness that settles that is the server itself. The probe registers
an MCP server that does nothing but write down every request it receives.

Both did. `witness` is that server's own log, in order:

| | Route | Witness |
| --- | --- | --- |
| OpenCode | per-session `mcpServers` on `session/new` | `started`, `initialize`, `notifications/initialized`, `tools/list` |
| Codex | per-thread `config`, overriding what would be read from `config.toml` | the same four |

`approvals_reviewer` is the real app-server's own answer for a thread nobody
redirected: `user`. Measured, not taken from the enum.

**What this does not show.** The probe runs with `HOME` redirected to its own
directory, so the owner's configuration was never in reach. That the
registration arrives without consulting it is **by construction**, not an
observation: nothing here shows what either harness would do with a real
user configuration present. The Claude Code route does not have that gap —
`--strict-mcp-config` is the harness's own guarantee — but for these two it
remains to be shown when a live run has a real configuration beside it.

## `L1-rehearsal.json`

The L1 lead run rehearsed against the labeled fake, through one
`serve-opencode` service, by `scripts/lead_run.py --rehearse` at a clean
committed head (`commit` and `dirty: false` are in the record). **Zero
tokens, no model call.** It is the same code path the live run takes. Only
the harness binary and its environment differ.

Each of the 29 rows carries what was observed, what was expected, and
whether they agree, and a disagreement would have failed the run. Two rows are
marked not provable here: whether PIO deleted a session in the owner's
history, and whether each run's steps could be read from the owner's store (a
rehearsal's home has none). What OpenCode does with a permission prompt is a
**record**, with no expected value, and is never counted; here it is the
fake's.

The record also carries the receipt the live run will write: the
reservations made before the service started, every run's meter (tool calls,
bytes its session sent, the bound, the ceiling), the charge on a ledger of the
rehearsal's own, the owner's approvals, and the no-sleep assertion held for
the run. The charge here is the bound, the reported total times the steps the
turn could have taken, because there is no store to read. Live, it is the sum
of the steps the owner's store records, when its last step is the one
reported.

**What this does not show.** The fake is not a model. It launches the lead
tool the way 2.0.11 was measured doing, then makes the tool calls a script
tells it to. It counts the lines itself. So the two "true count" rows show
that the runner compares the runs' words with its own `wc -l`; the
`wrong-child` and `wrong-relay` mutants are what show the comparison can fail.
Whether a real model uses the tool at all, and reports what the files say, is
the live run's to show.

The first rehearsal record under this name (`d99855b`) was reverted: its lead
had been refused for a missing brief while its children ran, and its runner
had no live path. The second (`d552b55`, from `f3b3895`) was replaced after
review 44, which found that a failure path wrote no receipt and no ledger line,
that the desk exited when nobody answered, that the lead's polling was bounded
only by its deadline, and that an unknown initiator was admitted under a grant.
The third (`29ed7a5`, from `d68adde`) was replaced after review 45: a runner
killed from outside left no ledger line, and the lead's 20-second reads let its
meter stop it before a slow desk answered. See issue #12.

## `L1-live.json`

The L1 lead run, **live**: one `serve-opencode` service, the lead and both
led runs on `minimax-coding-plan/MiniMax-M3`, budget 2, by
`scripts/lead_run.py --desk "start now"` at clean head `703706d`
(`dirty: false`, the binary built by the runner from that head, its digest in
`preflight`). Started 2026-09-24 10:27:35Z, finished 10:28:11Z, detached from
any tool, on AC power with the no-sleep assertion held to the end. The owner
was at the desk; `desk` quotes their words. Reviews 45 to 46b; issue #12.

**Every row holds: 29 rows and one record, none failed, none left
unproven.** The receipt is the scrubbed copy the runner wrote; it was
checked for home and volume paths, the owner's name and the lead's
credential before it was committed, and none is in it.

| | Observed |
| --- | --- |
| The lead used the tool | `start_run` for `alpha` and `beta` (both `started: true`), then `read_run` for each (`7`, `4`). Every request carried the grant. The model did this, not a script. |
| The children | Each ran one shell command (`wc -l`) and answered with the number alone: `7` and `4`, equal to the runner's own `wc -l`. The lead relayed `alpha.md: 7`, `beta.md: 4`. |
| The third start | `started: false`, `call_budget_spent`, by PIO, while the lead ran. |
| Steer under the grant | `not_supported` on `L1.alpha` while `active`/`acknowledged` (still `active` after), and again after it exited, each naming the grant. |
| The desk | **No run asked for an approval**, so the desk relayed nothing. The children's shell commands ran without a prompt, as M3b found for a shell command inside the workspace. The desk path is proven by the rehearsal and its `desk-silent` mutant, not by this run. |
| The owner's history | 0 sessions listed in the fixture repository before, 3 after (the three PIO started); none deleted. |
| The owner's service | the same digest before and after. |

**How the lead reached the tool.** OpenCode 2.0.11 did not expose the lead
tool's functions as tools of their own. The model called OpenCode's `execute`
tool, which runs code the model writes, and that code called the MCP server.
Three `execute` calls carried the four MCP calls: the first used `execute`'s
own `search` and failed on an un-awaited promise; the second made both
`start_run` calls; the third made both `read_run` calls. So the runner's meter
counts three tool calls (ACP `tool_call` updates), the tool's own log four
(MCP `tools/call`), and the store records four model steps. The step bound,
tool calls + 1, holds against the store; the call ceiling is the tool's own
count and would hold a call however it was wrapped.

**The charge: 73,532.** Each run is charged the sum of the steps OpenCode's
store recorded for its session, read under the owner's decision of
2026-09-24 (`session_message` of the three PIO sessions, one `select`,
`mode=ro`: 8 rows), because each last step equals what OpenCode reported.
That is **measured from the store's recorded steps**, which is not the bill.
The receipt names this basis `measured_per_step`, its name at `703706d`; after
review 46 the runner calls it `measured_from_store_steps`.

| Run | Steps recorded | Charged | Reported (last step) | Bound |
| --- | --- | ---: | ---: | ---: |
| `L1` | 8,253 · 10,800 · 10,949 · 11,070 | 41,072 | 11,070 | 44,280 |
| `L1.alpha` | 8,102 · 8,130 | 16,232 | 8,130 | 16,260 |
| `L1.beta` | 8,099 · 8,129 | 16,228 | 8,129 | 16,258 |

The reservations (951,200 + 222,000 + 222,000) were replaced by these lines;
none is left. Sequence `M4-lead-opencode`: **73,532** of the 1,600,000 stop.
MiniMax: **160,395 charged** of 300,000,000 (stop at 240,000,000).

**What this does not show.**
- **That the store holds every billed call.** OpenCode 2.0.11 ships hidden
  agents (title, summary, compaction) whose calls may not be in a session's
  steps. The owner reconciles this run against the MiniMax console.
- That the desk works live: nothing asked.
- That a long `read_run` comes back live: both children had exited before
  the lead read them, so no read waited.
- That the lead stays within its ceilings when it has reason not to: it made
  four calls against a ceiling of sixteen.

**A defect in the runner's release, found by this run.** `release_error`
records that `case_cleanup` refused the live store: the runner never
registered `~/pio-m4-live` as a permitted prefix (the M3b runner registers its
own), so its sweep for surviving processes did not run. The daemon had been
killed before the guard. A check by hand before 10:30Z, `ps` for every
process of the case, found nothing PIO started still running, and the
owner's service and session still running with the same process ids. The live store is kept, as M3b's are, and
`~/pio-m4-live` was made `0700` by hand at 10:30Z. The fix, and a row that
fails on a release error, come before merge.
