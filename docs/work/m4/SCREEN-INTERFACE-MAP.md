# M4 step 1 — the screen-to-interface map

Every element of every accepted screen, and the **public operation** that feeds it. Where none exists, it is marked **GAP** and numbered.

Design input: [`design-input/M4-DESIGN-INPUT.md`](design-input/M4-DESIGN-INPUT.md), accepted by the owner 2026-09-21, with its [corrections](design-input/CORRECTIONS.md). Rule 1 of that document is the constraint this map exists to test: *the screen reads and writes only through the public API the command line uses.*

**Steps 2 and 3 merged as `d0fd88f`** ([PR #15](https://github.com/Combraton/pio/pull/15), 2026-09-23), from reviewed head `f915137`; main's tree is identical to that head's. The L1 lead run is built on its own branch from there.

**Revised 2026-09-21** after the reviewer's read. Four corrections, and three of them changed conclusions rather than wording. What the first draft got wrong is recorded at the end rather than quietly fixed.

## The rule that governs every answer below

**`execution.inspect`'s result is `unevaluatedProperties: false`, and so is every entry inside it.** Nothing may be promoted into the view that the pinned schema does not already define. That single fact decides most of this document, and the first draft was written without it.

So a field PIO records and a screen needs travels by exactly one of two routes:

1. **A Protocol event payload.** `event.payload` is `{"type": "object"}` with no `unevaluatedProperties`, so a payload may carry fields the schema does not name. This is the route, and it is already how the hosts record everything.
2. **A namespaced PIO profile or feature**, advertised through `core.capabilities` and `core.negotiate`, under its own name.

**Never a new name under `execution.*` or `core.*`.** Conformance is **206 pass / 73 unsupported / 1 skipped**, plus 2 supplemental, and it stays there.

## What the interface has today

Read: `core.describe`, `core.negotiate`, `core.capabilities`, `core.events.read` / `.subscribe` / `.unsubscribe`, `core.effects.get`, `core.grant.get`, `execution.discovery.list`, `execution.inspect`, `execution.output.read`, `execution.reconcile`.

Write: `execution.submit`, `execution.cancel`, `execution.steer`, `execution.respond_action`, `execution.controller.claim`, `execution.workspace.checkpoint`, `core.grant.issue`, `core.grant.revoke`, `core.effects.abort_obligation`.

**The `execution.inspect` view, in full, from the schema** — not from what PIO happens to populate, which is what the first draft listed:

`actions`, `admission`, `alternative`, `cancellation`, `completions`, `context`, `continuation`, `correlation`, `deliveries`, `delivery`, `effects`, `evaluation`, `execution`, `exit`, `finalized_by`, `host`, `next_cursor`, `obligations`, `origin`, `predecessor`, `queue_reason`, `reason`, `recovery`, `result`, `revision`, `runtime`, `runtime_detail`, `scheduling`, `steering`, `usage`, `workspace`.

Four of those matter here and were missing from the first draft:

- **`actions[]`** — `action_entry`: `action_id`, `owner`, `state` (`pending` or `answered`), `requested_at`, and optionally `answered_at` and `response_effect`. Closed.
- **`steering[]`** — `steering_entry`: `steer_id`, `request`, `recorded_at`, `delivery`, `proof_class`, `evidence`, `behavior`. Closed, and **carries no author and no grant**.
- **`origin`** — `{initiator, depth, call_budget}`, required and closed. `execution.submit` takes it as `payload.origin`. **PIO does not implement it.**
- **`scheduling`** — present and unused by PIO.

## The gaps, revised

| # | Gap | Route | Screens |
| --- | --- | --- | --- |
| **G1** | Draw a board of five or six runs. | **Closed.** The Protocol-native fold works; no new operation. Proven in `scripts/board_fold.py`. | 1, 2, 3, 7 |
| **G2** | The approval walk needs, per pending request: the **deadline**, the **option list the harness offered**, what PIO will send, and the classification. `actions[]` gives identity, owner, state and `requested_at` — and is closed, so the rest cannot go there. **And a lapsed deadline reaches the stream nowhere today.** | `execution.runtime.changed` payload, key `pio.combraton.dev/approval`; the answer on `execution.action.answered`, key `pio.combraton.dev/decision`. | 1, 4, 7 |
| **G3** | Transcript blocks **as they happen**, each with where a tool use landed and who decided. | Blocks from `execution.output.read`, per harness. The audit on `execution.exit.observed`, key `pio.combraton.dev/tool-uses`. Live placement is the *absence* of a record. | 1, 5 |
| **G4** | Mid-turn messages for Claude Code. `execution.steer` exists and Codex takes one; the Claude host has no steer control. The thread shows **queued**, never delivered, until it does. | PIO host work. | 3, 5 |
| **G5** | Per-message usage for Claude Code. Usage arrives once, at turn end. | PIO host work. | 1, 2, 5 |
| **G6** | Incremental changed-key commits. | **Closed** by the same fold: settled runs are never re-read. | 1, 2 |
| **G7** | Projects are not modelled anywhere. | Client-side configuration. Belongs nowhere near the Protocol. | 1 |
| **G8** | **`origin` is defined and PIO does not implement it.** | **PIO gap. No Protocol issue.** | 3 |

**Not a gap:** *events since a cursor, for "while you were away"*. `core.events.read` takes a cursor and returns `next_cursor`. Screen 7 needs a client that keeps the cursor across a detach.

**The one real Protocol gap** is narrower than the first draft claimed: `steering_entry` and the event record are **closed and carry no author or grant**, so a steering note cannot record which run wrote it. A proposal is filed for **that alone**, with demonstrating evidence. Everything else here is PIO's own work.

## G1 and G6 — the Protocol-native fold, tried first

Before proposing any new operation:

1. `core.events.read {from: "start", kinds: ["execution"]}` enumerates **every execution subject the caller may see**, because every execution's first event is visible to a grant that can read it. That is the board's roster, with no new surface.
2. A retention gap returns a **snapshot of subjects with revision and state**, so a client attaching late is not left with a hole.
3. `core.events.subscribe` delivers the changes.
4. `execution.inspect` is then called **only for subjects whose revision moved** — which is also **G6**: the board stops re-reading whole state on every refresh, without any new operation.

**Proven**, in `scripts/board_fold.py`, run in CI. A second caller attaches after six runs already exist, holding a grant whose rights are `core.events.read` and `execution.read` and nothing else. It discovers all six **without being told a single id**, then inspects **one** subject per changed subject in each later round; the six settled runs are never read again, which is G6. Its `execution.submit` is refused, so the grant is a boundary and not a label. The mutant — a board that re-inspects everything every tick — draws the same screen and fails on `['run-1', …, 'run-7'] != ['run-7']`.

Two things the proof established that the schemas do not say out loud:

- **`core.grants` must be negotiated to *present* a grant**, not only to issue one. A session that has not negotiated it is refused `invalid_envelope` at `/grant`, before authorization is reached.
- **A retention gap hands back `snapshot.subjects[]`** with `{subject, revision, state}`, exactly as the reviewer said — a roster, not a hole.

**G1 and G6 need no new operation.** That is now a measurement rather than a plan.

## The carrier, settled before any G2 or G3 code

Three facts, checked in the source, that decide everything below.

1. **Host `life.event` records go to the private `*.events.jsonl`, not to `core.events.read`.** `permission_options_observed`, `action_requested`, `request_declined_by_pio` and `request_denied_by_default` are host records. No caller can read them.
2. **On the Protocol stream, a pending action is only `execution.runtime.changed`**, with payload `{runtime: "requires_action", action_id, owner}`. The projection turns `action_requested` into a closed `action_entry` in `view.actions[]` — `action_id`, `owner`, `state`, `requested_at` — and emits that one event. The deadline, the offered options, the classification and the decider reach the stream **nowhere**.
3. **`request_denied_by_default` is not projected at all.** Zero occurrences in the projection. When nobody answers and PIO denies on the caller's behalf, the Protocol stream learns nothing and `view.actions[]` shows the action **`pending` for ever**. A caller cannot tell a lapsed request from a waiting one. That is a defect, not a gap.
4. **`request_declined_by_pio` reaches only the private adapter namespace.** It is folded into `e[<adapter>]["declined_by_pio"]` and never becomes an action. So PIO's own decline was as invisible to a caller as the lapse. Screen 7 promises "anything PIO decided, marked as PIO's decision", and **neither of PIO's two decisions reached it**.

Both are fixed, and both were measured rather than argued: `scripts/approval_desk.py --baseline-as-mutant` runs the same probes against the binary built at `a22c98c` and requires them to fail there. At that commit a lapsed action's state, read after the run has exited, is `"pending"`.

### The decisions

**No new event type name is introduced under `execution.*` or `core.*`.** Every field rides in the **payload** of an event that already fires, under a **namespaced key**, because `event.payload` is `{"type": "object"}` with no `unevaluatedProperties` — that is the schema rule that allows it. Keys follow the Protocol's own `extension_key` shape, `<domain>/<name>`, so a future Protocol field cannot collide with one of ours.

| What | Rides on | Key |
| --- | --- | --- |
| Deadline, offered options with their kinds, the option kind PIO will send, the classification and where it lands | `execution.runtime.changed`, which already fires at the moment the action becomes pending | `pio.combraton.dev/approval` |
| Who decided, and the option actually sent | `execution.action.answered`, which already fires when a caller answers | `pio.combraton.dev/decision` |
| **A lapsed deadline** | `execution.action.answered` **must also fire for PIO's own default deny**, with `decided_by: "pio"`, and set the action to `answered` | `pio.combraton.dev/decision` |
| **A decline PIO made before any caller was asked** | the same event. A decline is an action PIO answered at once: it enters `actions[]` already `answered`, so the walk and the "while you were away" list read one shape and not two | `pio.combraton.dev/decision`, with `basis: "out_of_scope"` and the classification |
| The end-of-turn tool-use audit: placement and decider per tool use | `execution.exit.observed`, which already fires at turn end | `pio.combraton.dev/tool-uses` |

### The schema constraint on PIO's own decisions

An `execution.action.answered` emitted for a decision **PIO** made has no command behind it. The event schema's `if/then` makes the mix impossible: `origin: "command"` *requires* `operation_ref` and `command_id`, and anything else *forbids* both. So these two emits pass `None` where the caller's answer passes `Some((p, op))`, and come out `origin: "provider"` with neither field. `scripts/approval_desk.py` reads `$defs.event` out of the vendored schema and checks that rule on every event it sees, so a future emit that gets it backwards fails the run rather than the validator.

**A client that ignores the key still works.** Every existing field of those payloads is unchanged, and a reader that knows only `runtime`, `action_id` and `owner` behaves exactly as it does today. The keys are additive in an already-open object. **Conformance stays at 206** — re-run, not assumed.

Proven by diff, not by inspection. `approval_desk.py` builds the binary at `a22c98c` in its own worktree and runs the same scenarios through both:

- **A run with no PIO decision**: with the namespaced keys removed, the two streams are the **same stream** — 8 events, equal field for field once instants, uuids, SHAs and the temporary root are normalized.
- **A run where PIO decided**: every event the old stream carried is still there, unchanged and in the same order, and the single addition is the `execution.action.answered` whose stripped payload is `{"action_id": …}` and nothing else. Sequence and revision necessarily advance, because a new event on a stream advances the stream; that is why the diff is on type, subject, origin and payload rather than on counters.

`--mutant unstripped` runs the same comparison without removing the keys and must fail, so "the only difference is the keys" is a claim the run can lose.

### What a caller holding only `core.events.read` on a run may see of another run

**Nothing.** The stream filters by subject against the grant's resources, and the approval fields ride in the payload of an event **on that run's own subject** — so the existing subject filter covers them with no extra rule. Proven, not assumed: a grant scoped with `id_prefix: run-1` sees only `run-1`'s events, the result carries `filtered: true` so the caller knows something was withheld rather than absent, and `execution.inspect` on `run-2` is refused `permission_denied` / `out_of_scope`. That is the exact shape the M4b lead's grant will have.

## G2 — where each field travels

`actions[]` gives the walk `action_id`, `owner`, `state` and `requested_at`. It is closed, so everything else travels in the **event payload** that already carries it:

| Field the screen needs | Where it is today | Route |
| --- | --- | --- |
| Deadline before PIO denies | the host's `action_requested` / `request_denied_by_default` events; the value is the caller's own `timeouts.delivery` | event payload; the screen computes time left from `requested_at` + the deadline |
| The option list the harness offered | `permission_options_observed`, measured live in M3b: `once` / `always` / `reject` | event payload |
| What PIO will send | the host chooses by option **kind** | event payload, beside the list |
| "Always allow" visible and unavailable | derived from the list | no new field |
| The classification and where it lands | `request_declined_by_pio.classification` | event payload |
| Who decided, afterwards | `control_applied.decided_by`, `request_denied_by_default.decided_by` | event payload |

The negative control — an answer aimed at the wrong run or a stale controller — is **already fenced** by `execution.respond_action` on subject and revision.

### G2 is proven, and where

`scripts/approval_desk.py`, through **`serve-opencode`** with the labeled ACP fake behind it — a real host process, a real journal, the real projection, the public Unix API in front. `serve-fake` has no approvals, so it cannot prove this. Everything is read from `core.events.read` and `execution.inspect`; the host's private events file is never opened.

| What | How it is shown | Its mutant |
| --- | --- | --- |
| Two requests pending across two runs, ordered by deadline | run 1 is submitted first and due last (300s); run 2 is submitted second and due first (75s), so arrival order and deadline order disagree and the walk has to use the deadline — which is only on the stream because G2 put it there | `--mutant arrival-order` walks them as they arrived and fails on `['run-1', 'run-2']` |
| A caller answers through `execution.respond_action` | the answer reaches the stream as `execution.action.answered`, `origin: command`, `decided_by: "caller"` | the baseline probe: no `pio.combraton.dev/decision` at `a22c98c` |
| An answer aimed at the wrong run is refused | run 2's `action_id` sent against run 1's subject is refused `not_found`, and both runs are still `requires_action` afterwards | `--mutant cross-run` sends the same envelope at its **own** run, which succeeds — so the refusal is attributable to the run mismatch and not to a malformed envelope |
| A lapsed deadline is PIO's decision | `origin: provider`, no `operation_ref`, no `command_id`, `decided_by: "pio"`, `basis: "deadline_lapsed"`, `after_seconds: 75`, `option_kind: "reject_once"`; the action reads `answered` with an `answered_at`; the walk, redrawn from the same fold, is empty | `--baseline-as-mutant`: at `a22c98c` the same run ends with **no** answered event and the action still `"pending"` |
| A decline is PIO's decision | the run never enters `requires_action` at all; one action, already `answered`, and `basis: "out_of_scope"` with the classification on the event | `--baseline-as-mutant`: at `a22c98c` there is no answered event and no action |

### One open item, reported rather than closed

**A run waiting on an approval is sometimes ended by a PIO timeout before its answer clock lapses.** The signature is exact: `execution.timeout.passed` on that run, **no** `execution.usage.observed` (so the harness was stopped, not finished), and the action still `pending`. Seen **three times in twenty-eight runs**, always while other heavy work was running, and **not once since** — several dozen runs now, including a batch with the machine deliberately saturated by CPU burners.

**What has changed since it was first recorded.** The run now records the clock **by name**: `execution.timeout.passed` carries `{"timeout": <name>}`, and which of the five it is decides whether this is the delivery deadline sharing its number with the answer deadline or something else entirely. Guessing cost three rounds, so the name is read rather than inferred. And the acknowledged-delivery wait that `pass_one` makes is now made by the lapse probe too — it was the one path that could hit the delivery timeout while measuring the answer timeout, and it had no such wait.

I still have not seen the name, because the failure has not recurred. **The item stays open with a measurement attached rather than a conclusion**: I will not name a clock I have not read.

What is known:

- **It is not a regression of this fix.** At `a22c98c` the same run also ends with the action `pending`; that is the defect this section fixes. This failure is the *absence* of a lapse to observe, not a lapse that went unreported.
- **`action_answer_timeout_seconds` *is* `timeouts.delivery`.** A run's answer deadline and its delivery deadline are the same number. While delivery is still `pending` the delivery timeout is live — which is why the pass now asserts **acknowledged delivery before drawing the walk**. That assertion was already in place when the third failure happened, so it is not the whole story.
- **The screen is correct under this failure anyway.** A run that has exited cannot answer anything, so **the walk drops approvals whose run has exited** — a rule that is exercised on real data, including at `a22c98c` where it is the *only* thing that empties the walk.

The pass now tells the two apart: if `execution.timeout.passed` is present for the lapse run it fails with that named, and dumps the service's own record for the execution — the timeouts, `admitted_at`, `timeouts_passed` and the delivery state — which is what will identify the clock the next time it happens.

### The same walk, through the other two release harnesses

`approval_desk.py --harness claude` and `--harness codex` run it through
`serve-claude` and `serve-codex`. What each harness **does not** do is the point.

| | Codex | Claude Code | OpenCode |
| --- | --- | --- | --- |
| Answer deadline | the caller's own `timeouts.delivery` (since 2026-09-25; before, none) | the caller's own `timeouts.delivery` | the same |
| If nobody answers | a single `decline` of PIO's (since 2026-09-25; before, nothing) | a single-use `deny` | a `reject_once` chosen by kind |
| Option list | none | none | `once` / `always` / `reject`, measured live |
| Also carries | `method`, `approval_kind` | `suggestions_offered`, `widening_fields_sent` | the offered list and what PIO will send |

**Codex now lapses like the other two** (owner decision, 2026-09-25, for L3: "if I don't answer, it lapses"). Until then a Codex approval waited until it was answered: left alone for 45 seconds it was still `pending`, so the walk showed no countdown, and `--mutant assume-countdown` failed on inventing one. Now the host carries the caller's delivery timeout as the deadline and, when it passes, sends one `decline` recorded as PIO's (`request_denied_by_default`), so the walk shows a countdown to a decision PIO will make. The mutant and the 45-second wait are retired, because no release harness is without a deadline now. The Codex matrix's `mcp_approval_lapses` proves the lapse.

**Claude Code offers a rule update with every request.** Acting on one widens a permission beyond the request. What PIO will send is built by `permission_decision`, which cannot encode one, so the walk shows `suggestions_offered: 1` and `widening_fields_sent: []` — and the fake's own marker confirms `widening_fields_received: []`, which is the harness's record rather than PIO's.

### Two actions on one run

The default-deny arm resolves an action from `action_seq`. With one action that number is always 1, so nothing separated *settles the right row* from *settles the only row*. A turn that asks about two things does: both lapse, and the run ends with `run-1.action-1` and `run-1.action-2` each `answered`, each with its own `answered_at`, and two decisions rather than one twice.

This check has **no baseline mutant**, deliberately: the fake at `a22c98c` has no `permission_requests` knob, so the scenario would produce no actions and the probe would fail because the fixture is newer than the binary. A mutant that dies for the wrong reason proves nothing.

The classification the walk shows for this harness is `disposition: surface_as_action`, `placement: not_classifiable` — an `execute` call announced with a command line and no path, which the resolver will not place. The screen shows that, rather than guessing `inside`. Same rule as G3.

## G3 — blocks as they happen, not an end-of-turn audit

**Carrier decision.** The blocks themselves need no new carrier: `execution.output.read` already returns what the host spooled, newline-delimited — one record per `session/update` for OpenCode, per stream message for Claude Code, per item event for Codex. The screen needs a per-harness reader, which the table below names.

Placement and decider are different. **No Protocol event fires per tool use**, and for a tool use nobody was asked about — R2 and R3 of the MiniMax sequence, where the harness simply acted — there is no Protocol event at all. So:

- **Live, before the audit exists**, the screen shows `not yet classified` and `unknown`. That is the *absence* of a record, so nothing has to travel for it.
- **The end-of-turn audit** rides on `execution.exit.observed`'s open payload under `pio.combraton.dev/tool-uses`. No new event type name, and a client that ignores the key sees today's behaviour. The harness's own per-call status travels beside it on purpose, so a reader can see the two **disagree** rather than being handed one of them.

### G3 is proven, and where

`scripts/transcript_blocks.py`, through the same real host path.

| What | How it is shown | Its mutant |
| --- | --- | --- |
| Blocks arrive **while the run is still working** | the screen reads `execution.output.read` from its own offset and gets bytes in more than one read with the run still at `requires_action` — measured, not assumed | — |
| A live reader never re-reads what it has seen | every read starts exactly where the last one ended | `--mutant whole-spool` reads from offset 0 every tick and fails that |
| Live, a tool use is **not placed and nobody is named** | every call on the screen reads `not yet classified` / `unknown`, and nothing on the stream places it either — the absence is asserted, not assumed | — |
| A screen may say it does not know, but anything it **does** say must survive the audit | the audit is compared against what the screen claimed live, not merged over it | `--mutant guess-inside` calls a call `inside` because its command names a workspace path; the audit says `not_classifiable` and the run fails |
| The audit fills the blanks in place | the ids seen live are a subset of the audited ids, and every one is placed | `--baseline-as-mutant`: at `a22c98c` `execution.exit.observed` carries no audit at all |
| Three deciders told apart | one call the caller answered, two nobody was asked about; `decided_by: null` is shown as **nobody was asked**, never as a decider | — |

### Blocks and the audit, through the other two

`transcript_blocks.py --harness claude` and `--harness codex`. The live half reads the same on all three — bytes arrive while the run is still working, every tool use is `not yet classified` and `unknown`, and the reader never re-reads what it has shown. The decoder is per harness: one record per `session/update` for OpenCode, per assistant message for Claude Code, per item event for Codex.

**Codex produces no audit at all.** Its host emits no `tool_uses` record, so `execution.exit.observed` carries no `pio.combraton.dev/tool-uses` and a Codex run's placements are **never** classified. The screen has to say that. Showing an absent audit as "nothing happened outside" would be a containment claim PIO never made, so the pass asserts the key is absent and that every placement stays `not yet classified`.

### Gap R1 — the Codex tool-use audit

**Owner decision, 2026-09-22: in v0.1, built in M4a.** Numbered here so the screens are designed against a release harness that will have it, and so the shape is settled before anything is written. **Not built now, and not a step-3 blocker.**

| | |
| --- | --- |
| **What is missing** | The Codex host emits no `tool_uses` record, so a Codex run has no end-of-turn audit and no containment record. The other two release harnesses have both. |
| **Source that fills it** | `item/completed` items of type `command_execution` and `file_change`. A command execution carries its command and `cwd`; a file change carries its paths. The **existing resolver** classifies them — the same one the Claude and OpenCode audits use, so placement means the same thing on all three. |
| **Record shape** | Identical to the other two audits: per use, the placement, the target digest, who decided, and the harness's own status beside it. |
| **Carrier** | `execution.exit.observed` under `pio.combraton.dev/tool-uses`, exactly as for Claude Code and OpenCode. No new event type name. |
| **Proof** | `transcript_blocks.py --harness codex` flips from *key absent, everything unclassified* to *key present, every placement classified*. Its mutant drops a target. Plus a Codex case whose command runs with a `cwd` **outside** the fixture, where the audit says `outside_fixture`. |

Until it lands, the screen shows a Codex run's placements as `not yet classified` for the whole run and says why — which is what `transcript_blocks.py --harness codex` asserts today.

Three placements come back from one turn — `inside_fixture`, and `not_classifiable` twice — which is why the screen cannot collapse them into "inside or outside".

**The first draft of this proof passed while proving nothing.** It read the transcript forty times, got **zero bytes every time**, then everything at once after the run had exited — and still asserted that the audit filled in the blanks. A turn from this fake is over in about a second, which is not long enough to watch. The live window now is a **pending approval**: a real pause with real blocks spooled behind it.


The first draft called this "mostly promotion". It is not, and the reason matters.

**`tool_uses` is computed once, at the end of a turn, from the whole transcript.** A run view watched live has no such record and cannot wait for one. So the screen needs two different things:

1. **Blocks as they arrive**, from a per-harness incremental source.
2. **The end-of-turn audit**, which corrects and completes them.

| Harness | Incremental source | What it gives live |
| --- | --- | --- |
| OpenCode | `session/update` — `tool_call` then `tool_call_update`, keyed by `toolCallId` | the call, then its target and final status. **Measured in M3b:** the announcement carries an empty `rawInput`, so the target arrives later |
| Claude Code | the stream's assistant `tool_use` blocks and `tool_result` | the call and its result |
| Codex | the app-server's item events | the item and its status |

**What the view shows before the audit exists is the part that must not be got wrong.** Placement is **`not yet classified`** — never `inside`. An optimistic `inside` is exactly the R6 defect of the Claude sequence in a new place: a claim about containment made before anything checked. The glyph and word for it are the violet uncertain pair the design already defines, and the block is re-rendered when the audit lands.

Who decided is the same: **unknown until a decision is recorded**, never "the user's rules" by default.

## Screen 1 — the board

| Element | Fed by | Status |
| --- | --- | --- |
| Product, service, version | `core.describe` | have |
| Qualified harness versions | `execution.discovery.list` | have |
| Counts of runs by state | the events fold | **G1** |
| Project name and working location | client-side; location from `workspace.lease` | **G7** |
| Run row: glyph and state word | `runtime`, `delivery`, `usage.liability` | have |
| Run row: harness, worktree, age | `admission`, `workspace.lease.base`, submit time | have |
| Preview: name, state, version, location, tokens | `execution.inspect` | have |
| Preview: the truth line | `deliveries[].evidence`, `.proof_class`, `usage.liability`, `recovery` | have |
| Preview: latest blocks | incremental source | **G3** |
| Amber sticky note: approvals waiting | `actions[]` where `state == "pending"`, across the fold | **G1** + **G2** |
| Violet sticky note: uncertain runs | `delivery == "ambiguous"`, `usage.liability == "unresolved"`, `runtime == "unknown"` | have, per run |
| Refresh without re-reading everything | the events fold | **G6** |

## Screen 2 — split view

Screen 1's preview repeated. Same sources, same gaps. **Closing a card never stops the run** means it issues no operation at all — the easiest rule in the document to honour and the easiest to break by accident, so it gets its own headless case.

## Screen 3 — orchestrate (M4b)

| Element | Fed by | Status |
| --- | --- | --- |
| Lead card, goal, what it waits for | `execution.inspect` on the lead | have |
| The runs the lead started, as a team | `origin.initiator` and `origin.depth` | **G8** — defined, not implemented |
| MESSAGES thread: from → to | `steering[]` | **the Protocol gap**: `steering_entry` is closed and has no author |
| Delivery ticks `✓` / `✓✓` / violet `◇` | `steering[].delivery` and `.proof_class` | have. **An unproven message never shows two ticks** falls straight out of `proof_class: null` |
| The lead's budget meter | `execution.submit` → `payload.budget` with `pool`, `amount`, `ceiling` | **implemented.** A `hard` ceiling is refused `enforcement_unavailable` unless the adapter can enforce that measure — PIO will not promise one it cannot keep |
| A run may never answer another run's permission request | `core.grant.issue` omitting `execution.respond_action` | **implemented**; needs a case proving the grant refuses it |

## Screen 4 — the approval walk

Covered by the **G2** table. `execution.respond_action` answers; `actions[]` supplies identity and state; the rest travels by event payload.

## Screen 5 — the run view

Covered by the **G3** table, plus `execution.inspect` for identity, the truth line and usage, `execution.workspace.checkpoint` for the diff, `execution.cancel` for `c`, and the events fold for `[` and `]`.

## Screen 6 — an uncertain run

**Needs no new operation.** `deliveries[].evidence`, `exit`, `result`, `usage.liability`, `recovery` and `obligations` already say what PIO knows, does not know and will not do. *Usage unknown is never shown as zero* holds because there is no zero to show — `usage.observations` is empty and the liability is unresolved.

The screen that needs the least is the one about uncertainty. That is the right way round.

## Screen 7 — leaving and coming back

Runs still working and approvals still waiting are **G1** and **G2**. *While you were away* is `core.events.read` from the stored cursor — **have**.

"Anything PIO decided is in the events already" is what the first draft of this section said, and it was **wrong twice over**: PIO's two decisions, the lapse and the decline, were the two things a caller could not see. They are both there now, both as `execution.action.answered` with `pio.combraton.dev/decision`, so this list is one filter over one event type rather than a special case per decider.

## What the first draft got wrong

Recorded rather than quietly fixed, because the reviewer will want to know which conclusions moved.

1. **It confused two fields with the same name.** PIO's **event** `origin` is `command | provider` and says whether the service or a caller produced the event. The **view's** `origin` is `{initiator, depth, call_budget}` and says who started whom. I read the first, concluded "not implemented", and called it a **Protocol** gap. The first half was right and the second was not: it is defined in the pinned schema and unimplemented in PIO. A PIO task, no Protocol issue.
2. **It did not check `unevaluatedProperties`.** Most of the draft assumed fields could be promoted into the view. They cannot. Every "promotion" answer is now a route — event payload or a namespaced profile.
3. **It listed the view from what PIO populates, not from the schema.** `actions[]`, `steering[]`, `origin` and `scheduling` were all missing, and `actions[]` is most of the approval walk.
4. **It called G3 "mostly promotion".** `tool_uses` is an end-of-turn audit; a live view needs blocks as they arrive, and must say **not yet classified** rather than guess `inside`.

The reviewer's own correction to the design input, on `origin`, is recorded in [`design-input/CORRECTIONS.md`](design-input/CORRECTIONS.md).

## Step 3 — orchestrate, on what the Protocol already has

`scripts/lead_runs.py`. Items 1 and 2 are done; item 3 is filed; item 4 is posted on #12 and waits on the owner.

### `origin` on `execution.submit`

`{initiator, depth, call_budget}`, a field of the submit params and of the inspect result, under the same closed shape. **PIO refused it outright**: the gate required `execution.context_revalidation`, a feature that appears nowhere in the pinned schemas and that nothing advertises — while the three conformance fixtures about revalidation key off `execution.context`, the gate for `context_bindings`, a different field. The requirement was PIO's own and wrong, and nothing could say a run had a lead.

| | Rule | Mutant |
| --- | --- | --- |
| Depth | A call sits exactly one level below the run that started it; a top-level run is its own initiator at depth 0. **Derived, not configured** — the launch config is validated against the pinned `launch-config.schema.json`, whose `executor` is closed, so a limit could not have gone there, and a constant would be a number with no reason behind it. An initiator PIO has never seen constrains nothing. | `deeper` names a run already at depth 1, so the same depth 2 is one level down and admitted |
| Budget | Belongs to the run spending it, **counted from the runs that name it** rather than kept in a field PIO must hold in step — the journal is the counter, so it survives a restart. A refused run never started and spends nothing. | `richer` gives a second call, so the refused start is admitted |
| Echo | The view carries what the caller sent, never more | `no-origin` submits without it, so the view has none |

**A surviving mutant found a real bug.** `richer` passed at first, because a run that had been *refused* was still counted against the budget — a budget of two admitted only one.

### The lead's grant

A lead may submit, steer, read and follow the stream. It may **not** answer an approval.

Before this, `execution.steer` and `execution.respond_action` both fell to the default arm of the authorizer and were refused **whatever the grant said** — so a grant could not express steer at all, and refusing an answer proved nothing. Both are grantable now, which is what makes the refusal attributable: `permission_denied` / `right_missing`, with the owner answering the same action to show the refusal is about the right and not the action.

The negative runs through **`serve-opencode`**, not `serve-fake`: the fake advertises no `execution.actions`, so an answer there is refused `unsupported_required_feature` before authorization is reached — a refusal that says nothing about scope. Steer needed the same treatment, and the session has to negotiate the feature as well as hold the right.

### Two holes the reviewer found, closed before any live lead run

**`origin.initiator` was an unchecked caller claim.** A principal holding a plain submit grant could submit a run naming an **unrelated** run as its initiator. It was admitted, and the named run's next start was then refused `call_budget_spent`: forged lineage and budget theft, from a grant that was never meant to reach that run at all.

The initiator is now bound to the grant. Under a grant, the only run a caller may name is the one whose subtree the grant covers — derived from the grant's own resources (`id_prefix: "<lead>."` names `<lead>`), because `core.grant.issue`'s params are closed and no field could be added. **A grant that names no subtree names no initiator**, so an `origin` under an unscoped grant is refused rather than trusted: `permission_denied` / `out_of_scope`. The spoof case also asserts the victim's budget is **still there** afterwards.

**The lead grant's resources were unscoped.** `kind: execution.execution` with no `id` or `id_prefix` let a lead read every run on the service, including ones it did not start. Children are now named under the lead's prefix and the grant is scoped `id_prefix: "<lead>."`; an inspect outside it is `permission_denied` / `out_of_scope`.

**Omitting the origin was the same hole by the other door.** Once the lead's budget was spent, submits under its prefix carrying **no origin at all** were admitted, and their views carried no lineage — the budget checked nothing because there was nothing to check. A grant that names a subtree owner now *requires* an origin naming it; the reason is `out_of_scope`, the same as a forged one, because it is the same fence: a grant scoped to a subtree authorizes runs **in** that subtree, and a run that claims no lineage claims no place in it.

**Depth was accepted if it did not exceed the permitted one, not if it matched.** A child of the depth-0 lead could record `depth: 0` and its grandchild `depth: 1`, so the tree's shape was whatever the caller said. The depth is derived, so a claim that disagrees is refused, and the two directions are named apart — `call_depth_exceeded` and `call_depth_understated` — because "exceeded" is not true of a depth that is too shallow.

**An initiator must be alive to ask.** Owner decision, 2026-09-23. The first L1 rehearsal's lead was refused for a missing brief, and its two children were admitted anyway: lineage attached to a run that never started, spending a budget its caller had merely claimed. A child now names only an initiator that was **not refused** and has **not exited**. The refusals are `initiator_refused` and `initiator_exited`. A run naming itself is untouched, because it is the one being started. Mutants: `refused-initiator-ok` admits the initiator, and `exited-initiator-ok` submits the child while it is still running; both are then admitted. The two lead passes that start children now give their `serve-fake` lead a 60-second run, so it is alive while they do.

**Under a grant, an initiator PIO has never seen is refused `initiator_unknown`** (review 44). The reviewer's probe: a grant for `ghost.` with no run `ghost` admitted four children, one at depth 9 with a budget of 99, because an initiator PIO has not seen derives no depth and declares no budget. And children submitted before their lead existed spent the budget it was admitted with later. The grant already binds the initiator to its subtree's owner, and that owner now has to exist. The owner, submitting with no grant, may still name an initiator PIO has not seen. That is a claim PIO records and cannot check, and the case records it too. Mutant: `known-initiator-ok` admits `ghost` first, and its children are then admitted. The lead-grant pass now admits its lead before submitting under the grant, which it had never done.

Mutants: `bound-initiator` scopes the stranger's grant to the lead's own subtree, so naming the lead is legitimate and the submit is admitted; `unscoped-grant` hands out a grant that names no subtree and therefore requires no origin; `shallow-ok` claims the derived depth. Each shows the refusal is about the specific thing and not about origins, grants or depth in general.

### Codex — approvals come to the person

`ThreadStartParams.approvalsReviewer` overrides **where approval requests are routed**, and its enum is `user | auto_review | guardian_subagent` — two of the three send them somewhere other than the person. The whole `thread` object is operator configuration that reaches `thread/start` unchanged, so **PIO refuses to send the field at all**, at the wire, and asserts the harness's answer is `user` on every run. The answer is recorded in `thread_started` as well as asserted, because a field that is never read is not a check. `approvals_reviewer_must_be_user` makes the fake answer `guardian_subagent` and the run fails with `approvals_reviewer_not_user`. `approvals_reviewer_absent_refused` has it answer **nothing**, and the run fails the same way: 0.155.1's `ThreadStartResponse` lists the field as required, so a response without it did not come from the qualified app-server, and silence is not `user`. Until this was tightened, an absent answer passed as if it had said `user`.

### Steer authorship — filed, not widened

`steering_entry` is closed and the event record has no authorship field, so **a steer from a lead and a steer from the owner are indistinguishable on the stream**. PIO records the grant in the payload of `execution.steer.requested` under `pio.combraton.dev/under-grant` — `{grant, holder, recorded_by: "pio"}` — which says plainly that this is the implementation's record and not the Protocol's. Nothing closed gained a field and no new event type name was introduced.

The proposal is [Combraton/protocol#17](https://github.com/Combraton/protocol/issues/17), filed with the evidence: two places that could carry authorship and why neither can, what PIO did locally, and two shapes the Protocol could take. `--mutant owner-steers` sends the same steer with no grant and the key is absent, which is what shows it means *who held a grant* rather than *a steer happened*.

### L1 — the lead run, rebuilt

The first rehearsal was reverted in `f915137`. Its own lead had been refused for a missing brief while its children ran, and its live path did not exist. The rebuild follows review 42.

**The tool reaches the lead's session and no other.** `pio.combraton.dev/lead-tool` on `execution.submit` carries an ACP `McpServer` in the stdio shape `lead_tool_probe.py` measured: `{name, command, args, env}`. Admission copies it onto that run's OpenCode spec, the host sends it as that session's `mcpServers`, and every other run sends `[]`. Any other adapter refuses it rather than drop it. It is **the owner's act**: a submit under any grant that carries it is refused `owner_authority_required`, because a server spec is a command the harness will launch. The spec is journaled, so it may carry **no secret**. A credential-named variable or a `ccred1.` value anywhere in it is refused, and the lead's credential travels as a path to a 0600 file.

**The labeled fake does what 2.0.11 was measured doing.** It launches each listed server at `session/new` with `initialize`, `notifications/initialized` and `tools/list`, then scripts the `tools/call`s a model would make **inside** the turn. So the lead is running while its children start, which is the only time it can start anything. A session with no servers stands in for a led run and answers with the real line count of the file its brief names.

**One code path.** `scripts/lead_run.py --rehearse` and the live run differ in the harness binary and its environment, and in nothing a row depends on. Live adds a preflight (clean tree, a `cargo build` of it, the binary's digest), the owner's own OpenCode as configured, the MiniMax ledger with one line per run, and the lead sequence's total against the 1,600,000 stop, read from **charged**. It also adds the desk: every pending approval is written to `desk/pending-<action>.json` and answered only from `desk/answer-<action>.json`, the owner's decision relayed with their words. A rehearsal answers for itself and says so. Only `allow` and `deny` are encodable, and the host sends them as the single-use kind.

**Every row carries its expected value, and a mismatch fails the run.** 27 rows. The lead's own run is admitted, acknowledged and exits normally. The tool's own log shows it was launched **exactly once**. Both children are admitted, bound to `L1` at depth 1, run, and exit. A third start through the same tool, made while the lead is still running, is refused `call_budget_spent` by PIO. Two steers are sent under the grant: one while the child runs, which shows OpenCode has no steer, and one after it exits, which any harness refuses. Both are `not_supported` and both name the grant and the holder `lead`. The lead may not answer an approval, a check aimed at a child it can read so only the missing right can refuse it. It cannot read its own run. It cannot attach the tool. Each child's count and the lead's relay are compared with the runner's own `wc -l`. Every approval is decided at the desk, sent as the single-use kind with `always_option_taken: false`, and none lapsed. Every run reports its usage. The owner's service is untouched.

**What a rehearsal cannot prove, and says so:** that a real model uses the tool at all; that the counts are right, because the fake counted, so there those two rows show only that the runner compares; what the owner's OpenCode does with a permission prompt; and that PIO deleted no session in the owner's history.

**Mutants.** Seven in the runner, each required to fail its named row, in CI: `no-tool`, `reports-refused-as-started`, `grant-may-answer`, `grant-no-steer`, `wrong-child`, `wrong-relay`, `lead-without-brief`. Two in the source, run by hand: the host sending `[]` while recording `pio-lead` fails "the tool was launched exactly once" (0), and removing the grant refusal fails "a grant cannot attach the tool".

#### Review 44 — the exit path, the desk, and a bound on spend

**Every exit writes the receipt and charges the ledger.** That covers an exception, `SystemExit`, `KeyboardInterrupt` and `SIGTERM`. On the way out the runner creates the lead's stop file, cancels whatever still runs, waits out the host's kill, reads usage from the views before the service is released, charges every run that ran, writes the receipt, and raises the error again. A second interrupt during that path is ignored. A rehearsal charges a ledger of its own, so the charging code is the live run's. Before this, a desk answer the runner did not expect printed a line and wrote nothing: live, tokens spent with no ledger line and no receipt.

**The desk never blocks the run and never gives up on it.** It is asked on every pass of the watch. A request nobody answers is left alone. The host's single-use reject lands when the delivery timeout runs out, counted from the moment the request reached it (`opencode.rs`, `action_answer_timeout_seconds` = the run's `timeouts.delivery`, 300 s), and the desk records it as lapsed. The run finishes and the receipt is written. A lapse fails "every approval was decided at the desk", which ends L1 as failed with its cause. An answer that is not `allow` or `deny` is never sent. It is set aside so that a corrected one can take its place.

**OpenCode reports only the last model step's usage.** The M3b runs R2, R3 and R5 each made a tool call, so each took two model steps, and each recorded only the second. It was found in the owner's own OpenCode store, read-only (`mode=ro`), on 2026-09-23 between 17:46 and 18:01Z. **Exactly what was read:** the names in `~/.local/share/opencode`; the store's schema (`sqlite_master`, every table's definition); and rows from `part`, `message` and `session_message` for the three PIO session ids only. `part` and `message` held nothing for them. No credential-table row and no auth file was read. (The first account of this on #12 said "`session_message` rows only", which was wrong, and is corrected there.) The committed receipts corroborate it: each step 1's input plus cache read equals step 2's `cachedReadTokens` (8,088/8,088, 8,200/8,199, 8,204/8,204; review 45). The `usage` of a `session/prompt` result is the turn's last assistant message:

| Run | Step 1 (not recorded) | Step 2 (recorded and charged) | Turn |
| --- | --- | --- | --- |
| R2 | 8,135 | 8,208 | 16,343 |
| R3 | 8,238 | 8,288 | 16,526 |
| R5 | 8,270 | 8,392 | 16,662 |

M3b therefore under-charged by **24,643**. Correction lines, one per run with the step it missed, are added to the MiniMax ledger: 62,220 becomes **86,863** charged.

**For L1, the charge is read from the store.** Owner decision, 2026-09-24: the live runner may read, read-only, the `session_message` rows of the sessions PIO started, and nothing else in the store. Each run is charged the sum of its steps as recorded there, provided the last step is the one OpenCode reported. The bound is recorded beside it: the reported total times the number of steps the turn could have taken, one per tool call plus one. Where the store cannot be read, or its last step disagrees, the bound is charged. It is an upper bound because each earlier step's context is contained in the last one's. A row checks the read, live only. `store_steps` reproduces the M3b figures above exactly.

**Every run is metered while it runs, and stopped at a ceiling.** Nothing OpenCode reports during a turn can stop it. What PIO does see is every session update, spooled as it arrives. A run has spent at most `(tool calls + 1) × (10,000 + bytes its session has sent)`: each tool call is at most one more step, and no step's context holds more than the harness's fixed base (7,960 to 8,076 tokens in M3b, rounded up) plus everything sent so far, since a token is at least one byte. The runner reads every meter twice a second. It cancels the lead past **16 tool calls** or **450,000**, and a child at **90,000**. OpenCode does not end a turn on `session/cancel` (M3b R4), so the host kills it ten seconds later. Until then the lead's own tool **holds every call** for 30 seconds and then refuses it: past its ceiling, or once the runner has created its stop file. So the lead cannot take another step through the tool. `read_run` now waits up to **55 seconds** for its run to exit before it answers, and returns at most 2,000 characters. It was 20 at `29ed7a5`. In `desk-silent`, with the fake's ~680 bytes a call, the lead's meter stopped it at 17 calls one second before the lapse landed; live calls are larger, so an answer in the desk's third minute could have failed L1 with nothing wrong (review 45). At 55 seconds, a child held for the whole 300-second answer window costs the lead six reads, and sixteen calls leave room for more than twice that. 55 seconds is under the MCP TypeScript SDK's 60-second default. It is far under what OpenCode 2.0.11 allows: its shipped code makes an ACP-supplied server a local one with no timeout, and `callTool` falls back to 43,200,000 ms (12 hours). That is static evidence from the shipped code, not a measurement, and the live tool log is the check.

**The worst case for one attempt is 1,395,200 tokens**, under the 1,600,000 stop. The live runner refuses to start unless the sequence's charged total plus this bound stays under the stop.

- **Lead: 951,200.** 450,000, plus the step in flight at the stop, which is at most 450,000 again plus one tool result. OpenCode truncates a built-in tool's output at 50 KiB, which PIO has not measured.
- **Each child: 222,000.** 90,000, plus at most eleven steps before the kill (the fastest M3b step took 0.96 s) of 12,000 each (M3b's child steps were 8,135 to 8,392). **The child figure is not enforced:** PIO cannot bound the size of a child's step.
- **Assumed:** everything added to a context after the first step arrives as a session update, and the runner reads a meter at least once per step.

**The running steer hits a turn that is running and delivered.** `runtime: active` and `delivery: acknowledged` when it is sent, and not yet exited just after. Anything earlier is `not_supported` on every harness, so it would say nothing about OpenCode. **Each run's model** is a row, read from the host's own events: `model_selected`, then `session_created` with `reported_model: minimax-coding-plan/MiniMax-M3` and `model_matches_requested: true`, both before `turn_start_sent`. **A no-sleep assertion** (`caffeinate -i -s -w <pid>`) is held for the whole run and recorded in the receipt with the power source. Live, the run refuses to start without one: macOS stops its monotonic clock while asleep, and every deadline here stops with it. The receipt names the owner's approvals: the M3b model exception, the lead tool, and the L1 plan. The permission-prompt observation is now a **record**, never counted, since it has nothing to compare against.

**A reservation before anything can spend, and a watchdog** (review 45). Before the service starts, each run's worst-case share is written to the ledger as a `reserved` line: 951,200 for the lead, 222,000 for each child. The exit path replaces each with its charge; a run the service says was refused or never submitted is charged 0. A runner killed from outside (SIGKILL) runs no exit path. Before this, it left no receipt and no ledger line, and `serve-opencode` alive with the lead running to its 900-second deadline, unwatched. Now the reservation stands. A watchdog started in its own session sees the runner gone without `runner-done`. It stops the lead's tool, cancels every run still going, waits out the host's kill, stops the daemon, and writes `watchdog.json`. Mutant `runner-killed` SIGKILLs the runner once the desk has answered. There must then be no receipt, every reservation standing, and the daemon stopped. The live runner is run detached, never under a tool's timeout.

**A root run is the owner's act; a lead's grant starts runs only inside the lead's subtree.** Owner decision, 2026-09-24 (review 45, item 4); the wording narrowed to lead grants after review 46 (below). `initiator_unknown` had made a grant that names one run by id unable to start it, for a reason that explained nothing. Now a submit under any grant whose origin names the run itself is refused `owner_authority_required`. That covers the run a grant names by id, and a subtree's owner under its own subtree's grant. With no origin, the grant's refusal is `out_of_scope`, as before. The owner starts the same root, and its child is then admitted under the subtree grant. Case `root_runs` in `lead_runs.py`; mutant `owner-submits-root` sends the root submits as the owner and they are admitted.

**Nits.** Receipts and desk files leave the machine scrubbed. `redact` rewrites only home paths; the repository and any other volume path are now named (`<repo>`, `<volume>`) rather than spelled out. A running steer whose turn ends inside the steer's own round trip is recorded as **inconclusive**, neither held nor failed.

#### L1 live, and review 46

**L1 ran live** on 2026-09-24 at `703706d`, 10:27:35Z to 10:28:11Z, the owner at the desk ("start now"). Every row held. Charged 73,532 (L1 41,072, `L1.alpha` 16,232, `L1.beta` 16,228). The receipt and what it does not show: [`evidence/L1-live.json`](evidence/README.md). Two things it found:

- **OpenCode wraps MCP tools in its own `execute` tool.** The model wrote code that called the lead tool: three `execute` calls carried the four MCP calls. So the meter counts ACP tool calls, the tool counts MCP calls, and the store's model steps matched the meter's bound (calls + 1).
- **The release was refused.** The live tree, `~/pio-m4-live`, was never registered with the cleanup (the M3b runner registers its own), so the runner's sweep for surviving processes did not run, and no row read `release_error`. Checked by hand: nothing survived.

**Before merge, review 46:**

| Item | Now |
| --- | --- |
| D1. The root rule, under grants that name no subtree | The reviewer's probe: under an unscoped `{kind}`, a bare prefix `x` with no separator, or two ids, a submit with **no origin** is admitted. The preferred fix, refusing every submit under such a grant, was built and **failed two pinned conformance fixtures** (`execution.effects-resolve-only-with-target-authority` and `execution.recovery-revalidates-before-dispatch`): each issues a plain submit grant `id_prefix: "a1-"` and requires its origin-less submit to succeed. So the code stays and **the wording is narrowed**: the root-run rule is about **lead grants**, the ones that name a subtree. A grant that names none is a plain Protocol submit grant: it may start a run with no lineage and may claim none (`out_of_scope`). Case `plain_grants_are_not_leads` records all three shapes both ways, and that a lead's budget is untouched by them; mutant `names-a-subtree` scopes the three grants to `x.`, and the origin-less submit is then refused. Conformance stays at 206. |
| E1. What the charge is | The basis is `measured_from_store_steps`: **measured from the store's recorded steps, which is not the bill**. Not proven: that the store holds every billed call, because OpenCode 2.0.11 ships hidden title, summary and compaction agents. The owner reconciles each live run against the MiniMax console. |
| E2. After a SIGKILL | The reservations (1,395,200) stand and count as charged, so the next attempt is refused by the 1,600,000 stop before it starts. Replacing them with a figure is the owner's act, dated and recorded in the ledger, never the runner's. |
| F1. The release (from the live run) | The live tree is made through one function that registers it with the cleanup and makes it `0700`. The live run refuses to start if the cleanup could not release its tree. Every rehearsal makes a probe through the same function, checks it, and removes it; with the registration removed by hand, the rehearsal fails that row. Two more rows: the runner's own steps raised no error (a `judge_error` or `settle_error` left rows unwritten, and an unwritten row never fails), and the service was released with nothing surviving. Mutant `release-refused` has the guard refuse the tree at the end; the run fails on that row, then the tree is released for real. |
| Optional | The watchdog holds its own `caffeinate -i -s -w <its pid>`. |

32 rows and one record. Twelve runner mutants, in CI.

29 rows and one record at `703706d`. The runner mutants added since review 44, all in CI: `desk-silent` never answers, and dies on the desk row after the 300-second lapse with its receipt written. `lead-loops` keeps calling `read_run`, and the runner stops it at 17 calls while the tool holds the 17th. `interrupted` raises `KeyboardInterrupt` with every run admitted. All three runs are cancelled on the way out and charged their bounds, and the receipt is written. `runner-killed` is described above. Every other mutant must also leave a receipt that charges each run that ran, with no reservation left.

#### L3 — a lead on Codex, built (plan approved 2026-09-24; decisions of 2026-09-25)

L3 is L1's shape on Codex 0.157.0 and `gpt-5.6-terra` (`--plan L3`, lead `L3`, sequence `M4-lead-codex`, cap 400,000, stop 320,000). Its claim is **a steer under the lead's grant, delivered to a running Codex turn**. M2 showed Codex acknowledge the owner's steer; L3 shows it under the lead's authority. Behavior stays `not_observed`: delivery is not obedience.

- **The children** each run one command, the one their brief quotes: `sleep 30 && wc -l alpha.md` and `sleep 5 && wc -l beta.md`. `alpha` is still running when the runner steers it under the lead's grant, and the lead's `read_run` of it waits.
- **The limits (owner, 2026-09-24):** lead 125,000; child 50,000; and at most one step in flight after a stop, 30,000, from the 22,000 to 24,500 per step M2 measured. The worst case is 155,000 + 2 × 80,000 = 315,000, reserved before anything starts. That is under the 320,000 stop, and 421,450 + 315,000 = 736,450 is under Codex's 800,000. Each run is charged Codex's reported total, which covers every step (review 48). The runner stops a run by that total, which arrives after every step.
- **The lead tool** rides on the lead's own `thread/start` `config.mcp_servers`, the route the M4b probe saw Codex launch; children get none. `serve-codex` accepts it under owner authority only, and a grant is still refused.
- **The lead's own two tools are pre-allowed (owner decision, 2026-09-25).** Codex 0.157.0 asks before every MCP tool call that is not marked read-only; this was read from its source and is identical at 0.155.1. So the lead's thread config sets `approval_mode = "approve"` for `start_run` and `read_run` on that one server. The setting is passed per launch and never written to the owner's configuration. Every other request still comes to the desk.
- **MCP tool-call approvals come to the desk.** They arrive as `mcpServer/elicitation/request` with `codex_approval_kind: mcp_tool_call`, and the host surfaces them as actions. `allow` is sent as `accept` with no `persist`, a single use; `deny` as `decline`; never "for this session" or "don't ask again". Any other elicitation is declined natively.
- **Unanswered approvals lapse (owner decision, 2026-09-25).** PIO sends one `decline` after the caller's delivery timeout.
- **Model and provider** come from Codex's own `thread/start` answer, before the first turn: `gpt-5.6-terra` on `openai`, with `approvalsReviewer: user`. A mismatch ends the run with no turn sent. The provider is only checked, never sent.
- **Codex was re-pinned to 0.157.0 (owner decision, 2026-09-25)** after it updated itself. What PIO's host uses is unchanged in shape; the evidence is in `docs/work/m2/codex-qualification/`.
- **Not proven by the rehearsal:** the fake is not Codex. When Codex asks, what it offers and how it reads an answer come from its source, not from a measurement. Whether `on-request` in a writable sandbox asks about `sleep && wc -l` at all is unknown; the rehearsal's `beta` asks so the desk has something to relay, and live, the desk row may be inconclusive.

#### L1b — the desk and a waiting read, live (plan approved 2026-09-24)

L1 held two rows only because nothing could fail them: no approval was asked, and the fixture's session history was empty (review 47). And no read had to wait. **L1b** is L1's shape (`--plan L1b`, lead `L1b`, the `M4-lead-opencode` sequence), briefed so that both happen:

- **The desk.** OpenCode 2.0.11's shipped default asks before reading `*.env` files (review 48, a static reading). The owner's `opencode.jsonc` overrides no permission: checked 2026-09-24, reading only its permission keys. `beta` reads a placeholder `beta.env` with OpenCode's read tool. The file exists only in the fixture, every line says it is a placeholder, and nothing in it looks like a setting. The request is inside the workspace, so PIO relays it to the desk rather than declining it. The owner answers.
- **The waiting read.** `alpha` runs `sleep 30` before it counts. A shell command inside the workspace runs without a prompt (M3b). A row reads, from the lead tool's own log, a `read_run` of `alpha` that took at least 20 s and returned `exited`. The tool now logs each call's duration.
- **Review 47, in both plans:** the desk row is inconclusive when nothing was asked, and "no session deleted" is inconclusive when nothing was listed before.
- **The fake** gained `led_delay_if` and `ask_if`, so only the run whose prompt names it waits or asks. Its line counter now reads `.env` names, and the lead's relay is parsed for the plan's own file names. Checked against L1's live relay, which ran on without a separator.
- **Mutants:** `no-wait` fails the waiting row; `no-ask` must leave the desk row inconclusive.
- **After attempt 1** (it failed on the desk, when the owner's answer arrived after the deadline; see the evidence README): attempt 1 read `alpha` in 54.6 s of the tool's 55 s wait. A slightly slower `alpha` would have come back still running, and the row would have failed a read that had waited the longest it can. The owner's decision of 2026-09-25: the row also holds on a read of `alpha` that came back still running after the limit. Mutant `alpha-outlasts` has `alpha` work 62 s. The row must hold through the limit alone: the second read returns `exited` too soon to count. With the limit branch removed, it fails. Attempt 2 runs under its own ledger name, `--attempt L1b-attempt-2`.
- **A leak the gate found.** L1b's first rehearsal crashed while building its scenario, after its tree existed and before the `try` that releases it, and the tree stayed in `/tmp`. Now a failure before anything is reserved releases the tree. Mutant `setup-fails` fails the scenario at that point: the tree must be gone, and there is no receipt. The source mutant (the release removed) fails it.
- **Attempt 2 held** (2026-09-25, at `0ba60de`): 32 of 33 rows hold, and "PIO deleted no session" is inconclusive. The desk request was the same as in attempt 1 (`inside_fixture`; `once` / `always` / `reject`). It was allowed once by the relay, from the owner's advance decision, within a second of reaching the desk, so it was not answered live. The lead's read of `alpha` waited 53.1 s and returned `exited`, so the row held on the exited branch, not on the limit. 82,910 charged. See the evidence README.

**Step 2 order:** ~~G1 and G6 (the events fold)~~ **done**, ~~G2 (the approval walk)~~ **done**, ~~G3 (blocks and the audit)~~ **done**. Each with its own headless case and a mutant. Next: step 3, orchestrate.
