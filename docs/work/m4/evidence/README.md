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

Each of the 32 rows carries what was observed, what was expected, and
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
meter stop it before a slow desk answered. The fourth (`703706d`, from
`2172913`) is the one the live run was approved on; it was replaced after
that run and review 46, which added three rows: the live tree can be released
(checked through a probe made the same way as the live tree, then removed), the
runner's own steps raised no error, and the service was released with nothing
surviving. See issue #12.

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

## `L1b-rehearsal.json`

L1b rehearsed against the labeled fake by `scripts/lead_run.py --rehearse
--plan L1b` at clean head `e042672` (`dirty: false`), for the second attempt. **Zero tokens, no model
call.** Plan approved by the owner on 2026-09-24, with review 48's amendments
(issue #12).

**Every row holds: 33 rows and one record, none failed.** Two rows are not
provable in a rehearsal, as for L1: the store read, and whether a session was
deleted. What L1b adds:

- **The desk had something to decide.** `beta`'s read of `beta.env` was
  relayed as `L1b.beta.action-1`, answered `allow` and sent as `allow_once`.
  It was decided by the caller, never "always", and nothing lapsed. **Here
  the rehearsal answered for itself**, and the fake asked. Live, OpenCode
  asks (its shipped default for `*.env`, review 48) and the owner answers.
- **A read waited.** The lead's `read_run` of `L1b.alpha` took **30.6 s**, by
  the tool's own log, and returned `exited`. The row also holds on a read that
  came back still running after the tool's 55 s limit (owner decision,
  2026-09-25; mutant `alpha-outlasts`).
- **Review 47's rule.** A desk nobody asked is inconclusive (mutant
  `no-ask`), and so is a session listing that was empty before (live only).

The record under this name at `117f283` was the one attempt 1 ran on; it was
replaced before attempt 2, after the waiting row was widened.

**What this does not show.** The fake is not OpenCode. Its read request uses
the read tool's parameter name (`filePath`), not a measured permission
request. Whether 2.0.11 asks about `beta.env` live is the live run's to show.
If it doesn't ask, the desk row is inconclusive, not passed.

## `L1b-live.json`

L1b, **live**, at clean head `990c985` (`dirty: false`), 2026-09-24
**18:11:55Z to 18:19:35Z**, detached, **on battery** (the owner lifted the AC
condition; the no-sleep assertion was held to the end). The owner's desk
words are in `desk`, verbatim except that their "HH:MM" placeholder was filled
with the start time, and "on AC power" was changed to "on battery" at the
owner's answer. Review 49 cleared the run; issue #12.

**L1b failed, and its cause is the desk.** `beta`'s read of `beta.env` came to
the desk (`L1b.beta.action-1`, requested 18:14:16Z) and was relayed at once.
The owner's answer reached the relay at about 18:28Z, after the 300-second
deadline (18:19:16Z). So **the host's single-use reject landed** (`reject_once`,
decided by PIO, never "always"), and `beta` exited with no count. Three rows
fail on that one cause: the desk row (lapsed), the children's counts (`beta`:
none), and the lead's relay (`beta.env`: none). The owner's answer was
"always allow". It could not have been sent as "always" in any case, because
the desk encodes only `allow` and `deny`, and an allow is sent as `allow_once`.

| | Observed |
| --- | --- |
| **The desk request** | OpenCode's read tool, `<fixture>/beta.env`, classified **`inside_fixture`**, disposition `surface_as_action`. OpenCode's options: `once` (`allow_once`), `always` (`allow_always`), `reject` (`reject_once`). So 2.0.11 does ask about `*.env` live, as the static reading said. |
| **The waiting read** | The lead's `read_run` of `alpha` took **54.6 s** and returned `exited` with `7`. `beta`'s reads took 56.8, 56.5, 57.6 and 56.5 s (each `requires_action`), then 31.1 s (`exited`, after the reject), then 1.4 s. Nine tool calls against a ceiling of sixteen. |
| **The lead** | Relayed `alpha.md: 7` and left `beta.env` blank: "Beta's text is empty, so the number beta reported is empty/absent." It invented nothing. Midway it said "Acknowledged — I won't use the pi-delegate skill": a skill from the owner's own OpenCode setup was visible to it, as configured. |
| **Everything else** | Held, as in L1: tool on the lead only, launched once; both starts; the third refused `call_budget_spent`; steer `not_supported`, running and exited; the lead cannot answer; release clean; the owner's service untouched. |
| **Inconclusive** | "PIO deleted no session": 0 sessions listed before (3 after), so nothing could be deleted (review 47). |

**The charge: 128,790**, measured from the store's recorded steps (one
`select` on `session_message` for the three PIO sessions, `mode=ro`, 13
rows), not the bill.

| Run | Steps recorded | Charged | Reported (last step) |
| --- | --- | ---: | ---: |
| `L1b` | 7,964 · 10,528 · 10,729 · 10,897 · 11,049 · 11,153 · 11,257 · 11,404 · 11,514 | 96,495 | 11,514 |
| `L1b.alpha` | 8,113 · 8,169 · 8,197 | 24,479 | 8,197 |
| `L1b.beta` | 7,816 | 7,816 | 7,816 |

Sequence `M4-lead-opencode`: **202,322** of the 1,600,000 stop. MiniMax:
**289,185 charged** of 300,000,000. The owner reconciles 18:11–18:20Z against
the MiniMax console.

## `L1b-attempt-2-live.json`

L1b's second attempt, **live**, at clean head `0ba60de` (`dirty: false`),
2026-09-25 **04:59:36Z to 05:01:11Z**, detached, **on AC power** (the no-sleep
assertion was held to the end). It ran under its own ledger name,
`L1b-attempt-2`, with the same plan, model and sequence, as the owner approved
on 2026-09-25 (after review 50; review 51 gave the go). The owner confirmed the
desk words in `desk` before the start. Issue #12.

**Every row holds but one, which is inconclusive.** Of 33 rows, 32 hold (and
one record, of what OpenCode did with the prompt). The exception is "PIO
deleted no session": no sessions were listed before the run (3 after), so
there was nothing to delete (review 47).

**The desk: one item, applied by the relay from the owner's advance decision,
not answered live.** The owner decided `beta`'s `.env` read in advance
(2026-09-25, item 3). The relay (`L1b-attempt-2-relay.py`, below) answered
that one request by itself, and would have brought any other to the owner
live. There was no other.

| Desk item | Requested | On the desk | Answered | Sent | Who answered |
| --- | --- | --- | --- | --- | --- |
| `L1b.beta.action-1`: OpenCode's read tool, `<fixture>/beta.env`, `inside_fixture` | 05:00:31Z | 05:00:32Z | 05:00:32Z | 05:00:33Z (`allow_once`) | The relay, applying the owner's decision of 2026-09-25, item 3, quoted in the answer's `words` |

Within a second of the request reaching the desk, and two seconds after the
request, counted in whole-second stamps. `decided_by` is `owner` in the desk
answer, and `caller` in PIO's own record: the answer came from the caller's
side, not from PIO's default. OpenCode offered the same options as in attempt 1:
`once` (`allow_once`), `always` (`allow_always`), `reject` (`reject_once`). No
`*_always` option was taken.

| | Observed |
| --- | --- |
| **The waiting read** | The lead's `read_run` of `alpha` took **53.1 s** and returned `exited`, with `alpha`'s text "I'll run the sleep command and count the lines in parallel.7". `beta`'s read took **0.4 s** and returned `exited` with `4`. Four tool calls in all: two starts and two reads. |
| **The counts** | `alpha.md: 7` and `beta.env: 4` from the children, and the same two in the lead's answer. They match `wc -l`. |
| **The lead** | It said at first that it could not see the tool, then that it could, and it started both runs and read them. Its answer was "alpha.md: 7 / beta.env: 4". |
| **Everything else** | Held, as in L1: tool on the lead only, launched once; both starts; the third refused `call_budget_spent`; steer `not_supported`, running and exited; the lead cannot answer an approval; a credential in a tool spec is refused; release clean; the owner's service untouched. |
| **Inconclusive** | "PIO deleted no session", as above. |

**The charge: 82,910**, measured from the store's recorded steps (one `select`
on `session_message` for the three PIO sessions, `mode=ro`, 8 rows), not the
bill.

| Run | Steps recorded | Charged | Reported (last step) |
| --- | --- | ---: | ---: |
| `L1b` | 8,293 · 14,024 · 14,175 · 14,356 | 50,848 | 14,356 |
| `L1b.alpha` | 7,845 · 7,919 | 15,764 | 7,919 |
| `L1b.beta` | 8,113 · 8,185 | 16,298 | 8,185 |

Sequence `M4-lead-opencode`: **285,232** of the 1,600,000 stop. This was the
last attempt the stop allowed; there is no attempt 3 without a new decision by
the owner. MiniMax: **372,095 charged** of 300,000,000. The owner reconciles
04:59–05:02Z against the MiniMax console.

**What this does not show.** That the store holds every billed call: OpenCode
2.0.11's hidden title, summary and compaction agents may not write to a
session's messages, which is why the console reconciliation exists. The model
those hidden agents used is not in the receipt either. The owner's
`opencode.jsonc` sets no `small_model` and no agent override. Whether `alpha`
ran `sleep 30` before counting ("in parallel", it said) is not read from the
store. Only the 53.1 s wait points to it.

## `L1b-attempt-2-relay.py`

The desk relay that ran beside attempt 2, committed as it ran (sha256
`6e735c82…`, the same as the copy that ran). It reads the live tree's name from
the runner's log and polls it with `os.listdir` and `open()`, so no command
line names the tree (the cleanup kills any process group whose command line
does). It answers only `L1b.beta` · `read` · `<fixture>/beta.env` ·
`inside_fixture`, with `allow` and the owner's words. On any other request it
prints it and exits, so the request goes to the owner live. It exited on
`RUNNER EXITED` after answering one request.

## `L3-live.json`

The first live L3, on 2026-09-26 from 08:54:14 to 08:55:48Z, at clean head `8be3803`, on AC power. It ran on the orchestrator's dated go under the owner's delegation of 2026-09-26. It is **not independently verified**: the owner paused independent verification until the TUI phase. The receipt's `desk` field is that go line, not the owner's words; the owner was not at the desk.

**It failed, and it failed early.** The lead's only messages were "I'll start the two requested lead runs and poll only those runs to completion." and then "I can't access a `pio-lead` tool in this session, so I can't obtain the requested numbers without violating your constraint." It took two model steps and never called `start_run`. So no child ran, and every row about the children, the steers, the reads and the relayed counts fails.

**Why** (read from Codex's source at `rust-v0.157.0`, not measured):
- `gpt-5.6-terra`'s catalog entry sets `tool_mode: "code_mode_only"` (`models-manager/models.json:676`). The model's tool mode wins over any feature flag (`core/src/tools/mod.rs:73-95`).
- In that mode, the model sees only `exec` and `wait` at top level (`core/src/tools/spec_plan.rs:369-404`).
- Every tool from an ordinary MCP server is deferred (`core/src/mcp_tool_exposure.rs:90-94`) and is never named in `exec`'s description (`code-mode-protocol/src/description.rs:291-356`). PIO's tool was reachable only as `tools.mcp__pio_lead__start_run(...)` inside an `exec` script, and nothing told the model that.
- Codex did start the server: the lead tool logged `initialize`, `notifications/initialized` and `tools/list`, and then no `tools/call`.
- A server can opt out of deferral with `omit_tools_from = ["code_mode","deferred"]` (`spec_plan.rs:255-270`). That is PIO's own setting on its own server, and the next attempt sends it, with `required = true`.

**The rehearsal could not see this.** The labeled fake lists the lead tool's tools and scripts the lead's calls directly; it does not model which tools a model can see. Once more, a fake that sends the shape PIO expects cannot test PIO.

**The lead's one tool call** went to `cua_repl`, from the owner's Codex plugins.
- It is the one MCP tool its plugin manifest keeps visible in code mode.
- It is marked read-only, so Codex approves it without asking (`core/src/mcp_tool_call.rs:2436-2461`). No request reached PIO, and nothing was declined.
- It returned the plugin's instructions and a list of applications on the machine to the model. Nothing of that is in this receipt.

Whether the owner's plugins stay on for PIO's test runs is the owner's decision.

**What held:**
- the model and provider checked before the turn;
- approvals to the user;
- only the lead got the tool, pre-allowed and nothing else;
- every run within its ceilings and its share;
- every step within the in-flight bound (largest 20,957);
- no sub-agent and no turn after the turn;
- no stream retry;
- the owner's Codex configuration changed only by the fixture's trust entry;
- the owner's OpenCode service untouched;
- the service released, with nothing surviving.

**Charged 36,126,** from Codex's own total. The Codex ledger now stands at 457,576.

**The memory row failed conservatively, on a change the pipeline did not make.**
- `memories_1.sqlite` kept its size, but its modification time moved.
- Codex opens every one of its runtime databases at app-server start, whatever features are on, and rewrites a header on connect (`state/src/sqlite.rs:251-310`).
- `goals_1.sqlite` changed in the same second, although Goals was off too.
- The pipeline itself is gated on the thread's configuration, which carried the per-launch override (`memories/write/src/start.rs:34`).
- The row will be corrected so that a size-preserving change it cannot attribute is inconclusive, not a failure.

## `L3-rehearsal.json`

L3 rehearsed against the labeled Codex fake by `scripts/lead_run.py --rehearse
--plan L3` at clean head `fff09be` (`dirty: false`), after round 4 of the
review of L3 and the owner's decisions of 2026-09-26. **Zero tokens, no model
call.** The fake is `pio codex fake-app-server` (`pio-fake-app-server`): it
speaks 0.157.0's app-server shapes and runs no model, tool or command, and
`serve-codex` does not qualify a labeled fake. Plan: owner approval of
2026-09-24, the decisions of 2026-09-25 (issue #12), and the owner's
decisions of 2026-09-26: L3's sizing ("More headroom": lead 150,000, child
60,000, sequence cap 565,000 and stop 452,000) and Codex's five unmetered
features off per launch.

**44 rows and three records: 41 rows hold, none failed, and three are
inconclusive**, the three this rehearsal cannot observe:

- **Codex qualified at the pinned identity**: the labeled fake has no
  qualification record (`qualification_check`: not checked). Live, a record
  that differs from the committed one refuses the run before anything is
  reserved (`qualified-elsewhere`), and the row states the result;
  `qualified-as-committed` holds it on a planted record.
- **Every stop the runner made reached Codex**: no run is stopped here. The
  mutant `child-overspends` must hold it (alpha's interrupt sent,
  acknowledged, its turn interrupted) and `ceiling-cancel-never-sent` fails
  it.
- **Every run that spawned a sub-agent was stopped when it appeared**: none
  spawned one. `subagent-spawned` holds it and `subagent-not-stopped` fails
  it.

The owner's OpenCode service row held on this machine: a service of the
owner's was running before and after, unchanged. Where none runs, as on CI,
it is inconclusive (`owner-service-absent`).

What L3 shows, as the fake plays it:

- Codex's five unmetered features are off on every thread per launch,
  under the owner's decision `owner-2026-09-26-l3-codex-unmetered-features-off`,
  which the L3 plan sends: the receipt's route for sub-agents, memories,
  goals, standalone web search and image generation is "per launch" for
  each, and every thread's `thread/start` config carried exactly the eight
  keys of `pio_codex::features_off` (`features_off_sent`, read back by the
  host from its own request). The Codex home here has no `config.toml`, so
  no owner key was read; live, the owner's are read by those keys alone
  (`memories = true`, nothing else, on 2026-09-26), and without the decision
  the runner refuses for all five (`decision-absent`).
- The lead tool is on the lead's thread only. The host read back from its
  own `thread/start` request exactly `start_run` and `read_run` at
  `approval_mode: approve` and no server-wide default; the children's
  threads carried no server. Nothing was asked about the lead's tool, by
  any path, and PIO declined nothing by itself, before, during or after a
  turn (record: empty for all three runs).
- Each thread's model and provider (`gpt-5.6-terra`, `openai`) were checked
  from `thread/start`'s answer before its first turn. `approvalsReviewer` is
  `user` on all three.
- The lead's steer on `L3.alpha` was recorded under its grant while the turn
  was `active` and `acknowledged`, and acknowledged with `provider_ack_id`.
  Behavior is `not_observed`.
- The lead's `read_run` of `alpha` took 30.4 s and returned `exited`.
- Each child said what it was about to run before its command, as measured
  on this model (``Running `sleep 30 && wc -l alpha.md`.``), then answered;
  the count row read each child's last message after its last command: 7
  and 4. The lead's answer came one message per file and was read as two
  lines.
- `beta`'s command approval came to the desk with its placement
  (`inside_fixture`, `<fixture>/`), no network ask and a null reason, was
  answered `allow` by the rehearsal, and was sent as
  `{"decision": "accept"}`.
- The lead's tool answered four calls, each through its gate (one gate
  line each in its log); none was handed back past the hold (120,000), and
  no result reached the lead that its tool never saw.
- Every model step was 4,096 tokens, within the 30,000 the bound assumes in
  flight (the host's own usage events: the lead 5 steps, each child 2). No
  run had a thread but its own, no run took a turn of its own after its turn
  had ended (each host read its run's thread for three seconds after the
  turn), and every exit carried both lists (empty). No stream retry was
  reported. No execution under the lead but the plan's three.
- Codex's memory state did not change (there is none in a labeled fake's
  home, and memories were off per launch).
- Codex's runs were metered on the event stream and on each run's own host
  events by a thread of their own: 320 passes in 48.3 s. No stop and no
  silence.
- The charge is Codex's reported totals, 20,480 + 8,192 + 8,192 = 36,864,
  on the rehearsal's own ledger, read with each run's host events afresh:
  each at least what the run could have spent, and each within its share.
  The reservations, written immediately before the lead's submit (210,000 +
  120,000 + 120,000 = 450,000), were replaced and none is left. Every
  submit made an execution, and no probe was admitted.

**What only the live run can show.** These are the places where
`lead_run.py` takes a different branch live, or where a row that holds here
holds only because of what the fake does (reviews of L3, F13 and round 2):

- **Codex itself.** Live runs the owner's `~/.local/bin/codex` with the
  owner's home and `~/.codex`. `serve-codex` qualifies it before any native
  work, and the receipt's `harness.qualification` names the npm wrapper, the
  native binary and the Node that ran it (Hermes-managed), each by label and
  sha256, from that record. Right after the service starts, and before
  anything is reserved, the runner compares them, the pin, the version and
  the schema listing with the committed record and refuses a run that
  differs (`qualified-elsewhere`: refused, nothing reserved); the row "Codex
  qualified at the pinned identity" states the same in the receipt. Here the
  labeled fake has no qualification, so that row is **inconclusive**, and
  only a planted record is checked (`qualified-as-committed` holds it). A
  service that never becomes ready leaves no reservation
  (`service-never-ready`).
- **Codex features.** Before anything is reserved the runner reads two
  features of the Codex configuration's `[features]`,
  `exec_permission_approvals` and `request_permissions_tool`, each by its
  own key and by every legacy alias Codex 0.157.0 lists for it
  (`request_permissions` for the first; `features/src/legacy.rs`), the
  last key in sorted order deciding, as Codex applies them. It refuses to
  start if either is on, or set to something Codex would not read as a
  switch, and records which key decided: PIO does not opt into Codex's
  experimental API, so Codex strips a command approval's extra permissions
  from what the desk sees, and an allow would grant them unseen
  (`experimental-feature-on`; `experimental-alias-on`, the alias set true
  beside the canonical key set false). A feature set by a profile, a
  managed configuration or a command-line override is not read.
- **The sizing.** Codex reports a step once its tool has finished (M2 R5,
  R6), so a report that crosses a ceiling arrives with the next step begun.
  From the code, each run can reach its ceiling plus two steps. The limits
  are the owner's of 2026-09-26 ("More headroom"): a child 60,000 + 2 x
  30,000 = 120,000, and the lead 150,000 + 2 x 30,000 = 210,000. The lead's
  tool withholds any response (a result, an error or an unknown tool) once
  the lead has reported more than 120,000, and the meter stops the lead
  when a result its tool never saw comes back past 120,000; that keeps a
  lead that uses only its tool within 180,000 (`lead-heavy`,
  `lead-tool-error-past-hold`). But a result the tool never sees (Codex's
  own shell, another MCP server, a refused approval) is back, with the next
  step begun, before anything can see it, when the lead may be just under
  150,000 (review of L3, round 2, SB-1; `lead-shell-past-hold`). The worst
  case is **450,000** (210,000 + 2 x 120,000). For the second attempt it is
  within the L3 sequence's 488,000 stop (cap 610,000; owner, 2026-09-26, Q8)
  after attempt 1's 36,126 (486,126), and, with Codex's 457,576 used, under
  the 908,000 Codex stop (cap 1,135,000): 907,576 (`--sizing-selftest`).
  Attempt 1 was checked against 452,000 (cap 565,000) and 872,000 (cap
  1,090,000), with Codex's 421,450 used: 871,450. Under the limits of
  2026-09-24 (lead 125,000, child 50,000, sequence cap 400,000 and stop
  320,000, Codex cap 1,000,000 and stop 800,000) it was 405,000, past every
  one of them, and the live runner refused L3 until the owner decided the
  sizing. That figure holds only with Codex's unmetered features off
  (below). With sub-agents on, each run could also start sub-agents in one
  step before its stop lands, each with a step in flight: **720,000** for
  multi-agent V2 (three resident per run, 450,000 + 3 x 3 x 30,000) and
  **990,000** for V1 (six), from Codex's source at rust-v0.157.0 and not
  measured. The bound rests on a step being at most
  30,000; the runner now checks every report against that, stops a run
  whose step is larger, and charges its largest step in flight
  (`step-past-in-flight`; round 3, SPEND-4). Codex retries a dropped stream
  up to 5 times on each transport, so up to 10 on the step that falls back
  from WebSocket to HTTPS, and a failed request up to 4 times, by default
  (the owner's `model_providers` settings can raise either and are not
  read); the first WebSocket retry of a step is not surfaced in a release
  build. It records usage only on a completed response: whatever a dropped
  attempt is billed is never reported and is outside the bound (not
  measured). The host now keeps each `error` notification's `willRetry`, and
  the receipt counts the retries Codex surfaced per run ("Stream retries
  Codex reported"), and says why none is charged (review of L3, round 4,
  SPEND-10).
- **Sub-agents.** Codex 0.157.0 has `multi_agent` on by default and
  attaches every thread it creates to every initialized connection, so a
  sub-agent a run spawns reports on PIO's connection. The host now tells
  threads apart: another thread's notifications never end the run's turn
  or speak in its output, its requests are declined by PIO, its usage is
  added to the run's (the run's usage is the sum over its threads), it is
  interrupted with the run, and the run's exit carries it
  (`pio.combraton.dev/other-threads`). The runner stops a run the moment
  one appears and charges it a step in flight on each of its threads; the
  rows "No run spawned a sub-agent" and "Every run that spawned a sub-agent
  was stopped when it appeared" judge it (`subagent-spawned`,
  `subagent-not-stopped`, `subagent-uncounted`). What the fake spawns is
  0.157.0's shape read from source (a `subAgentActivity` item, then the
  agent's own thread), not a measurement; whether the live model would
  spawn at all is not known.
- **Codex's unmetered, default-on features** (review of L3, round 4, U1).
  Five features of Codex 0.157.0 spend outside every report PIO reads:
  sub-agents (threads of their own), the memory pipeline (the owner's idle
  sessions sent to an extraction model and a consolidation agent, on the
  order of 900,000 input tokens an attempt), goals (a continuation turn
  after `turn/completed`), standalone web search (`web.run`, its own model
  call) and image generation (a separate endpoint). **Live, the runner
  refuses to start unless each is off for L3's threads**, by the owner's own
  `~/.codex/config.toml` or per launch under a recorded owner decision,
  which puts every key of
  `pio_codex::features_off` in each thread's config: `agents.enabled`,
  `features.multi_agent` and `features.multi_agent_v2` false,
  `features.memories` and its legacy alias `features.memory_tool` false,
  `features.goals` false, `web_search = "disabled"` and
  `features.image_generation` false, each with its source at rust-v0.157.0.
  The preflight lays the override over the owner's keys as Codex lays a
  request override, resolves each feature as Codex does (a feature's keys
  in sorted order, the last deciding), names each feature not off and the
  key that decided it, and records each feature's route: the owner's
  configuration, per launch, or Codex's default. It reads those keys and
  nothing else. The owner's configuration, read by those keys alone on
  2026-09-26, sets `[features] memories = true` and none of the others, so
  every one of the five is on without the override. `multi_agent = false`
  alone is not enough for sub-agents: at rust-v0.157.0 the model catalog's
  multi-agent version wins over it, and `gpt-5.6-terra`'s bundled entry
  names V2 (`subagents-multi-agent-only`). The check mutants plant a Codex
  home with every other feature off and one on (`subagents-unguarded`,
  `memory-unguarded`, `memory-alias`, `goals-unguarded`,
  `web-search-unguarded`, `image-generation-unguarded`), and
  `override-misses-alias` models an override without `memory_tool` beside
  the owner's `memory_tool = true`: each is refused for its own feature and
  no other. `overrides-on` turns all five off per launch over a home that
  turns each on, and holds. **Owner decision, 2026-09-26** ("Per-launch
  override"): "L3's Codex threads are launched with these five off, per
  launch, under a dated test-only exception like the lead-tool
  pre-allowance. Your ~/.codex/config.toml is not changed." It is recorded
  as `owner-2026-09-26-l3-codex-unmetered-features-off` in pio-protocol's
  `FEATURES_OFF_DECISIONS`, the one entry there, and the L3 plan sends it for
  its own threads and no other plan's, live and rehearsed; the receipt
  records each feature's route. Read by those keys alone, the owner's
  configuration leaves all five on, so live the route for each is per
  launch; without the decision the runner refuses for all five
  (`decision-absent`, over the owner's keys as read). The rehearsal's own
  token remains for the matrix and for the mutants that model the override
  (`overrides-on`, `override-misses-alias`); the mutants that play a Codex
  whose features are on send none. Behind the override, the host keeps reading a
  run's own thread for three seconds after its turn has ended: a turn Codex
  starts there by itself (a goal's continuation) is interrupted, carried on
  the exit (`pio.combraton.dev/continuations`), and its run charged as cut
  short, and the row "No run took a turn of its own after its turn ended"
  fails (`goal-continued`; `continuation-uncharged` charges it as ended by
  itself, and the floor row fails).
- **The pre-allowance.** Whether Codex honours the per-thread
  `tools.<name>.approval_mode: approve` and asks nothing before the lead's
  tool calls. The fake implements that mode itself, so the row shows that
  the host sends exactly the two tools and no default mode (read back from
  the request, and witnessed by the fake), and that the runner reads it. If
  Codex asks anyway, by any path, the row fails and says the pre-allowance
  did not hold: an ask PIO surfaces (the relay then answers, item 3b); one
  in a shape PIO does not recognise, such as `openai/form`, which PIO
  declines by itself (`lead-asked-in-openai-form`); or one by
  `item/tool/requestUserInput`, Codex's route when its elicitation route is
  off (`lead-asked-by-user-input`). A quiet run cannot tell an honoured
  pre-allowance from a Codex that would not have asked: there is no control
  tool without it.
- **The MCP elicitation.** When Codex asks before an MCP tool call, what it
  offers, and how it reads the answer. This is read from its source at
  rust-v0.157.0 (`codex-rs/core/src/mcp_tool_call.rs`; the functions that
  decide, build and parse are identical at 0.155.1, the file is not) and not
  measured. Nothing in this rehearsal asked it.
- **What PIO declines by itself.** A request the host answers with an error
  (a permission grant, a login or a form asking for data, a request for the
  user's input, a tool call it does not run) is recorded with what was
  asked and why, never an argument, a form, a URL or a question's text,
  carried on the run's exit event, listed in the record "What PIO declined
  by itself", and fails the desk row. That now includes a request that
  arrives while the host waits for `initialize`, `account/read` or
  `thread/start` (phase `before_turn`; before round 2 it was dropped
  unanswered). The fake sends none unless a mutant or case does. Live, the
  owner's plugins, apps and own MCP servers are off on every L3 thread
  (below), so what could still ask is Codex itself and PIO's lead tool;
  each ask PIO declines fails the desk row, by design.
- **The lead's tool in the model's own list** (L3's first live run,
  2026-09-26). `gpt-5.6-terra` runs code-mode-only
  (`models-manager/models.json:676`): the model sees `exec` and `wait`, and
  an ordinary MCP server's tools are deferred behind `exec` and never named
  (`core/src/tools/spec_plan.rs:234-266`, `core/src/mcp_tool_exposure.rs:90-94`).
  The lead's thread now sends PIO's own server table with
  `omit_tools_from = ["code_mode","deferred"]`, `required = true` and
  `startup_timeout_sec = 30` (`config/src/mcp_types.rs:229-256`, `:372-385`;
  the surfaces' names `protocol/src/config_types.rs:396-407`), which makes
  its tools `DirectModelOnly`, in the model's own list
  (`tools/src/tool_executor.rs:68-72`; `spec_plan.rs:528-566`, `:761-772`).
  Read from source, not measured: `code_mode` alone would do on this model,
  and `deferred` alone would keep them inside `exec`. With `required` Codex
  refuses `thread/start` if the server fails to start
  (`codex-mcp/src/connection_manager/required.rs:15-58`); before the lead's
  turn the host also waits, bounded, for Codex's
  `mcpServer/startupStatus/updated` = `ready` for it on the lead's thread
  (`app-server/src/bespoke_event_handling.rs:202-228`). The rehearsal's fake
  computes the exposure the same way and plays a lead that cannot call a
  tool not in its list, as the live one could not (`lead-tool-deferred`).
  Whether the live model then calls `start_run` is the model's. The lead's
  brief names the tools as the model sees them: `start_run` and `read_run`,
  namespace `mcp__pio_lead` (`codex-mcp/src/tools.rs:228-234`,
  `codex-mcp/src/rmcp_client.rs:850`, `core/src/tools/handlers/mcp.rs:504-505`).
- **The owner's plugins, apps and MCP servers** (owner decision,
  2026-09-26, Q7). L3's first live lead called `cua_repl`, a tool of the
  owner's Computer Use plugin that Codex approves without asking as
  read-only. Every L3 thread, lead and children, now carries
  `features.plugins`, `features.apps` and its legacy alias
  `features.connectors` false, and each MCP server the owner's `config.toml`
  names by a table header as `enabled = false` inside the thread's own
  `mcp_servers` table (`pio_codex::plugins_off`, each key with its source).
  Per thread, from source: the plugin manager loads no plugin
  (`core/src/config/mod.rs:1692-1700`, `core-plugins/src/manager.rs:776-779`),
  so no plugin server is registered (`core/src/config/mod.rs:1735-1783`,
  `ext/mcp/src/plugin.rs:123`, `:207`); the apps server is not registered
  (`core/src/config/mod.rs:1824`, `core/src/mcp.rs:323-335`,
  `ext/mcp/src/lib.rs:50-51`, `core/src/connectors.rs:125-130`, `:141`); and
  a disabled server is never started
  (`codex-mcp/src/connection_manager.rs:241-251`, `:288-291`). The servers
  are read from header lines alone; the receipt carries them as digests and
  a count. Two rows judge it: what each thread's host sent, read back from
  its request, and every server Codex reported starting on each run's
  thread, which must be the lead's tool on the lead's thread and nothing
  else. Here the fake announces what Codex would start (the rehearsal home's
  two servers, a plugin's, the apps server) unless the thread turned it off;
  live, that row is Codex's own word. A server the header reader cannot see
  (one written as keys under a bare `[mcp_servers]` table, which the runner
  refuses, or as an inline table) would show on that row. A managed
  configuration that sets any of this is not read.
- **Code mode, for the children** (read from rust-v0.157.0's source, not
  measured). On a code-mode-only model a child runs its command as
  `tools.exec_command(...)` inside `exec`
  (`code-mode-protocol/src/description.rs:21`), a nested call dispatched
  through the same tool runtime (`core/src/tools/code_mode/mod.rs:330-407`)
  under an id of its own, `exec-<uuid>`
  (`core/src/tools/code_mode/delegate.rs:323`). It is still a
  `commandExecution` item, and any approval is still
  `item/commandExecution/requestApproval`, from the same shell tool and the
  same approval path (`core/src/tools/approvals.rs:707-724`,
  `core/src/session/mod.rs:2801-2887`). The command Codex names is
  `shlex_join` of `[shell, "-lc", cmd]`
  (`app-server/src/bespoke_event_handling.rs:748`;
  `core/src/tools/handlers/unified_exec.rs:99-124`, `core/src/shell.rs:22-31`),
  so `/bin/zsh -lc 'sleep 30 && wc -l alpha.md'`, a login shell unless the
  model asks for none (`core/src/config/mod.rs:3798`; `-c` then, which the
  relay leaves to the owner); its cwd is the thread's, or the model's
  `workdir` under it (`core/src/tools/handlers/unified_exec/exec_command.rs:196-203`),
  so PIO's placement applies as before. The item starts before the request
  (`bespoke_event_handling.rs:754-770`), and `reason` is omitted when absent.
  The fake now sends exactly that for the children, and the relay's exact
  match and the host's placement hold on it (the rehearsal's `beta`). But
  under `on-request` in a `workspace-write` sandbox 0.157.0 **does not ask**
  about a command that is not flagged dangerous and does not ask to leave
  the sandbox (`core/src/exec_policy.rs:820-838`, reached from
  `core/src/unified_exec/process_manager.rs:1477-1495`): M2's command
  approvals were under `untrusted` (`scripts/codex_live_run.py`, R5 and R6).
  So live, most likely nothing reaches the desk, and the desk row is
  **inconclusive, not failed**. `exec` yields to the model after 30 s by
  default (`core/src/config/mod.rs:1144`), and `exec_command` itself after
  10 s, at most 30 s (`core/src/tools/handlers/unified_exec.rs:62-64`,
  `core/src/unified_exec/mod.rs:77`, `:218-224`): `alpha`'s 30-second command
  outlasts both, so live it takes at least one more model step (`wait`, or a
  poll) than the fake plays, which is not modelled here.
- **Command approvals.** The fake asks about `beta`'s command because its
  scenario says to, in the code-mode shape above, named `/bin/zsh -lc '…'`
  as M2 R5 measured and 0.157.0's source builds it. Live, `on-request` in a
  `workspace-write` sandbox does not ask (above). If nobody asks, the desk
  row is inconclusive, not passed. Each command approval carries
  its placement (the command's working directory, followed through any
  symlinks and classified against the workspace; a cwd through links that
  lead out of it reads `outside_fixture`), a network flag and Codex's
  reason. One with no cwd, or asking for the network, is left to the owner.
  The relay answers only the exact command, inside the fixture, with no
  network ask. Live answers come from answer files (the relay or the
  owner). Here the rehearsal answered itself (`desk_answered_by`).
- **File-change approvals.** A file change carries no working directory, so
  it has no placement unless it asks for writes under a root
  (`grantRoot`): then `grant_root_requested` is true and the root is
  classified by label and digest. The owner decides any other file change
  with no placement. The relay leaves every file change to the owner.
- **An approval pending when its turn ends.** If a stop or a deadline ends
  a turn while one of its approvals is still pending, the approval stays
  pending, and a late answer (the relay's or the owner's) is accepted and
  never sent (deferred; see STATE.md). The desk row catches it: no
  `control_applied`, so nothing was sent, and the row fails.
- **Model, provider, reviewer, tool launch.** The fake answers whatever its
  scenario names. Live, these are Codex's own answers, and the lead tool is
  launched with the owner's `config.toml` present, which the lead-tool probe
  never had.
- **The steer.** The fake acknowledges `turn/steer`. M2 R4 saw real Codex
  acknowledge a steer. Live will show whether 0.157.0 acknowledges one sent
  under the lead's grant.
- **The work, and what the runs say.** A real model decides whether to use
  the tool. The children run `sleep N && wc -l` for real. Here `alpha`'s
  30 s is the fake's delay and the counts are the fake's. As measured on this
  model (M2 R5, R6), each fake child says what it is about to run before its
  command (``Running `sleep 30 && wc -l alpha.md`.``), then answers; the
  count row reads each child's last message completed **after its last
  command**, and none if it never answered (`first-number-of-all` shows
  why). The fake lead's answer arrives one message per file, which is not
  measured. Live, what a preamble says is the model's.
- **Usage, stops and the charge.** Here usage is the fake's 4,096 per step,
  reported as Codex was measured reporting it: after a step's tool; and,
  for a step an interrupt cuts short in its tool phase, after the interrupt
  (M2 R1, R3). A step cut short while the model is still producing it is
  not reported, since 0.157.0 records usage only on a completed response;
  that timing was not measured live. A model step takes 1.5 s in the fake;
  live steps take seconds. Measured against the fake, a query to the service
  takes a median 167 ms, a stop about 1.5 s to reach a run, and the meter's
  passes and seconds are in each receipt (`metering`). The lead's tool waits
  for a meter pass that began 1.5 s after its call arrived
  (`lead_tool.py --selftest`). If the meter thread itself fails, it holds
  the lead's tool and cancels every run still going on a connection of its
  own, at once (`meter-dies`: all three cancelled within two seconds); the
  runner's own third start runs on a thread of its own, so the watch loop
  never sits a minute in the tool's wait (review of L3, round 3,
  SPEND-5). No run is stopped in this rehearsal, so the
  row "Every stop the runner made reached Codex" is **inconclusive**;
  `child-overspends` must hold it and `ceiling-cancel-never-sent` fails it.
  A run whose turn was cut short, by the runner's stop or by an interrupt
  the host sent (its own deadline, a cancel), is charged what it reported
  plus a step in flight on each of its threads (30,000, or its largest
  step, if larger), not capped (`stopped-past-share`, and
  `stopped-charge-capped` for the cap put back; `deadline-interrupted`, and
  `interrupted-charged-reported` for a charge that reads the runner's stops
  alone); one not seen exited, or stopped for silence, its share or that,
  if more; one with no usage, its share; a submit that made no execution,
  nothing. What a run was seen to spend, its largest step, its threads and
  whether its turn was cut short are read for the charge from its own host
  events afresh, as the floor reads them, not only from the meter's fold,
  which stops when the runner halts the meter: a step the host reports
  during the cancels on the way out is charged (review of L3, round 4,
  SPEND-11; `late-step-after-halt`, and `stale-meter-charged` for the fold
  alone, which fails the floor row). `lead_run.py --charge-selftest` checks the corners no play
  reaches. Two rows judge that: the charge covers what each run could have
  spent (a floor worked out from the views, the meters and the host's own
  usage events, not from the charge), and each charge stays within its
  reserved share. A Codex that ignored an
  interrupt (`stop-ignored`) fails the share row and the stop row. Whether
  Codex bills an aborted step beyond what it reports is not measured. A run
  active for 150 s with no usage is stopped (`usage-suppressed`,
  `asked-silent`); every M2 step reported usage, so live this should not
  fire, but three reads of runs still running in one lead step, each
  waiting its full 55 s, would come to about 170 s and stop the lead falsely
  (not likely with this brief, and not measured). Live, usage is charged to
  the Codex ledger (`M4-lead-codex`, counted in the Codex cap).
- **The lead's read.** `read_run` waits up to 55 s for its run. Codex's own
  timeout for an MCP tool call is 300 s at 0.157.0
  (`codex-rs/codex-mcp/src/rmcp_client.rs:104`, `DEFAULT_TOOL_TIMEOUT`,
  applied in `connection_manager.rs:336-339` when a server sets no
  `tool_timeout_sec`; PIO sets none). The lead tool's longest path, a read
  (55 s), the meter wait (30 s) and a hold (30 s), is 115 s, which fits: a
  timeout would hand the lead an error and start a step.
- **Codex's memory pipeline.** The owner's configuration has `[features]
  memories = true`. At rust-v0.157.0 the app-server then starts a
  background pipeline when a root thread's first turn starts with input
  (`app-server/src/request_processors/turn_processor.rs`, calling
  `codex_memories_write::start_memories_startup_task`), unless the session
  is ephemeral or a sub-agent's: Phase 1 sends up to two of the owner's
  recent idle sessions (idle at least six hours, at most ten days old) to a
  model and stores what it extracts in `~/.codex/memories_1.sqlite`; Phase 2
  syncs `~/.codex/memories/` (a git baseline, `raw_memories.md`,
  `rollout_summaries/`, `phase2_workspace_diff.md`) and runs a consolidation
  agent that edits `MEMORY.md`, `memory_summary.md` and `skills/`. Each L3
  run is a root thread in its own app-server, so each could start it. **None
  of it is in `config.toml`**, so the configuration row never saw it; the
  agent is started without a connection, so the host never sees it; and its
  model calls are outside every meter and the bound. The runner now lists
  those paths before the service starts and after every run is over (by a
  name Codex chose or a digest of any other, size and time, never content),
  and the row "Codex's memory pipeline wrote nothing during the run" fails
  if a path was added or removed or a size changed (`memory-pipeline-ran`,
  where the fake writes what the pipeline would; `memory-db-grew`). A later
  modification time alone, size unchanged, is **inconclusive**, not a
  failure (`memory-mtime-only`): at rust-v0.157.0 every app-server start
  opens the memories database read-write, in WAL mode, and runs its
  migrations, whatever features are on (`state/src/runtime.rs:172-185`;
  `state/src/sqlite.rs:251-293`, the pool at `:296-310`). L3's first live run
  failed this row on exactly that: `memories_1.sqlite` kept its size and
  moved its time, as `goals_1.sqlite` did in the same second with goals off. In-turn memory tools (an ad-hoc note under
  `memories/extensions/ad_hoc/notes/`) are offered only with `[memories]
  dedicated_tools = true`, which is off by default. With memories off for
  L3's threads, as the runner now requires (above), the pipeline does not
  start there, and the row stays behind that as a witness. What the
  pipeline would cost is not measured.
- **Placement is a snapshot.** Where a command approval's cwd lands is
  decided when the request arrives; the command runs only after the desk
  or the relay answers. A link changed in between changes where it runs
  without PIO seeing it (review of L3, round 3, R3-HC-8; deferred, see
  STATE.md). The children could make such a link without asking; nothing
  in this rehearsal does.
- **Children under other names.** The lead's tool starts only the plan's
  children (`PIO_LEAD_CHILDREN`), so a lead that names another is refused
  before anything reaches the service (`child-renamed`). Behind that, the
  runner meters, stops, cancels (on its way out and from the watchdog),
  relays at the desk and charges every execution under `L3.` the stream
  shows, whatever its name (`child-renamed-unchecked`; review of L3, round
  3, SPEND-1). Before this, a renamed child ran unmetered and was charged
  nothing while every spend row held.
- **The runner's probes.** The third start, the grant's attempt to attach
  the tool and the credential check must be refused; one that was admitted
  instead is cancelled with the rest on the way out and charged on a line of
  its own, through the same branches as any run and never below its share:
  at least its share, and at least what it was seen to spend plus a step in
  flight. The floor and share rows cover it (`third-admitted`; review of
  L3, round 4, SPEND-12: `probe-over-share`, whose admitted third run takes
  a 150,000-token step, fails the share row with the floor holding, and
  `probe-charged-flat`, the old flat share, fails the floor row).
- **The configuration snapshot.** Here the diff is of the fake's own Codex
  home inside the tree. The lead's thread shows `existed_before: false` and
  one fixture trust entry added. Live, it is the owner's
  `~/.codex/config.toml`, before and after each run. The ChatGPT desktop app
  runs its own Codex app-server against the same `~/.codex` (seen during the
  review: `/Applications/ChatGPT.app/Contents/Resources/codex … app-server`,
  with plugin hosts under `~/.codex/plugins`). A write by it during L3 would
  appear in PIO's diff: the configuration row **fails** rather than falsely
  holds, but it would read as PIO's run's change. Quit the app before the
  live run, or record that it was running. No store is read on Codex in
  either mode.
- **The live tree.** Here the tree is under `/tmp` and removed. The
  live-tree row holds through a probe made the same way under
  `~/pio-m4-live` and then removed. Live, the tree itself is checked before
  anything starts, and at the end its store is released and the tree kept.
