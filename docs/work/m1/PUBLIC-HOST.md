# Public durable-host checkpoint plan

Owner: Codex. Base `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; starting head `7a4659f423737a5341e198dd779392e54b797458`, branch `codex/m1-core-host`. One implementation worktree; siblings read-only.

The owner accepted ADR 002 and the 206/73/1 scripted checkpoint. M1 is incomplete. Implement in order:

1. Public Unix Execution submits committed intent to `pio-host`; its detached fake child, launch guard, slot lock and kernel identity supply actual observations. Keep `executor.script` a conformance launch control.
2. Reproduce process/storage matrix through authenticated public commands and queries. Keep independent markers/process-table evidence, named mutant refusal classification, and restart generations. Diagnostic commands remain test tooling only.
3. Store output bytes in a content-addressed spool; journal references carry digest/offset/lost ranges.
4. Add a separate bounded caller-operation store, durable before send, with restart reconciliation using the original command identity.
5. Document whole-state diff cost and the pre-M2 bound in ADR 002.

Acceptance: Cargo checks, full pinned runner with directory classes and separate supplemental counts, public matrix attempted/repetition/outcome evidence, client lost-response recovery, no output arrays in journal, clean-clone and two-platform CI. Map issue #3 items to public or diagnostic evidence. No real harness, authentication, adapter or end-to-end claim; all six journeys remain `not_evaluated`.


## Implementation checkpoint

The first complete public matrix (18 attempts, one repetition) passed all expected classes: 13 pass, one intended identity/count failure, three named defense refusals and one wrong-reason classifier control. The CAS/client check then passed: durable request before unavailable endpoint I/O, no spawn on empty reconciliation, lost-success-response reconciliation with exactly one child, actual output and observed exit, output digest references rather than bytes in journal. Cargo has 18 passing tests; Clippy passes. These are local development receipts, not yet the final exact-head CI claim.

A read-only MiniMax M3 inventory completed (13,798 tokens, $0.004564); its class descriptions were checked against code rather than adopted as new Protocol contracts. A scoped Sol code review identified pre-release dead-owner recovery, spawn cleanup and evidence provenance problems; all three were fixed and re-reviewed. Dead/fenced pre-release owners now journal known-not-released; an unrecorded child remains blocked behind the release pipe. Full matrix rerun follows those corrections.

[Protocol #10](https://github.com/Combraton/protocol/issues/10) records the typed process witness gap. `conformance/proposals/process-identity.json` and its Cargo test demonstrate rejection by the frozen schema; this is not counted as a conformance pass. All public matrix results are separately schema-validated.

## Issue #3 acceptance mapping

| Item | Evidence path at this checkpoint |
| --- | --- |
| Pinned workspace/toolchain/lockfile, both platform builds | Existing CI; new exact-head run required below |
| Core/Execution subset and pinned runner artifact classes | Scripted conformance participant; full rerun required below |
| Labeled process host and attempted/repeated fault matrix | Public Unix submit/inspect/reconcile; independent journal/kernel witnesses |
| J3 identity/count negative control and three defense layers | Public-triggered launches; independent spawn markers and named host refusal receipts |
| No spawn after failed commit | Public submit under launch-config store fault, plus markers and independent process-table snapshot |
| Store-level fault | Actual read-only SQLite files/directory prevent service startup; independent process-table/marker absence |
| Detach/restart/reattach and lost host | Public API, correlated kernel start identity, host slot and controller generations in witness artifacts |
| Caller operation durability | Public reconciliation and exact original-command replay from separate caller ledger |
| Content-addressed output | Real fake-child pipe bytes -> immutable spool -> digest/offset projection; corruption test and public output read |
| Native adapters/authentication and journeys | Not proven; M2/M3 separate, all six journeys `not_evaluated` |

The diagnostic JSON daemon remains test tooling. No required process case relies exclusively on it now. Detailed PID/start/slot/controller witnesses remain outside frozen public result fields, and runtime cancellation/workspace/usage support in the process mode remains explicitly unavailable. M1 remains incomplete pending owner review and the broader acceptance contract.
