# Execution checkpoint

Work for [issue #3](https://github.com/Combraton/pio/issues/3), [PR #4](https://github.com/Combraton/pio/pull/4), base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`. [ADR 002](../../decisions/002-protocol-journal.md) was recorded in `c6b5b8f` before Execution code; the Core journal migration is `5f2e386`.

## Scripted host and transaction path

`executor.script` selects deterministic fake-host inputs only from the pinned runner launch configuration. Its source is `fake-host/executor.script`; the descriptor, internal execution journal records and public evidence identify that source. This conformance adapter models host observations; it is not a native process adapter or evidence that the polling service is a durable host. The separate lower-level matrix remains the process-identity/survival evidence.

Admission commits execution state, delivery effect intent, acknowledgment/replay result and event together in the `pio-core` journal. Script progress and dispatch generation are durable per-execution records. A write-ahead dispatch commit precedes delivery observations; response loss and restarts preserve identities. Cancellation commits its request and forwarding intent before observing an outcome. Reconciliation reads observations and never resubmits. Completion conflicts and superseded generations remain separate observations. Only a failed-before-delivery determination releases an unresolved reservation.

The five PLAN features implement controller epochs, separate discovery facts, bounded output spool/loss ranges, workspace probe coverage and attributed usage/liability. All native discovery and workspace inputs here are scripted, not probes of the user's installation or checkout. Optional context, actions, steering and continuation remain unsupported.

## Evidence before backpressure

At parent `5f2e386` plus this implementation, the pinned runner's 51 Execution fixtures report **36 pass, 15 unsupported, zero fail/timeout/harness_error**. The unsupported fixtures require features outside PLAN's selected subset. Artifact: `/tmp/pio-execution-third/manifest.json` and transcripts. Public `core.effects` is claimed only after those effect-producing and lifecycle fixtures ran, including unknown outcomes, retry classes, abortion, authority filtering and restart/replay. Twelve Cargo tests and Clippy pass; the new test verifies same-transaction admission records, failure rollback, one attempt across restart/replay and reconstruction from the journal.

Next: bounded per-connection event output and the seven Execution-dependent Core/stream/socket cases. Full per-directory and both-platform CI receipts follow. M1 remains incomplete and unaccepted. No real adapter claim.

## Bounded event output and coverage boundary

The connection writer bounds pending output. A full queue starts a fixed room deadline; partial progress never extends it. A slow negotiated consumer gets ending notices within one shared budget, then the connection closes. Older consumers close without that new notice. Events remain in the journal and reconnect uses the caller's durably processed cursor. A normal draining-consumer test verifies frames stay ordered without forced closure.

All four frozen backpressure cases pass locally, and sockets report 12 pass / 1 unsupported. The requested seven remaining cases have a scope correction: six fit PIO, while `core.feature-dependencies-match-negotiation` also requires evidence, context, knowledge, verification and `execution.context`. It remains officially unsupported. [Protocol issue #9](https://github.com/Combraton/protocol/issues/9) contains a demonstrating component-scoped fixture; `conformance/regressions/` runs it separately with the pinned runner. Its pass is never counted among the 280 release fixtures. `socket.subscription-recheck-race-regression` still requires an undeclared barrier.

The descriptor is now `pio-journal-fake-executor`. Full exact-head receipts follow in this record; the earlier Core handoff is historical. No real adapter claim.

## Full-suite verification and self-review

At `c47786063129f995311d1994ebf5bfe937a3edc9`, the clean-clone sequence and [both-platform CI](https://github.com/Combraton/pio/actions/runs/35085521766) report **206 pass / 73 unsupported / 1 skipped**, with zero fail/timeout/harness_error. Core: 134 pass / 1 unsupported; stream: 24 pass; socket: 12 pass / 1 unsupported; Execution: 36 pass / 15 unsupported. Other profiles remain outside scope. The official Core/stream/socket set is now **170/172 passes**. The one supplemental dependency fixture passed separately. The process matrix remains 54 attempts with 39 positive passes, 3 intended count-property failures, 9 expected defense refusals and 3 intended classifier failures.

Self-review then added regression checks for independent delivery/inactivity timers, cancellation effect payload identity and idempotency key before dispatch, and exactly one attempt when a scripted crash follows a dispatch already awaiting evidence. Fifteen Cargo tests and two supplemental runner fixtures pass locally. The supplementary crash fixture is separate from the frozen release suite. Unsupported adapter launch controls are explicitly refused. Final exact-head CI receipts follow; neither successful conformance nor this self-review grants M1 acceptance.
