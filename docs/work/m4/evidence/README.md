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

## `L3-rehearsal.json`

L3 rehearsed against the labeled Codex fake by `scripts/lead_run.py --rehearse
--plan L3` at clean head `baa4e50` (`dirty: false`), after the fixes of the
review of L3. **Zero tokens, no model call.** The fake is
`pio codex fake-app-server` (`pio-fake-app-server`): it speaks 0.157.0's
app-server shapes and runs no model, tool or command, and `serve-codex` does
not qualify a labeled fake. Plan: owner approval of 2026-09-24 (sequence cap
400,000, stop 320,000) and the decisions of 2026-09-25 (issue #12).

**37 rows and two records: 35 rows hold, none failed, and two are
inconclusive**, the two this rehearsal cannot observe:

- **Codex qualified at the pinned identity**: the labeled fake has no
  qualification record. Live, a missing or different record fails it; the
  mutants `qualified-elsewhere` (fails) and `qualified-as-committed` (holds)
  exercise it on a planted record.
- **Every stop the runner made reached Codex**: no run is stopped here. The
  mutant `child-overspends` must hold it (alpha's interrupt sent,
  acknowledged, its turn interrupted) and `ceiling-cancel-never-sent` fails
  it.

What L3 shows, as the fake plays it:

- The lead tool is on the lead's thread only. The host read back from its
  own `thread/start` request exactly `start_run` and `read_run` at
  `approval_mode: approve` and no server-wide default; the children's
  threads carried no server. Nothing was asked about the lead's tool, by
  any path, and PIO declined nothing by itself (record: empty for all
  three runs).
- Each thread's model and provider (`gpt-5.6-terra`, `openai`) were checked
  from `thread/start`'s answer before its first turn. `approvalsReviewer` is
  `user` on all three.
- The lead's steer on `L3.alpha` was recorded under its grant while the turn
  was `active` and `acknowledged`, and acknowledged with `provider_ack_id`.
  Behavior is `not_observed`.
- The lead's `read_run` of `alpha` took 27.1 s and returned `exited`.
- Each child said what it was about to run before its command, as measured
  on this model (``Running `sleep 30 && wc -l alpha.md`.``), then answered;
  the count row read each child's last message: 7 and 4. The lead's answer
  came one message per file and was read as two lines.
- `beta`'s command approval came to the desk with its placement
  (`inside_fixture`, `<fixture>/`), no network ask and Codex's reason, was
  answered `allow` by the rehearsal, and was sent as
  `{"decision": "accept"}`.
- Codex's runs were metered on the event stream by a thread of their own:
  279 passes in 46.3 s. No stop and no silence.
- The charge is Codex's reported totals, 20,480 + 8,192 + 8,192 = 36,864,
  on the rehearsal's own ledger, each within its share. The reservations,
  written immediately before the lead's submit (155,000 + 110,000 +
  110,000 = 375,000), were replaced and none is left.

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
- **Codex features.** Before anything is reserved the runner reads two keys
  of the Codex configuration's `[features]`, `exec_permission_approvals` and
  `request_permissions_tool`, and refuses to start if either is on: PIO does
  not opt into Codex's experimental API, so Codex strips a command
  approval's extra permissions from what the desk sees, and an allow would
  grant them unseen (`experimental-feature-on`). A feature set by a profile,
  a managed configuration or a command-line override is not read.
- **The sizing.** Codex reports a step once its tool has finished (M2 R5,
  R6), so a report that crosses a ceiling arrives with the next step begun.
  From the code, each run can reach its ceiling plus two steps: a child
  50,000 + 2 x 30,000 = 110,000, and the lead 125,000 + 2 x 30,000 =
  185,000. The lead's tool withholds any response (a result, an error or an
  unknown tool) once the lead has reported more than 95,000, and the meter
  stops the lead when a result its tool never saw comes back past 95,000;
  that keeps a lead that uses only its tool within 155,000 (`lead-heavy`,
  `lead-tool-error-past-hold`). But a result the tool never sees (Codex's
  own shell, another MCP server, a refused approval) is back, with the next
  step begun, before anything can see it, when the lead may be just under
  125,000 (review of L3, round 2, SB-1; `lead-shell-past-hold`). The worst
  case is **405,000** (185,000 + 2 x 110,000), past the 320,000 stop, the
  400,000 cap and, with Codex's 421,450 used, the 800,000 Codex stop, so
  **the live runner refuses to start L3** until the owner decides the
  sizing. The limits are the owner's and are unchanged.
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
  owner's own MCP servers and plugins (four servers and fourteen plugins in
  `~/.codex/config.toml`) are loaded on every thread and could ask
  something; each such ask fails the desk row, by design.
- **Command approvals.** The fake asks about `beta`'s command because its
  scenario says to, and names it `/bin/zsh -lc '…'` as M2 R5 measured, with
  `reason` null as both measured approvals (R5, R6) had it. Live,
  `on-request` in a `workspace-write` sandbox need not ask. If nobody asks,
  the desk row is inconclusive, not passed. Each command approval carries
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
  (`lead_tool.py --selftest`). No run is stopped in this rehearsal, so the
  row "Every stop the runner made reached Codex" is **inconclusive**;
  `child-overspends` must hold it and `ceiling-cancel-never-sent` fails it.
  A run the runner stops is charged what it reported plus one step, not
  capped; one not seen exited, its share or what it reported or its meter
  saw plus a step, if more; one stopped for silence or with no usage, its
  share or what it reported, if more; a submit that made no execution,
  nothing. Two rows judge that: the charge covers what each run could have
  spent (a floor worked out from the views and meters, not from the charge),
  and each charge stays within its reserved share. A Codex that ignored an
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
- **The runner's probes.** The third start, the grant's attempt to attach
  the tool and the credential check must be refused; one that was admitted
  instead is cancelled and charged its share on a line of its own
  (`third-admitted`).
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
