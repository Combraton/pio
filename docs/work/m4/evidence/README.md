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

Each of the 28 rows carries what was observed, what was expected, and
whether they agree, and a disagreement would have failed the run. One row is
marked not provable here: whether PIO deleted a session in the owner's
history. What OpenCode does with a permission prompt is a **record**, with no
expected value, and is never counted; here it is the fake's.

The record also carries the receipt the live run will write: every run's
meter (tool calls, bytes its session sent, the bound, the ceiling), the charge
on a ledger of the rehearsal's own (the reported total times the steps the
turn could have taken, because OpenCode reports only a turn's last step), the
owner's approvals, and the no-sleep assertion held for the run.

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
See issue #12.
