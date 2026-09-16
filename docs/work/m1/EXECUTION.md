# Execution checkpoint

Work for [issue #3](https://github.com/Combraton/pio/issues/3), [PR #4](https://github.com/Combraton/pio/pull/4), base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`. [ADR 002](../../decisions/002-protocol-journal.md) was recorded in `c6b5b8f` before Execution code; the Core journal migration is `5f2e386`.

## Scripted host and transaction path

`executor.script` selects deterministic fake-host inputs only from the pinned runner launch configuration. Its source is `fake-host/executor.script`; the descriptor, internal execution journal records and public evidence identify that source. This conformance adapter models host observations; it is not a native process adapter or evidence that the polling service is a durable host. The separate lower-level matrix remains the process-identity/survival evidence.

Admission commits execution state, delivery effect intent, acknowledgment/replay result and event together in the `pio-core` journal. Script progress and dispatch generation are durable per-execution records. A write-ahead dispatch commit precedes delivery observations; response loss and restarts preserve identities. Cancellation commits its request and forwarding intent before observing an outcome. Reconciliation reads observations and never resubmits. Completion conflicts and superseded generations remain separate observations. Only a failed-before-delivery determination releases an unresolved reservation.

The five PLAN features implement controller epochs, separate discovery facts, bounded output spool/loss ranges, workspace probe coverage and attributed usage/liability. All native discovery and workspace inputs here are scripted, not probes of the user's installation or checkout. Optional context, actions, steering and continuation remain unsupported.

## Evidence before backpressure

At parent `5f2e386` plus this implementation, the pinned runner's 51 Execution fixtures report **36 pass, 15 unsupported, zero fail/timeout/harness_error**. The unsupported fixtures require features outside PLAN's selected subset. Artifact: `/tmp/pio-execution-third/manifest.json` and transcripts. Public `core.effects` is claimed only after those effect-producing and lifecycle fixtures ran, including unknown outcomes, retry classes, abortion, authority filtering and restart/replay. Twelve Cargo tests and Clippy pass; the new test verifies same-transaction admission records, failure rollback, one attempt across restart/replay and reconstruction from the journal.

Next: bounded per-connection event output and the seven Execution-dependent Core/stream/socket cases. Full per-directory and both-platform CI receipts follow. M1 remains incomplete and unaccepted. No real adapter claim.
