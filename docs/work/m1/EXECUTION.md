# Execution checkpoint

Work for [issue #3](https://github.com/Combraton/pio/issues/3), [draft PR #4](https://github.com/Combraton/pio/pull/4). Base: `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`. Tested implementation head: **`245856a43426422b4271f32156a9e5a08c489ba9`**. M1 remains incomplete and unaccepted; no real adapter claim.

## Storage decision and service boundary

[ADR 002](../../decisions/002-protocol-journal.md) was committed in `c6b5b8f` before Execution code. The choice was to move Core onto the `pio-core` journal, avoiding separate stores for authorization/replay and effect admission. The migration in `5f2e386` passed ten tests and reproduced the verified Core checkpoint's 164/115/1 runner result before Execution.

Core subjects, command outcomes, effects, event metadata and individual events are journal-backed records. The service lifetime lock owns one writer; an immediate SQLite transaction and cache-revision fence commit changed projections, append-only journal facts and the outbox together. Rebuild replays those facts, including retention deletions. Rollback reloads committed projections. The old `protocol.sqlite3` blob is explicitly refused, not silently imported or ignored. The provider still caches and scans retained records; bounded total provider memory and production throughput are not established.

The mutex-guarded provider and 25 ms per-connection polling remain a **conformance service loop**, not the durable-host design. `executor.script` models host observations from the runner launch configuration. Its source is `fake-host/executor.script`, visible in execution journal records and public evidence; the descriptor is `pio-journal-fake-executor`. The separate lower-level fake-process matrix supplies kernel process identity/survival evidence. The scripted participant does not turn that into proof that public Execution is integrated with a surviving native process host.

## Implemented behavior

Admission commits execution state, delivery effect intent, acknowledgment/replay result and event together. A write-ahead dispatch commit precedes scripted delivery; response loss and restart preserve identities. Cancellation commits its forwarding payload digest and idempotency key before attempting, and its request receipt remains distinct from the observed outcome. Reconciliation reads observations and never resubmits. Completion conflicts and superseded generations remain separate observations. A timeout never proves that work ended or releases unresolved liability.

The five PLAN features implement controller epochs, separate discovery facts, output byte offsets/loss ranges, workspace probe coverage and attributed usage/liability. Discovery and workspace inputs here are scripted, not probes of the user's installation or checkout. Unsupported adapter/script controls refuse startup. Context, actions, steering and continuation remain unclaimed.

Public `core.effects` was claimed only after the effect-producing and lifecycle Execution fixtures passed. Bounded event output followed: a full connection queue starts a fixed room deadline; partial progress never extends it. Slow negotiated consumers get ending notices within one shared budget, then closure. Older consumers close without the new notice. Stored semantic events remain available from the caller's durably processed cursor, with explicit retention gaps. A draining-consumer test verifies ordered delivery without forced closure.

## Reproduced evidence

The [receipt index](evidence/execution-checkpoint.json) records downloaded artifact names, source/binary/manifest digests, environment and schema verification. [Both-platform CI](https://github.com/Combraton/pio/actions/runs/35086098791) and a clean checkout in an independent clone reproduced the documented command sequence, with fresh release downloads and fresh test stores, at `245856a`. Every official fixture outcome matches across all three runs.

| Frozen fixture directory | Pass | Unsupported | Skipped |
|---|---:|---:|---:|
| Core | 134 | 1 | 0 |
| Stream | 24 | 0 | 0 |
| Socket | 12 | 1 | 0 |
| Execution | 36 | 15 | 0 |
| Composition | 0 | 14 | 0 |
| Context | 0 | 11 | 0 |
| Evidence | 0 | 16 | 0 |
| Knowledge | 0 | 10 | 0 |
| Verification | 0 | 5 | 0 |
| Compatibility | 0 | 0 | 1 |
| **Total** | **206** | **73** | **1** |

Zero fail, timeout or harness_error outcomes. Core/stream/socket: **170/172 passes**. Fifteen Cargo tests and Clippy pass. All 64 vendored schema files match the verified release archive byte for byte; transitive common definitions do not claim their profiles.

The fake-process matrix remains **54 attempted**: 39 positive passes, 3 intended J3 count-property failures, 9 exact defense refusals and 3 intended wrong-reason classifier failures. It retains independent no-spawn observations, known-not-released evidence, and process start identity/host slot/controller generation across detach, restart and reattach. Those are separate evidence from Protocol conformance.

Two **supplemental PIO fixtures** pass under the same pinned runner in a separate manifest: Execution-only Core dependencies, and a crash after an existing pending dispatch retaining exactly one attempt. They are never added to the official 280-fixture count. Unit regressions also verify independent delivery/inactivity timers and cancellation intent identity before forwarding.

## Explicit coverage limits and next work

Six of the requested seven Execution-dependent Core/socket cases run and pass. `core.feature-dependencies-match-negotiation` additionally requires Evidence, Context, Knowledge, Verification and `execution.context`; it remains officially unsupported. [Protocol issue #9](https://github.com/Combraton/protocol/issues/9) contains the demonstrating component-scoped fixture. The original fixture and all frozen schemas remain unchanged. `socket.subscription-recheck-race-regression` remains unsupported because `subscription.recheck.after_authorization` is undeclared. The manifest preserves every other unsupported/skipped ID and reason.

M1 still needs durable caller operation storage, content-addressed payloads, public Execution integration with surviving process ownership, and the remaining process/storage certification. Native adapters, native authentication and end-to-end release journeys remain later work. M3 must distinguish the actual API/provider authentication source and refuse missing configured credentials even with a cached login present. No native credential or configuration changed; sibling repositories remain untouched. Merge, tag and release are not authorized here.

A read-only MiniMax M3 contract audit completed (28,170 tokens, $0.012545). A bounded code review was stopped without a final report; one narrow retry confirmed only the shared notice-deadline placement (5,070 tokens, $0.001124). This is not an independent code-review verdict. No helper remains active.
