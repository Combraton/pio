# M4 step 1 — the screen-to-interface map

Every element of every accepted screen, and the **public operation** that feeds it. Where none exists, it is marked **GAP** and numbered.

Design input: [`design-input/M4-DESIGN-INPUT.md`](design-input/M4-DESIGN-INPUT.md), accepted by the owner 2026-09-21, with its [corrections](design-input/CORRECTIONS.md). Rule 1 of that document is the constraint this map exists to test: *the screen reads and writes only through the public API the command line uses.*

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

**A run waiting on an approval is sometimes ended by a PIO timeout before its answer clock lapses.** The signature is exact: `execution.timeout.passed` on that run, **no** `execution.usage.observed` (so the harness was stopped, not finished), and the action still `pending`. Seen **three times in twenty-eight runs**, always while other heavy work was running, and **not once in the twenty-five since** — including a batch run with the machine deliberately saturated. I could not identify which of the five timeouts fires, and I will not claim a cause I have not measured.

What is known:

- **It is not a regression of this fix.** At `a22c98c` the same run also ends with the action `pending`; that is the defect this section fixes. This failure is the *absence* of a lapse to observe, not a lapse that went unreported.
- **`action_answer_timeout_seconds` *is* `timeouts.delivery`.** A run's answer deadline and its delivery deadline are the same number. While delivery is still `pending` the delivery timeout is live — which is why the pass now asserts **acknowledged delivery before drawing the walk**. That assertion was already in place when the third failure happened, so it is not the whole story.
- **The screen is correct under this failure anyway.** A run that has exited cannot answer anything, so **the walk drops approvals whose run has exited** — a rule that is exercised on real data, including at `a22c98c` where it is the *only* thing that empties the walk.

The pass now tells the two apart: if `execution.timeout.passed` is present for the lapse run it fails with that named, and dumps the service's own record for the execution — the timeouts, `admitted_at`, `timeouts_passed` and the delivery state — which is what will identify the clock the next time it happens.

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

**Step 2 order:** ~~G1 and G6 (the events fold)~~ **done**, ~~G2 (the approval walk)~~ **done**, ~~G3 (blocks and the audit)~~ **done**. Each with its own headless case and a mutant. Next: step 3, orchestrate.
