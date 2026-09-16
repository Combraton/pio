# M1 acceptance corrections

Owner: Codex. Base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; tested implementation head **`7b05342cbcaaa1813b2da02cf6bdb01a1ddc2673`**, branch `codex/m1-core-host`. One implementation worktree, no sibling writes. This documentation successor records the verified implementation; its own commit is available in Git/PR history. [Issue #3](https://github.com/Combraton/pio/issues/3), [draft PR #4](https://github.com/Combraton/pio/pull/4).

The owner reproduced the previous public-host checkpoint at `baf2fae` and requested five bounded corrections before M1 acceptance. All five now have implementation and clean-clone/two-platform evidence. **This is an M1 acceptance candidate; owner acceptance is pending.** No merge, tag or publication is authorized here.

## Changes and direct evidence

1. **Journal ordering.** A read-only oracle correlates each execution's delivery effect to the deterministic host command identity. It requires the first `protocol.commit` carrying `dispatch_intent` to precede `invocation.intent` by journal sequence. Ordinary dispatch cases require exactly one pair, with no unmatched dispatch. In the clean-clone first repetition, normal order is **4 < 5**; the actual reorder mutant produces **5 > 4** and fails specifically on `dispatch_intent_must_precede_invocation_intent`. The wrong-reason control exits the daemon at cut 91 after correctly ordered admission; the classifier rejects that crash as an ordering kill. The new cut exits **95** after marker commit but before host admission. It preserves the marker with no invocation intent, no spawn marker and no matching OS process. Restart records ambiguity and never admits/spawns automatically. Per-case `journal-order.json` includes actual journal sequence numbers and facts.
2. **Process discovery.** `execution.discovery.list` reports the built-in labeled fake process: detected true; recognized, version-supported and reachable yes; native authentication unknown, therefore `usable: false`. This identifies the loaded fake adapter/live local controller, not a native harness probe. The authenticated public query is schema-checked and spawns no child. Caller authentication never substitutes for harness authentication.
3. **Packaging skeleton.** [Private layout and user-job prototypes](../../../packaging/README.md) provide exclusive install, default `pio-standalone` alias, PATH/destination collision refusal, manifest and binary integrity checks, and ownership-checked uninstall. Launchd on macOS and systemd user manager on Linux actually start and stop the service in CI. Artifacts include generated job, install manifest, kernel start identity, authenticated discovery, stop observation and command receipts. Seeded unrelated `pio` retains inode/hash; modified binary and manifest refuse removal; config/state/unowned additions survive uninstall. No persistent test job remains. This is not a release updater: interruptions or later I/O failure can leave partial installation/removal, as documented.
4. **Restart facts.** Public process restart persists `execution.recovery.decided` before `execution.host.changed`, with advanced controller generation. The surviving child retains its kernel identity and slot. Pending marker-present delivery remains ambiguous while a parked child has no release evidence. The fenced-release case reads the actual `core.events.read` result and verifies reconciliation to `not_delivered` after confirmed release absence.
5. **Delivery evidence.** `child_release_marker` is the distinct evidence class for the child-written release observation. It establishes `delivered`, not a native harness acknowledgment or comprehension. `release_absence_confirmed` and `release_not_observed` are separate evidence classes; recovery-state names are not delivery classes. The frozen optional proof enum is unchanged. Reconciliation event outcomes remain `delivered | not_delivered | unknown`; a unit regression checks their persisted history after journal rebuild.

Read-only Sol review caught and prompted fixes for ambiguous-to-pending regression, vacuous ordering acceptance and the reconciliation-event outcome mismatch. Scoped re-review found no remaining defect in those corrections. Packaging review prompted manifest-integrity checking and explicit non-transactional limits. Source review is separate from runtime receipts.

## Exact-head verification

[CI 35096491269](https://github.com/Combraton/pio/actions/runs/35096491269) succeeds at `7b05342` on **macOS 15 arm64** and **Ubuntu 24.04 x86_64**. An independent fresh HTTPS clone at the same head completes the entire [documented command sequence](../../VERIFICATION.md), with fresh stores and freshly downloaded release assets, exit 0. The [receipt index](evidence/acceptance-corrections.json) records source/binary identities, artifact digests and three-environment comparisons.

Twenty Cargo tests, fmt, locked build, Clippy with warnings denied, docs and lockfile checks pass. All 64 vendored schemas match the pinned release inventory. The runner is built from the verified release archive. All 280 official fixture classifications agree across the clone and both CI artifacts. Two supplemental fixtures pass separately.

| Runner directory | Pass | Unsupported | Skipped |
| --- | ---: | ---: | ---: |
| core | 134 | 1 | 0 |
| stream | 24 | 0 | 0 |
| socket | 12 | 1 | 0 |
| execution | 36 | 15 | 0 |
| composition | 0 | 14 | 0 |
| context | 0 | 11 | 0 |
| evidence | 0 | 16 | 0 |
| knowledge | 0 | 10 | 0 |
| verification | 0 | 5 | 0 |
| compat | 0 | 0 | 1 |
| **Official total** | **206** | **73** | **1** |

Zero fail, timeout or harness_error. Unsupported reasons remain in each report/manifest: unclaimed product profiles and Execution features; Core's all-profile dependency fixture ([Protocol #9](https://github.com/Combraton/protocol/issues/9)); socket's undeclared authorization-recheck barrier. The compatibility fixture targets a different pinned participant. Unsupported is never pass.

| Matrix | Attempts | Positive pass | Intended property failure | Named defense refusal | Wrong-reason classifier control |
| --- | ---: | ---: | ---: | ---: | ---: |
| Public Unix API, 22 cases × 3 | **66** | 45 | 6 | 9 | 6 |
| Diagnostic tooling, 18 cases × 3 | **54** | 39 | 3 | 9 | 3 |

Every case/repetition class matches across all three environments. Public requests remain authenticated socket operations; independent witnesses read the journal and OS kernel. J3 still fails on actual child identity/count, distinct from the new ordering mutant. Caller recovery passes with exactly one child and digest-only journal output references. The packaging checks use launchd and systemd respectively; they do not substitute direct launches.

## Issue #3 acceptance mapping

| Acceptance item | Current evidence / limit |
| --- | --- |
| Pinned workspace, toolchain, lockfile, builds on both platforms | Clean clone and exact-head CI; 20 tests and required checks |
| Own participant, exact PLAN features/test controls, verified pinned runner | Scripted participant; native runner directory/classes and explicit unsupported limits above |
| Labeled process host and repeated fault matrix | Public Unix API, 66 attempts; diagnostic tooling separately 54 |
| Dispatch-intent before host invocation intent | Journal-sequence oracle, actual reorder mutant, crash cut 95 and wrong-reason crash control |
| J3 plus three defense layers | Public launches, independent markers, exact named property/refusal reasons |
| No spawn after journal failure | Public launch-config store fault plus independent markers/process-table snapshot; real read-only-store startup fault separately |
| Detach/restart/reattach and lost host | Public API, same kernel process identity/slot, before/after controller generation, no respawn, persisted recovery/host-change events |
| Durable caller operation store | Public reconciliation and exact original-command replay; one child after lost response |
| Content-addressed output | Actual fake-child pipe capture, immutable spool, digest/offset/lost ranges, corruption unit check |
| Process-mode discovery | Labeled built-in fake installation; native authentication unknown, usability false |
| Packaging skeleton | Collision-safe layout/uninstall checks; actual launchd/systemd user-job start and stop in CI |
| No real adapter and M3 authentication carry-forward | Explicit limitation; distinguish API/provider credential source and refuse no-credential runs despite cached login in M3 |
| State, Protocol gaps and sibling isolation | This report and STATE; Protocol #9/#10 with demonstrations; frozen schemas unchanged; no sibling writes |

No required process case is proven only by the diagnostic daemon. Scripted conformance claims remain distinct from process behavior: process cancellation/workspace/usage adapters remain explicitly unavailable. Detailed PID/start/slot/controller witnesses remain outside the frozen public result schema ([Protocol #10](https://github.com/Combraton/protocol/issues/10)).

No real harness, native authentication, end-to-end journey, production throughput, total-disk bound or release qualification is proven. All six product journeys remain `not_evaluated`. Before M2, implement ADR 002's 32 MiB / 32,768-record commit bound; M2 canonical JSON drift and Codex trust-config side effect and M3 credential-source/negative-control requirements remain. No M2 work starts in this checkpoint.

## Resources and handoff

All test-owned processes/user jobs were stopped; temporary clones, stores and evidence remain outside Git for diagnosis. Read-only helpers completed. Next: owner review of the recorded head/receipts and M1 acceptance decision. PR remains draft; no merge/tag/publish performed. The earlier [public-host checkpoint](PUBLIC-HOST.md) and its receipt index remain historical evidence.
