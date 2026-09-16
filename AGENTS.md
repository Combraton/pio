# PIO — working instructions

Build standalone harness execution and recovery: invocation/session/process/workspace identities, grants, usage, cancellation and immutable execution receipts. Do not decide project direction, claim truth, context-readiness policy or product acceptance. Combraton and other callers supply authorized work; CBR is optional. No shared writable product stores.

## Read the right sources

Start with [README](README.md) and [the documentation map](docs/README.md), then [execution spec](docs/spec/SPEC.md) and [internal mechanics](docs/spec/INTERNALS.md). Read the [accepted baseline](https://github.com/Combraton/combraton/blob/main/docs/architecture/BASELINE.md) and relevant shared/domain sections for boundary changes. Follow [the shared development workflow](https://github.com/Combraton/combraton/blob/main/docs/DEVELOPMENT.md); record its commit/revision for multi-session work. Research and old code are references, not silent overrides of accepted decisions.

## Preserve these boundaries

- Journal invocation before dispatch. Fence ownership and reconcile ambiguous effects before retry; a lost acknowledgment must not create a duplicate process.
- Preserve native harness investigation/edit/test loops within granted scope. Execution completion is not verification or project acceptance.
- Advertise actual adapter/version capabilities and enforcement limits. Submitted bytes, observed delivery and model comprehension are different facts.
- Support standalone callers through public protocol profiles. Caller-selected context obligations do not become hidden PIO knowledge policy.

## Current standalone-first milestone

Own the standalone CLI/TUI and its user-attributed optional CBR client. Keep caller context policy separate from execution-core admission; discovery is not proof of capability. Core use must pass with CBR absent. Follow [release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md) and [ADR 001](https://github.com/Combraton/combraton/blob/main/docs/decisions/001-standalone-first-and-evaluation.md). Comparative evaluation lives in [benchmarks](https://github.com/Combraton/benchmarks); product acceptance remains evidence-based.

## Work and coordination

Inspect the assigned issue/task, branch, head, worktree and uncommitted changes before editing. Preserve unrelated work. For a large task, persist a small plan with outcome, scope, acceptance, dependencies and next step in `docs/work/` or the linked issue; do not rely on chat alone. One owner per task; one isolated worktree per concurrent writer. Agree shared contracts before consumers diverge.

Use subagents when a bounded independent investigation or review will help; pass scope, relevant invariants, source revisions and expected evidence explicitly. Prefer read-only helpers. Parallel writers require separate worktrees and non-overlapping scope/resources. Collect and verify results. Use separate top-level sessions for independently owned component implementations; no recursive swarm or permanent model-to-repo assignment is required.

Changing process/session identity, cancellation, workspace isolation, sandbox/permission enforcement, recovery, external-effect handling, accounting or an adapter requires primary docs and pinned source for the actual supported harness/version. Record accepted choices and superseded sections in the owning [decision record](docs/decisions/README.md). Escalate a needed change of direction, authority or reserved judgment; routine scoped investigation and repair proceed automatically.

## Verify and hand off

Run `python3 scripts/check_docs.py` from the repository root for documentation changes; see [verification](docs/VERIFICATION.md). Build/test and pinned-runner commands now exist in verification. The conformance-only M1 participant claims Core/core-test, the five PLAN Execution features, public effects and bounded event output through a labeled scripted fake host. ADR 002 requires the shared journal, outbox and projections; do not reintroduce the old blob store. The experimental fake-host matrix checks lower-level process behavior; do not count unsupported fixtures or fake-host checks as a real-adapter or accepted M1 milestone. Add reproducible commands alongside implementation.

Future product validation must include a real adapter, surviving-process recovery, lost acknowledgments, duplicate commands, cancellation, stale owners and truthful unsupported capabilities. A fake process alone cannot complete the real-adapter milestone.

Review the actual diff at recorded base/head. Before a session ends, persist commits/files, commands with exit status and evidence, unresolved facts, active resources and the next action in the task handoff. Treat old handoffs as historical observations; reconcile them with the checkout. Keep public records free of credentials and private transcripts.

Use existing native harnesses to ship v0.1. Combraton self-development is deferred until all four usable v0.1 releases. Do not install ECC/global hooks, select a model or relax runtime permissions merely because a reference suggests it.

## Session state and prompt lifecycle

Read [current session state](docs/work/STATE.md) at startup and reconcile it with the assigned issue, actual branch/head, diff, task handoff and relevant running resources before acting. Maintain this small navigation snapshot at meaningful checkpoints and before pausing, handing off or completing work. Record timestamp/owner, task and PR links, inspected revisions, completed work, remaining work, decisions and unresolved questions, actual checks/evidence and their limits, active resources, and the next action. Link detailed task records instead of copying transcripts or maintaining a second backlog. GitHub issues own live progress; when unavailable, identify the local task record as a temporary fallback and reconcile it later. Separate sessions do not automatically share context.

Give the human a short update at startup and meaningful checkpoints: what changed or was found, what works with evidence, what remains uncertain or needs judgment, and what happens next. Update stale setup/status statements when implementation makes them false. A snapshot is dated evidence, not permission to repeat completed actions.

Treat one-off kickoff and continuation prompts as temporary instructions tied to a task, owner and source revision. When work advances, rewrite the active prompt to the remaining work and link the current state. On completion or supersession, first preserve useful decisions, outcomes, unresolved items and evidence in durable task/decision records; then delete a disposable prompt or replace its executable instructions with a clearly labeled completed/superseded notice and links to the outcome or successor. Remove or update active links to retired prompts. Do not leave an obsolete prompt looking ready to execute.

Preserve reusable templates as templates. Do not delete accepted specifications, decision history, evidence, user source material, or another session's active prompt as cleanup. For local/untracked prompts, preserve necessary context durably before removal; Git cannot recover an untracked file. Limit cleanup to the current task's owned prompts. If a referenced prompt is unavailable, record the unresolved cleanup instead of claiming it was deleted. Fresh sessions must check prompt status and current state rather than blindly replaying a saved prompt.
