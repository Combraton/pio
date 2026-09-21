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
| **G2** | The approval walk needs, per pending request: the **deadline**, the **option list the harness offered**, what PIO will send, and the classification. `actions[]` gives identity, owner, state and `requested_at` — and is closed, so the rest cannot go there. | **Event payload**, keyed by `action_id`, read with `core.events.read`. | 1, 4, 7 |
| **G3** | Transcript blocks **as they happen**, each with where a tool use landed and who decided. | **Event payload** per block, plus incremental per-harness sources. Not promotion, and not end-of-turn. See below. | 1, 5 |
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

## G3 — blocks as they happen, not an end-of-turn audit

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

Runs still working and approvals still waiting are **G1** and **G2**. *While you were away* is `core.events.read` from the stored cursor — **have**. Anything PIO decided is in the events already.

## What the first draft got wrong

Recorded rather than quietly fixed, because the reviewer will want to know which conclusions moved.

1. **It confused two fields with the same name.** PIO's **event** `origin` is `command | provider` and says whether the service or a caller produced the event. The **view's** `origin` is `{initiator, depth, call_budget}` and says who started whom. I read the first, concluded "not implemented", and called it a **Protocol** gap. The first half was right and the second was not: it is defined in the pinned schema and unimplemented in PIO. A PIO task, no Protocol issue.
2. **It did not check `unevaluatedProperties`.** Most of the draft assumed fields could be promoted into the view. They cannot. Every "promotion" answer is now a route — event payload or a namespaced profile.
3. **It listed the view from what PIO populates, not from the schema.** `actions[]`, `steering[]`, `origin` and `scheduling` were all missing, and `actions[]` is most of the approval walk.
4. **It called G3 "mostly promotion".** `tool_uses` is an end-of-turn audit; a live view needs blocks as they arrive, and must say **not yet classified** rather than guess `inside`.

The reviewer's own correction to the design input, on `origin`, is recorded in [`design-input/CORRECTIONS.md`](design-input/CORRECTIONS.md).

**Step 2 order:** ~~G1 and G6 (the events fold)~~ **done**, then G2, then G3. Each with its own headless case and a mutant.
