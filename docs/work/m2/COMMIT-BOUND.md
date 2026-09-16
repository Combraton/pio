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

## Owner review 1 and corrections (2026-09-16)

The owner reviewed PR #6 at `0f8685b` and reproduced it from a clean clone: 28 tests, public matrix 69 with the capacity case passing, diagnostic matrix 54, caller recovery, and the runner at 206 / 73 / 1 with two supplemental passes. The owner's own check-after-staging and off-by-one mutants were killed by the intended unit tests. **Verdict: continue.** The report above is left unchanged as the record of `5be40f9`. The decisions were implemented as follows; tested head is **`6a9d11a344e50de45a2e60d441026a90cad80f86`**, and receipts are in [review-1-corrections.json](evidence/review-1-corrections.json).

1. **Admission headroom (bench values).** A new `execution.submit` commit must also stay within **31,130 records** and **31,876,710 bytes** (95 percent of the hard limits). Observations, recovery facts and every other commit keep the hard limits, so admitted work can finish in the remaining room. See [ADR 002](../../decisions/002-protocol-journal.md#capacity-bound-2026-09-16). New exact-boundary unit tests; the public case `capacity_headroom_admitted_completes` admits one durable submit exactly at 31,130 and refuses a second submit at `admission_projection_records`. The admitted child is delivered and exits 0 while the projection grows past 31,130, with no hard-limit refusal and no stall. `capacity_refusal_no_spawn` now expects the submit to hit the admission threshold and shows the hard limit through a public `execution.controller.claim`.
2. **Incremental commits** that process only changed keys are recorded as a prerequisite in PLAN's M3b and M4 rows. No throughput claim.
3. **[Protocol #13](https://github.com/Combraton/protocol/issues/13)** filed, with `conformance/proposals/capacity-refusal.json` and a Cargo test showing the capacity and transient-commit errors are identical under the frozen schema. PIO does not wait on it.

**Two intermediate CI failures, both fixed, with evidence kept.**

- **`aa7ad78`:** the older capacity case still expected the hard-limit reason. After adding headroom, only the new case had been rerun before committing. Fixed in `df069bf`, and the full matrix is now run locally before each commit.
- **`df069bf`:** the headroom case exceeded the harness's fixed 3-second socket timeout on CI after the service had already logged the refusal. Near the bound every request pays whole-state commits. `b217936` spaces slow ticks (`MissedTickBehavior::Delay`), gives the two capacity cases a 60-second client timeout and records latency.

**Evidence at `6a9d11a`.** [CI 35130616765](https://github.com/Combraton/pio/actions/runs/35130616765) passes on macOS 15 arm64 and Ubuntu 24.04 x86_64 for push and pull request, and a fresh clone ran all 14 documented commands with exit 0.

| Check | Result (identical in clean clone, macOS CI and Linux CI) |
| --- | --- |
| Cargo | **31** tests pass, 1 ignored probe |
| Runner | 206 pass / 73 unsupported / 1 skipped, 2 supplemental passes; all 280 fixture classes agree |
| Public matrix | **72** = 24 cases × 3: 51 pass, 6 intended property failures, 9 defense refusals, 6 classifier controls |
| Diagnostic matrix | **54** = 39 / 3 / 9 / 3 |
| Capacity cases | both pass ×3 in every environment |

Measured headroom-case latency for the refused submit: 1.6 s in the clean clone, 2.2–3.5 s on macOS CI, 3.0 s on Linux CI, and 4.9 s under deliberate local CPU contention. Three local headroom mutants were killed: threshold never applied, threshold applied to every commit, and an inclusive boundary.

Remaining limits are unchanged from above, except that a new submit can no longer cause the at-limit stall; only facts from already-admitted work can still exhaust the hard limit. 0 live tokens; no real harness has run.
