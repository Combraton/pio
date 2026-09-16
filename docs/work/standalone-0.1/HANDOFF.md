# Standalone 0.1 readiness handoff

Updated 2026-09-16. Owner: Codex, PIO implementation lead. Task: [issue #1](https://github.com/Combraton/pio/issues/1). Branch: `codex/standalone-readiness`; base `e65b7c02318e71e848ab7c8b3f8efab3489fb2d2`. Read the branch/PR's actual head on resume; the readiness documents do not claim their own future commit ID.

Reviewable delivery: [draft PR #2](https://github.com/Combraton/pio/pull/2). The substantive readiness commit is `de05afb4217ea47dec1fc445ddbf67790cdfd6f0`; the follow-up links the created PR. No merge, tag or release was performed.

## Reconciled state

PIO began clean on main, matching remote main, with one worktree and no source runtime, package manifest or product build command. No PIO issues or PRs existed at inspection. Root AGENTS/CLAUDE, README/doc map, STATE, all three PIO specs and verification were read. This branch owns only PIO changes; no concurrent PIO writer was observed. CBR is independently owned and was not modified.

| Source | Inspected revision | Use |
|---|---|---|
| PIO | `e65b7c02318e71e848ab7c8b3f8efab3489fb2d2` | Accepted execution/core/client boundaries and original checkout |
| Combraton | `9af69ce966bfacf0deb03606d99f28a355d1f944` | Root instructions, DEVELOPMENT, STANDALONE-RELEASES, BASELINE and shared VERIFICATION |
| Protocol release | `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc` / `v0.1.0` | Verified normative handoff, profiles, schemas, fixtures and inventory |
| Protocol local main | `71ed2c4fcf79c35e6b5c7125f8d15b8925345f4a` | State reconciliation only; not the pin |
| CBR | `32783934b4e52d38a67f4bcc770f16e14b5993e3` | Clean main observed; no CBR file read or changed and no runtime conclusion |
| Benchmarks | `c8d5878ab655d090942ad613cd932b44b5e62929` | Shared-link revision; no comparative evaluation |

Protocol [issue #1](https://github.com/Combraton/protocol/issues/1) is closed; [release PR #7](https://github.com/Combraton/protocol/pull/7) and [state PR #8](https://github.com/Combraton/protocol/pull/8) are merged. The remote tag and release manifest identify the requested commit. This supersedes the older PIO STATE's pre-release observation. The historical Downloads corpus was not used as implementation authority.

## Delivered and pending

Delivered: [release plan](PLAN.md), [proposed stack ADR](../../decisions/001-standalone-stack.md), [journey matrix](../../JOURNEYS.md), [public integration map](PROTOCOL.md), [Protocol lock](../../../protocol.lock.json), release verifier and sanitized [inspection evidence](evidence/readiness.json). Entry points link them. No substantial product implementation or dependency installation occurred.

Pending owner judgment: stack/host and Claude bridge approach; two-adapter/platform scope; supported Claude authentication. Confirmatory thresholds and license are later reserved decisions. Codex 0.146.0 and Claude 2.1.273 are validation candidates, not supported releases. Authentication and native behavior remain unknown. The installed historical `pio` is not this product; avoid PATH collision during installation.

## Checks and limits

| Command / observation | Actual result | Scope |
|---|---|---|
| Git status/HEAD/worktrees and remote main | Clean matching PIO base; one worktree | Source reconciliation |
| Remote Protocol tag/peeled ref and source tree | Exit 0; requested commit/tree match | Tag identity at inspection |
| `gh release view/download`; tagged-commit `gh run view` | Exit 0; upstream CI completed/success | Release/upstream evidence; no local conformance rerun |
| `python3 scripts/verify_protocol_pin.py --assets "$PIO_RELEASE_DIR"` | Exit 0; six assets, 539 bundle files, 420 normative entries | [Release integrity](evidence/pin-checks.json), not product correctness |
| Verifier with one space appended to temporary manifest | Exit 1, expected manifest checksum mismatch | Intended negative control |
| Codex version/help/schema generation | Exit 0; 0.146.0; 275 generated JSON files | Interface inspection; hash listing retained; no authenticated job |
| Historical dispatcher discovery; direct Claude version/help | Discovery 2.1.270, direct 2.1.273; binary hash recorded | Drift observed; cause/authentication unknown |
| Read-only MiniMax contract review | Done, exit 0; 45,651 tokens, reported USD 0.009530 | Shared-source review, not independent runtime evidence |

Final documentation and diff checks are in [validation evidence](evidence/validation.json). All J1–J6 properties remain `not_evaluated`. No native permission/cancellation/recovery or real CBR composition was exercised. No model, permission defaults, credentials or global hooks were changed.

The helper's unrelated Knowledge/Verification-client expansion was rejected; the user's exact six journeys govern. One verifier error was fixed: macOS resolves `/tmp` to `/private/tmp`, so containment must compare canonical paths. Positive and tamper-negative checks then passed. Released artifacts were never changed. A historical `pio --version` probe is unsupported by that tool; it supplied no version evidence.

## Resources and continuity

No ORC session/pane environment was set. One read-only `pio run` worker (`20260916-100652-read-only-pio-implementa-031a`) completed; none remains running for this task. Post-review quota reported 99% five-hour and 95% weekly remaining. The large raw worker log stays private; only verified findings/usage are retained.

Existing Codex/Claude/Hermes processes were observed and left untouched. No process from this checkout, service, host or listening endpoint was started. Temporary release/source/schema caches are disposable; durable pins and evidence are in PIO. There is no product resource to clean up.

The attached kickoff is user source material and stays intact. This handoff preserves its useful outcomes and remaining work. No owned disposable kickoff was found in PIO. The workspace Protocol kickoff belongs to its owner and was neither edited nor retired. Its status is not decided by this task.

## Next action

Settle the three initial owner decisions in the plan; record answers before M1. Then implement core/host and packaging experiments, inspect live resources again, and present M1 with actual evidence. Preserve failed experiments and unsupported full-release obligations. Do not install over the historical dispatcher, merge, tag or publish without the applicable explicit authority.
