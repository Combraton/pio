# M4 step 1 — the screen-to-interface map

Every element of every accepted screen, and the **public operation** that feeds it. Where none exists, it is marked **GAP** and numbered. No terminal code is written until the reviewer has read this.

Design input: [`design-input/M4-DESIGN-INPUT.md`](design-input/M4-DESIGN-INPUT.md), accepted by the owner 2026-09-21. Rule 1 of that document is the constraint this map exists to test: *the screen reads and writes only through the public API the command line uses.*

## What the public interface has today

Read, by a caller with the right grant:

| Operation | Gives | Shape |
| --- | --- | --- |
| `core.describe` | service identity | one object |
| `core.negotiate` | profiles, majors, features | one object |
| `core.capabilities` | what this service can do | one object |
| `core.events.read` | events **from a cursor**, with `next_cursor` | page |
| `core.events.subscribe` / `.unsubscribe` | a live feed of the same | stream |
| `core.effects.get` | one effect | one object |
| `core.grant.get` | one grant | one object |
| `execution.discovery.list` | installations, separating detected / supported / authenticated / usable | list |
| `execution.inspect` | the whole view of **one** execution, by id, plus `next_cursor` | one object |
| `execution.output.read` | raw output bytes from an offset | bytes |
| `execution.reconcile` | executions, deliveries and obligations **matching one `command_id` or `delivery_id`** | list |

Write:

`execution.submit`, `execution.cancel`, `execution.steer`, `execution.respond_action`, `execution.controller.claim`, `execution.workspace.checkpoint`, `core.grant.issue`, `core.grant.revoke`, `core.effects.abort_obligation`.

The `execution.inspect` view carries: `admission`, `containment`, `completions`, `deliveries` and `delivery`, `effects`, `evaluation`, `execution`, `exit`, `host` (id and generation), `obligations`, `recovery`, `result`, `revision`, `runtime` and `runtime_detail`, `usage` (observations and liability), `workspace` (lease, base, checkpoints), and `next_cursor`.

## The gaps, numbered

| # | Gap | Which screens need it |
| --- | --- | --- |
| **G1** | **No query lists the executions a caller can see.** `execution.inspect` needs an id; `execution.reconcile` needs a `command_id` or a `delivery_id`. A board of five or six runs cannot be drawn. | 1, 2, 3, 7 |
| **G2** | **No query for pending requests across runs, with their deadlines.** A surfaced request appears in one execution's `runtime_detail`; the approval walk needs all of them, ordered, with time left. | 1, 4, 7 |
| **G3** | **The transcript is bytes, not blocks.** `execution.output.read` returns raw output. The run view needs blocks — what happened, its result, **where each tool use landed** and **who decided** — and the host already records exactly that in `tool_uses`, where nothing public can reach it. | 1, 5 |
| **G4** | **Mid-turn messages for Claude Code.** `execution.steer` exists and Codex takes one; the Claude host has no steer control. The thread must show **queued**, never delivered, until it does. | 3, 5 |
| **G5** | **Per-message usage for Claude Code.** Usage arrives once, at the end of a turn. A live token count on a running card needs it sooner. | 1, 2, 5 |
| **G6** | **Incremental changed-key commits.** The M2 prerequisite. A board refreshing five or six runs re-reads whole state today. | 1, 2 |
| **G7** | **Projects are not modelled anywhere.** A user-named folder grouping runs is client-side configuration; nothing in the Protocol knows about it, and nothing should. | 1 |

**One of the reviewer's expected gaps is not a gap.** "Events since a cursor, for *while you were away*" **already exists**: `core.events.read` takes a cursor and returns `next_cursor`, and `execution.inspect` hands out a cursor too. Screen 7 needs no new operation for it — only a client that keeps the cursor across a detach. Recorded here rather than built twice.

## Screen 1 — the board

| Element | Fed by | Status |
| --- | --- | --- |
| Product, service name, version (top left) | `core.describe` | have |
| Qualified harness versions (top right) | `execution.discovery.list` | have |
| Counts of runs by state (top left) | — | **G1** |
| PROJECTS card: the project a run belongs to, its name, its working location | — | **G7** (client-side; the location itself is `workspace.lease` from `execution.inspect`) |
| Run row: glyph and state word | `execution.inspect` → `runtime`, `delivery`, `usage.liability` | have, per run; needs **G1** to enumerate |
| Run row: harness and its colour | `execution.inspect` → `admission`, and the adapter in the submit record | have |
| Run row: worktree | `execution.inspect` → `workspace.lease.base` | have |
| Run row: age | `execution.inspect` → `submit` timestamp | have |
| Per-project state counts | — | **G1** |
| Preview card: name chip, state, harness version, location, tokens | `execution.inspect` | have |
| Preview card: the **truth line** | `execution.inspect` → `deliveries[].evidence` and `.proof_class`, `usage.liability`, `recovery` | have |
| Preview card: the latest blocks | — | **G3** |
| Sticky note, amber: approvals waiting, and for which runs | — | **G2** |
| Sticky note, violet: uncertain runs | `execution.inspect` → `delivery == ambiguous`, `usage.liability == unresolved`, `runtime == unknown` | have per run; needs **G1** |
| Live filter `/`, fold `space`, resize, sizes persisting | client-side | have |
| Refresh without re-reading everything | — | **G6** |

## Screen 2 — split view

Every element is screen 1's preview card, repeated. Same sources, same gaps: **G1** to know which runs exist, **G3** for the blocks, **G5** for a live token count. `+`, `x`, `z`, `=` and the resize keys are client-side. **Closing a card never stops the run**: it issues no operation at all, which is the easiest rule in the document to honour and the easiest to break by accident, so it gets its own headless case.

## Screen 3 — orchestrate (M4b)

Blocked before any code by the two items the design input names: a Protocol proposal so a message can record **which run wrote it**, and the owner's dated approval of the lead's tool. Mapping what it would need anyway:

| Element | Fed by | Status |
| --- | --- | --- |
| Lead card, goal, what it waits for | `execution.inspect` on the lead | have |
| The runs the lead started, as a team | — | **not implemented.** See below: PIO's `origin` is not the Protocol's |
| MESSAGES thread: from → to, kind, time | — | **G4** for author identity; the Protocol proposal the design input requires |
| Delivery ticks `✓` / `✓✓` / violet `◇` | `execution.inspect` → `deliveries[].delivery` and `.proof_class` | have. **An unproven message never shows two ticks** follows directly from `proof_class: null`, which is exactly OpenCode's case |
| The lead's budget meter | `execution.submit` → `payload.budget` with `pool`, `amount` and `ceiling` | **implemented.** See below |
| A run may never answer another run's permission request | `core.grant.issue` with rights that exclude `execution.respond_action` | have — this is a grant, not new code |

### What PIO implements today, of the three things step 3 names

Checked in the source rather than assumed, because step 3 says to implement the Protocol's features and not new ones.

**Budget pools: implemented.** `execution.submit` accepts `payload.budget` with a `pool`, an `amount` and a `ceiling` of `hard` or `soft`. The pool's measure comes from `executor.budget_pools`; usage is summed across every execution holding a `reserved` or `settled` reservation in that pool. A pool that does not exist is refused `budget_unavailable`, and — the part that matters for a lead — **a `hard` ceiling is refused `enforcement_unavailable` unless the adapter's `enforced_bounds` contains that measure.** PIO will not promise a hard ceiling it cannot enforce. A lead's budget meter is therefore a feature that exists, not one to build.

**Grants with rights and resources: implemented.** `core.grant.issue` and `core.grant.revoke`, with narrowing enforced: a child grant may not hold a right its parent lacks. So "a lead is a principal with a scoped grant to submit, steer and inspect, with **no** right to respond to an action" is expressible today, by omitting `execution.respond_action` from its rights — a grant, not new code. The rule *a run can never answer another run's permission request* needs no new mechanism, only a case proving the grant refuses it.

**Origin with initiator, depth and call budget: not implemented.** PIO has an `origin` field on every event, but it takes two values — `provider` or `command` — and says whether the service or a caller produced the event. It carries no initiator, no depth and no call budget. Nothing in the codebase mentions those. This is the same hole the design input names first: *a message can record which run wrote it* is exactly what is missing, and it is a **Protocol** gap rather than a PIO one. A Protocol issue is warranted here; the other two need none.

## Screen 4 — the approval walk

| Element | Fed by | Status |
| --- | --- | --- |
| One request at a time, across runs, in order | — | **G2** |
| Run, action id, what it wants, where it lands | `execution.inspect` → `runtime_detail`, and the classification the host records | have per run; the **where it lands** part is the same record as **G3** |
| What the harness offered | the host records the real option list (`permission_options_observed`) | recorded, **not public** — part of **G2** |
| What PIO will send | the host chooses by option **kind** | same |
| "Always allow" visible and visibly unavailable | from the offered list | same |
| `1` allow once, `2` deny | `execution.respond_action` | have |
| `3` show the full request | same source as above | **G2** |
| `4` decide later, `tab` next | client-side | have |
| The deadline, and that no answer means PIO denies it | the host's `action_answer_timeout_seconds`, from the caller's own `timeouts.delivery` | recorded; surfacing it is **G2** |
| **Negative control:** an answer aimed at the wrong run or a stale controller fails loudly | `execution.respond_action` already fences on subject and revision | have — this is the one element with live evidence behind it already |

## Screen 5 — the run view

| Element | Fed by | Status |
| --- | --- | --- |
| Run identity, state, tokens | `execution.inspect` | have |
| The truth line, folding open into full evidence | `execution.inspect` → `deliveries`, `usage`, `recovery`, `obligations` | have |
| Blocks: `⏺` what happened, `⎿` its result | — | **G3** |
| Where each tool use landed | the host's `tool_uses` record | recorded, **not public** — **G3** |
| Who allowed it: the user's rules, the user, PIO, the harness | the same record's `decided_by` and `denied_by_harness` | recorded, **not public** — **G3** |
| `d` diff | `execution.workspace.checkpoint` and the lease | have |
| `c` cancel, with confirmation | `execution.cancel` | have |
| `[` `]` previous or next run | — | **G1** |
| A live token count while running | — | **G5** for Claude Code; OpenCode reports `usage_update` mid-turn and Codex reports per message |

## Screen 6 — an uncertain run

| Element | Fed by | Status |
| --- | --- | --- |
| What PIO knows | `execution.inspect` → `deliveries[].evidence`, `exit`, `result` | have |
| What PIO does **not** know | `usage.liability == unresolved`, `delivery == ambiguous`, `result == absent`, `exit == unavailable` | have |
| What PIO **will not** do | `recovery` and the open `obligations` | have |
| A workspace check the user can verify themselves | `workspace.lease.base` and checkpoints | have |
| One action: open the workspace diff | client-side, from the lease | have |
| `e` evidence | `core.events.read` scoped to the execution | have |
| **Usage unknown is never shown as zero** | `usage.observations` is empty and `liability` is `unresolved`; there is no zero to show | have — and this is the rule three live runs were spent establishing |

**Screen 6 needs no new operation.** It is the screen the existing interface supports best, which is the right way round.

## Screen 7 — leaving and coming back

| Element | Fed by | Status |
| --- | --- | --- |
| How many runs keep working | — | **G1** |
| How many approvals keep waiting, and when PIO will deny them | — | **G2** |
| **While you were away** | `core.events.read` from the stored cursor | **have** — the not-a-gap above |
| Anything PIO decided, marked as PIO's decision | the host's `request_denied_by_default` events carry `decided_by: pio` | events are public; naming it per run is **G3** |

## What this map says

Six of the seven gaps are **one shape**: the host already records the thing, and nothing public can reach it. `tool_uses`, the permission option list, the deadline, who decided — all of it is in the append-only event file and none of it is in the view. **G1** and **G2** are the two that need genuinely new query surface; **G3** is mostly promotion of what exists; **G7** is client-side and belongs nowhere near the Protocol.

The screens that need the least are the ones about uncertainty. That is a good sign for the interface and a plain statement of where the work is.

**Waiting for the reviewer's read of this map before step 2.**
