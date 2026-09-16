# PIO documentation

Read [repository scope](../README.md), then [execution spec](spec/SPEC.md) and [internal mechanics](spec/INTERNALS.md). These current specs are authoritative for this repository; they are not implemented APIs.

- [Development workflow](https://github.com/Combraton/combraton/blob/main/docs/DEVELOPMENT.md) — ownership, parallel work, reviews and fresh-session recovery.
- [Verification](VERIFICATION.md) — commands that actually exist and their limits.
- [Decision records](decisions/README.md) — accepted internal choices and supersessions.
- [Task work](work/README.md) — durable plans and handoffs.
- [Shared baseline](https://github.com/Combraton/combraton/blob/main/docs/architecture/BASELINE.md) — product ownership and invariants.
- [Publication provenance](https://github.com/Combraton/combraton/blob/main/docs/architecture/PUBLICATION.md) — source import and historical material boundary.

For cross-repository work, also read the affected public contracts: [PIO](https://github.com/Combraton/pio/blob/main/docs/spec/SPEC.md), [CBR](https://github.com/Combraton/cbr/blob/main/docs/spec/SPEC.md), and [Protocol](https://github.com/Combraton/protocol/blob/main/docs/spec/SPEC.md). Navigation links track main; task packets pin actual source revisions.

- [Standalone release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md) and [accepted sequencing decision](https://github.com/Combraton/combraton/blob/main/docs/decisions/001-standalone-first-and-evaluation.md).
- [PIO standalone client contract](https://github.com/Combraton/pio/blob/main/docs/spec/STANDALONE-CLIENT.md).
- [Cross-product benchmarks](https://github.com/Combraton/benchmarks).

## Active implementation

- [Current state](work/STATE.md) and [M2 task packet](work/m2/TASK.md) — active M2 Codex adapter work, [issue #5](https://github.com/Combraton/pio/issues/5).
- [M2 capacity-bound checkpoint](work/m2/COMMIT-BOUND.md) — ADR 002 projection limits, refusal behavior, two-platform evidence and findings.
- [Standalone 0.1 plan](work/standalone-0.1/PLAN.md) — scope, milestones and owner decisions, including the 2026-09-16 owner amendment.
- [Accepted stack](decisions/001-standalone-stack.md) — primary sources, exact harness candidates, validating experiments and the owner amendment on harness scope, spend bounds, Claude Code authentication and license.
- [Journey verification](JOURNEYS.md) — required observations; all runtime journeys are currently not evaluated.
- [Released Protocol boundary](work/standalone-0.1/PROTOCOL.md) — verified pin and exact optional CBR interactions.
- [Execution checkpoint](work/m1/EXECUTION.md) — journal-backed scripted conformance, exact-head evidence and remaining M1 work.
- [M1 acceptance corrections](work/m1/ACCEPTANCE-CORRECTIONS.md) — public ordering oracle, recovery facts, truthful discovery, packaging and current evidence.
- [Private packaging](../packaging/README.md) — install/uninstall layout and native user-job prototypes.
- [Historical M0 handoff](work/standalone-0.1/HANDOFF.md).
