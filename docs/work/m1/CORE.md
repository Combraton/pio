# Core implementation checkpoint

This continues [issue #3](https://github.com/Combraton/pio/issues/3) and [draft PR #4](https://github.com/Combraton/pio/pull/4) after the owner's review of `4bc8f37`. Base is `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; the correction commit is `38ca3847663d019dd4f97a554c68e97bd5821f19`. [STATE](../STATE.md) records the current tested head. M1 remains incomplete and unaccepted.

## Implemented surface

The independently implemented `pio-protocol` crate serves the pinned Unix stream binding through the explicit `pio conformance` entrypoint. It authenticates each socket session, negotiates once, validates unchanged released schemas, canonicalizes command intent, and applies authorization, replay, epoch and revision checks in the specified order. The conformance-only core-test profile supplies real commands for testing these semantics; it is not a production API or a native harness adapter.

One process owns the protocol store through a lifetime lock. One processing lock serializes authorization with committed state. SQLite FULL transactions persist subjects, bound command results and their events together. Launch controls simulate commit refusal and response loss; request envelopes cannot invoke these faults. Credentials are hashed for authentication and are not persisted in the protocol store or error messages. No shell or native adapter is launched by the Core service.

Grants implement bounded delegation, holder/issuer visibility, expiry, authority binding, cascade revocation and replay after loss of authority. Events implement durable positions, epoch changes, retention snapshots, visible-subject filtering, receive-size-aware reads, backlog/live subscriptions, and idle authorization rechecks on a shared Unix service. Capability changes retain revision and append provider-origin events. A lost capability refuses new work but preserves earlier command results.

The effect store has internal tests for unknown status, overdue waits, abort without changing outcome, replay after restart, and absent versus forgotten history. **Public `core.effects` remains unclaimed:** the pinned effect-producing and effect-lifecycle fixtures require Execution and `executor.script`. Core-test creates no external effects. Those fixtures must run at the next checkpoint before this claim is enabled. There is no invented public test operation or launch control that seeds domain state.

## Pinned-runner results

Implementation progressed in the requested order: stream/authentication/negotiation/core-test, then grants (124 passes), events, capabilities, then the internal effect store. The final local run reports:

| Fixture directory | Pass | Unsupported | Skipped |
|---|---:|---:|---:|
| core | 130 | 5 | 0 |
| stream | 24 | 0 | 0 |
| socket | 10 | 3 | 0 |
| Other directories | 0 | 107 | 1 |
| Total | 164 | 115 | 1 |

No `fail`, `timeout` or `harness_error` outcomes. The requested Core/stream/socket set is 172 fixtures: 164 pass and 8 are unsupported. Four Core backpressure fixtures and the Core feature-dependency composition fixture need Execution. Two socket lifecycle/obligation fixtures also need Execution. `socket.subscription-recheck-race-regression` needs the undeclared `subscription.recheck.after_authorization` barrier. Idle expiry and revocation across sessions do run and pass; they do not substitute for that deliberately paused race fixture.

The descriptor declares `clock.file` and `store.faults`, no test barriers and no scripted executor. Execution, public effects and `core.events.backpressure` stay unclaimed. Dedupe advancement/retention, event epoch/retention, fixed/file clocks, capability overrides and receive limits come only from the released launch configuration. The full runner manifest retains each unsupported reason; nothing is reclassified as a pass.

## Correction evidence

The fake-host matrix now has 18 cases, each repeated three times: **54 attempted, 39 positive passes, 3 intended J3 count-property failures, 9 expected layer refusals and 3 expected wrong-reason classifier failures**. The three layer mutants bypass admission replay suppression, launch guard and host phase respectively; each must be refused by the next defense with its exact named reason. The J3 marker-count mutant remains a separate property check.

The fenced-release case journals `known_not_released` only after killing/reaping the unreleased child and observing no release marker. Released but unobservable work remains uncertain. The read-only-store case changes filesystem permissions and independently observes no child spawn markers; the request-field query-only simulation remains fake-only.

CI on the correction commit exposed an evidence-collection race: a reaper removed a transient attempt `.pending` file between glob and copy. Collection now copies only durable attempt JSON receipts and stderr, while all behavior assertions remain intact. PR checkout and artifact names use the branch head SHA.

## Reproduction and remaining work

Run the single sequence in [VERIFICATION](../../VERIFICATION.md#m1-build-and-conformance) from a clean clone. It builds the runner from freshly downloaded, verified pinned assets and uploads descriptor, schema verification, native manifest/transcripts, per-directory report and matrix evidence on both platforms. No sibling repository or private local state is required. Unit tests, Clippy, documentation checks and the matrix are separate checks from Protocol conformance. [Both-platform CI](https://github.com/Combraton/pio/actions/runs/35079691494) and a fresh clone reproduced the results at `642c56e`; the [receipt index](evidence/core-checkpoint.json) records artifact names, source/binary digests and per-directory classes. A follow-up adds a negotiation regression test for overlapping feature lists and duplicate-profile error precedence.

Next: implement the PLAN Execution subset and scripted executor, connect the verified Core transaction path to process ownership, and exercise public effects, backpressure and remaining controls. Durable caller operations and content-addressed payloads also remain. No real adapter has run; no native credential/configuration has changed. M3 must prove the API/provider authentication source and refuse missing configured credentials even when a cached login exists. M1 merge, tag and publication remain unauthorized.
