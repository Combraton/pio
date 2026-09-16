# M2 checkpoint — ADR 002 projection capacity bound

Owner: Claude Code session, standalone PIO M2 builder. [Issue #5](https://github.com/Combraton/pio/issues/5). Base `9cf70474c28f549650e6b48e8be20ae88426a1b0`; **tested implementation head `5be40f999484e18e4b3966b898ee4be70691379e`** on `codex/m2-codex-app-server`. This documentation successor records the evidence; its own SHA is in Git history. **Bounded checkpoint, not M2 acceptance.** No real harness has run through PIO; all six journeys remain `not_evaluated`; 0 live model tokens used.

## What changed

ADR 002's gate is enforced before any live run. The rules are in [ADR 002: capacity bound](../../decisions/002-protocol-journal.md#capacity-bound-2026-09-16):

- **Hard limits** on the retained Protocol projection: **32,768 records** and **33,554,432 bytes**, where a record's bytes are its key plus its compact JSON value, equal in length to canonical encoding. `meta`, subjects, grants, dedupe outcomes, executions, effects and every retained event count.
- **Checked in `Store::commit_protocol`** after the writer fence and before any row is staged, on every Protocol commit path. No-op commits are not re-checked; commits that stay within the bound, including shrinking ones, are accepted. Nothing is evicted.
- **Refusal:** a typed local `CapacityExceeded { limit, maximum, projected }` with no projection row, journal fact or outbox entry. Publicly this is the frozen CORE §10 step 8 `unavailable` (`same_command`, nothing bound), because Protocol v0.1.0 has no capacity error. Each distinct refusal produces one `PIO capacity refusal` stderr line.
- **Service behavior at capacity:** a capacity-refused background commit no longer makes every request unavailable. Queries and bound replays answer from committed state; new commands are refused at their own commit. Other store failures are unchanged.
- **Test tooling:** `pio fake fill-projection DIR RECORDS` pads or shrinks a stopped store's filler records through the same bounded commit. It is not a product command.

## Evidence

[Receipt index](evidence/commit-bound.json). [Runtime CI 35118539258](https://github.com/Combraton/pio/actions/runs/35118539258) passes at `5be40f9` on **macOS 15 arm64** and **Ubuntu 24.04 x86_64**. A fresh HTTPS clone at the same head completes the exact [documented sequence](../../VERIFICATION.md) with fresh stores and freshly downloaded release assets: all 14 commands exit 0.

| Check | Result (identical in clean clone, macOS CI and Linux CI) |
| --- | --- |
| Cargo | fmt, locked build, Clippy `-D warnings`, docs, lockfile; **28 tests pass** (20 from M1 plus 8 new), 1 ignored measurement probe |
| Pinned runner | **206 pass / 73 unsupported / 1 skipped**, per directory unchanged from M1; 2 supplemental passes separately; 64 vendored schemas unchanged; all 280 fixture classes agree |
| Public matrix | **69 attempts = 23 cases × 3**: 48 pass, 6 intended property failures, 9 named defense refusals, 6 classifier controls; every case/repetition class agrees |
| Diagnostic matrix | **54** = 39 / 3 / 9 / 3, unchanged; every class agrees |
| `capacity_refusal_no_spawn` × 3 per environment | one refusal logged for `projection_records`, projected **32,772** (a durable submit adds 5 records); journal 4 facts, outbox 4 and 0 invocations before and after; 32,767 projection records; no spawn marker, process-table match or execution record; `inspect` → `not_found`; after the filler is removed, the same store spawns exactly **1** control child with correct dispatch ordering |
| Caller recovery / packaging | pass; launchd on macOS, systemd user manager on Linux |

Unit tests cover: exact byte and record limits and one over; refusal before staging, proven with abort triggers; growth through unchanged records; shrinking and no-op commits; subject, event and dedupe records counted by a real command; `events.retain_last` changing the result; a refused `execution.submit` creating nothing; the at-limit stall; queries and replays staying available on capacity refusal versus `unavailable` on a disk-fault trigger; and equality with the canonical length.

Five local mutants were each killed for their named reason: inclusive limit, check after staging, unchanged bytes not counted, events not counted, dedupe outcomes not counted. They were run on the workstation against the unit tests; they are not CI artifacts.

## Limits and findings

- **At the limit, admitted work can outrun the projection.** The dispatch marker adds no records and commits, so the durable host may launch the child; the next observation adds events and is refused. Delivery stalls at `pending` in the projection while the host's own facts, stored outside the projection, keep the evidence. Safety and ordering hold; liveness does not. Reserving room for already-admitted work would be a new design decision, not made here.
- **At or near the bound the service is too slow to use.** At 32,767 small records a commit costs about 200 ms in debug and about 30 ms in release on the workstation, and the durable tick commits once per execution every 25 ms. A debug service starves clients there. The process case avoids this deliberately by filling a store with no executions. The bound is a safety limit, not an operating point; tracked changed keys remain the prerequisite for any throughput claim.
- **No automatic eviction or GC.** Executions, effects, subjects and grants have no retention. A full store needs operator action. Recovery facts that do not fit fail startup explicitly.
- **Protocol gap:** there is no capacity-specific public error. A versioned upstream proposal would be needed; none has been filed. Protocol #9, #10 and the undeclared subscription authorization-recheck barrier remain coverage limits.
- **Not bounded:** journal, outbox and invocation history, the spool, total disk, throughput. A refused submit can leave one unreferenced content-addressed script object.

## Next

Qualify the selected Codex executable, version, hash and per-file canonical schema identity, and build before/after capture of the user's Codex configuration. Then connect the app-server through the durable host. Live runs use throwaway fixture repositories within the 1,000,000-token Codex bound.
