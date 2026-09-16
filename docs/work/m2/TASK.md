# Task packet — M2 first real Codex adapter

Prepared 2026-09-16 for a fresh builder after PR #4's merge; **started and amended the same day** with the owner decisions recorded after M1 acceptance ([ADR 001 amendment](../../decisions/001-standalone-stack.md#amendment--owner-decisions-after-m1-acceptance-2026-09-16), [PLAN amendment](../standalone-0.1/PLAN.md#owner-amendment-after-m1-acceptance-2026-09-16)). Uses the shared [TASK template](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/templates/TASK.md) and development agreement `development-v2-20260913` at the same inspected commit.

## Task / feature and outcome

Implement [PLAN's M2 row](../standalone-0.1/PLAN.md): **Codex 0.146.0 native app-server behind the durable host**, driving the user's installed Codex **exactly as the user configured it** — its own login, model, instructions, settings and permissions. The standalone CLI submits scoped repository work and records actual native output and immutable result evidence. Prove J1, J3, J4 and J5 with real Codex without replacing its native loop, selecting a model or weakening permissions.

Predecessor: accepted [M1 issue #3](https://github.com/Combraton/pio/issues/3) (closed on merge; successor comment added), [PR #4](https://github.com/Combraton/pio/pull/4) merged as `9cf70474c28f549650e6b48e8be20ae88426a1b0` (reviewed head `a34c408864e8f87abefd6d820518902b31b6a724`, accepted M1 head `2048e84b0e77b258444f8f30333f4b76584cdccc`). **M2 issue: [#5](https://github.com/Combraton/pio/issues/5).**

## Accountable owner

Builder: a Claude Code session acting as standalone PIO M2 builder, assigned by the owner on 2026-09-16. Owner acceptance and independent review remain separate from builder evidence. One implementation writer/worktree; no sibling writes. Helpers are read-only unless given their own worktree and non-overlapping scope.

## Repository and worktree

Repository: `https://github.com/Combraton/pio`. Branch **`codex/m2-codex-app-server`** in one isolated `pio-m2` worktree. The earlier `pio` (readiness) and `pio-m1` (M1) worktrees are preserved untouched.

- **M2 base:** `9cf70474c28f549650e6b48e8be20ae88426a1b0`, the merge commit of PR #4 on `main`. Verified 2026-09-16: `main` contains it, its tree is identical to reviewed head `a34c408`, and accepted head `2048e84` is an ancestor.
- **Branch start:** `9cf7047`. The first branch commit is documentation only (owner decisions, LICENSE, records); later heads are recorded in [STATE](../STATE.md) and Git history.

## Scope / authority

Permitted M2 work: enforce the journal commit bound; qualify the exact Codex binary/version/schema; implement host-owned app-server transport, native identities/events, scoped submission, approvals, supported steering/interrupt, output/result capture and recovery; add real Codex journey evidence and the corresponding CLI/public API behavior. Preserve M1 journal/outbox/projections, caller identity, ordering/launch/slot/generation fences, loss reporting, collision-safe packaging and native policy.

Owner decisions that bind M2 (2026-09-16):

- **Harness as configured.** PIO never selects or injects a model or provider and never edits the provider configuration. A same-day draft routing MiniMax through a Codex custom provider was withdrawn by the owner; no such configuration exists or may be created.
- **Real Codex home.** Live runs use the user's real Codex home. Capture its configuration before and after **every** live run, disclose any trusted-project entry that `thread/start` adds, and change nothing else. Raw snapshots contain private paths and stay outside Git; commit digests and a redacted difference only.
- **Spend bound:** at most **1,000,000 Codex tokens across all M2 live tests**, counted per run from Codex's own usage reports and recorded per journey in STATE. A run without a usage report is unknown liability, never zero. Stop live work and report at **800,000** (80 percent). At most **three concurrent live sessions**.
- **Frugality consequence.** The Codex bound supports only a small number of deliberately tiny live tasks. Repetition-heavy live sampling belongs to OpenCode (PLAN row M3b), which the owner prefers for heavy-usage testing. Offline, fake-host and conformance evidence carry repetition in M2 but never substitute for a real journey.

Reserved owner decisions: evaluation thresholds/rubric, merge, tag and publication. No native permission relaxation, bypass, fabricated allow, unrestricted alternate API, global hook or global configuration change.

Exclude Claude implementation (M3), OpenCode/Hermes adapters (M3b), TUI journey completion (M4), optional real CBR composition (M5), release qualification (M6), workflow policy and sibling-store writes. Do not count fake-host or headless fixture evidence as a real harness or TUI journey. Preserve all M1 evidence directories, the acceptance mapping and historical checkpoint documents.

## Acceptance

M2 is accepted only with PLAN's distinguishing behavior and evidence:

- Codex **0.146.0 app-server** runs behind the durable host; client/daemon detach does not own its transport. The CLI submits scoped repository work and records real native output/result.
- **J1:** real Codex discovery/qualified binary selection, bounded repository edit/test, output and result evidence; wrong/unsupported executable is refused, never advertised usable.
- **J3:** real Codex detach, daemon restart, reattach/reconcile without duplicate prompt/work, with native session/turn, kernel start identity, host slot and controller generation evidence. Lost host remains uncertain; the duplicate-launch control fails for its intended identity/count reason.
- **J4:** real Codex supported steering/cancellation with correlated native acknowledgment and observed outcome, truthful refusal/unsupported behavior and a suppressed-ack negative control. A write or cancel request alone never proves comprehension or stopped work. M2 proves API/CLI paths; TUI proof remains M4.
- **J5:** real Codex core journey with CBR and Combraton absent; no hidden Context dependency. Claims remain bounded to the actual paths/platforms exercised.
- Schema and version drift are refused before native work. Bind the resolved executable, exact version, binary hash and per-file canonical schema identity; retain raw hashes only as capture provenance.
- Exercise approval **deny and allow** with actual native requests, correctly bound execution/turn/controller and preserved native restrictions. No bypass permissions, fabricated approvals or unrestricted alternative API used to evade the grant.
- Every live run records, without secrets: authentication class, the model/provider that actually served the run as observed from Codex, Codex version and binary hash, configuration before/after digests with the redacted difference, and token usage against the bound, alongside run/artifact identities.

Prerequisites and carry-forwards are part of the acceptance work:

- **Before any live run**, enforce ADR 002's hard **32 MiB canonical projected-state and 32,768-record** admission/commit limits, including retained events and dedupe outcomes, before staging/commit. Specify capacity refusal/retention behavior; test the exact boundaries and prove refusal causes no new effect/spawn. Whole-state diff cost remains explicit; these limits are not a total-disk/spool-GC or throughput guarantee.
- Compare **canonical parsed JSON per schema file**, preserving arrays and values while normalizing object-key order. Never compare raw bytes of `codex_app_server_protocol.v2.schemas.json` for compatibility: repeated same-binary generations are byte-nondeterministic but parsed-equal. Include parsed-equal/raw-different acceptance and meaningful schema-change refusal controls.
- `thread/start` with `cwd` and `workspace-write` writes a trusted-project entry into the user's Codex configuration. Disclose it and account for it with before/after capture on every live run, as above. Offline tests use isolated fixtures and never read the user's configuration to probe discovery.
- Keep [Protocol #9](https://github.com/Combraton/protocol/issues/9) and [#10](https://github.com/Combraton/protocol/issues/10) open coverage limits until upstream resolution is verified. The undeclared subscription authorization-recheck barrier also remains explicit. Never widen the frozen schemas locally.
- Carry **M3** without implementing or claiming it in M2: PIO drives the user's installed Claude Code with the user's own login, or API key when configured. M3 proves which route served every run, tests an explicit precedence rule when both exist, and refuses with the specific missing-route reason when neither is usable. Anthropic's third-party-login caveat stays visible; never log the user out, copy tokens or edit global Claude settings.

## Canonical inputs

- [AGENTS](../../../AGENTS.md), [STATE](../STATE.md), [PLAN](../standalone-0.1/PLAN.md), [ADR 001](../../decisions/001-standalone-stack.md) including its 2026-09-16 amendment, [ADR 002](../../decisions/002-protocol-journal.md), [JOURNEYS](../../JOURNEYS.md), [M1 acceptance corrections](../m1/ACCEPTANCE-CORRECTIONS.md), [CHECKPOINTS](../m1/CHECKPOINTS.md), [VERIFICATION](../../VERIFICATION.md).
- M1 acceptance at `2048e84b0e77b258444f8f30333f4b76584cdccc`; runtime `7b05342cbcaaa1813b2da02cf6bdb01a1ddc2673`. The historical acceptance report/index remains unchanged; the owner disposition is in CHECKPOINTS and issue #3.
- [Protocol lock](../../../protocol.lock.json): **v0.1.0**, source **`cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`**. Verify release assets with the existing verifier and use the released contracts/fixtures. Needed gaps go upstream as **versioned proposals with demonstrating fixtures**; pin an approved revision explicitly before adoption.
- Codex **0.146.0**, tag `rust-v0.146.0`, peeled source **`e363b08c9175ac1cbe5893615dd2cb9ddf95043b`**. Use the [pinned app-server README](https://github.com/openai/codex/blob/e363b08c9175ac1cbe5893615dd2cb9ddf95043b/codex-rs/app-server/README.md), ADR 001 and [readiness provenance](../standalone-0.1/evidence/readiness.json). On 2026-09-16 the workstation's `codex --version` reported `codex-cli 0.146.0`; that is not yet qualification of the selected executable or a reproducible build of that source.
- Shared TASK/workflow revision: `9af69ce966bfacf0deb03606d99f28a355d1f944`. Preserve PIO execution authority and public component boundaries.

## Dependencies

The PR #4 merge and issue transition are complete. Live testing additionally requires the enforced commit bound, the qualified selected Codex executable/schema, an isolated fixture repository and PIO store, before/after capture of the user's Codex configuration, unchanged native permission policy, a usable Codex login on the user's own account, and working per-run token accounting against the bound. Missing prerequisites block live calls, not offline bound/adapter work. Protocol gaps retain explicit limits while upstream proposals are reviewed. CBR and Combraton are not runtime dependencies.

## Plan and next step

1. **Done 2026-09-16:** merge verified; worktree and branch created from `9cf7047`; issue #3 closed on merge with a successor comment; [issue #5](https://github.com/Combraton/pio/issues/5) opened; owner decisions, MIT LICENSE and records committed as the branch's first, documentation-only commit.
2. **Next:** implement and test ADR 002's 32 MiB / 32,768-record pre-commit gate using offline fixtures, exact-boundary and no-effect refusal evidence. Do not launch a real harness to do this. Record the bounded checkpoint.
3. Qualify the Codex version/binary/schema and build the configuration capture, then connect app-server through the durable host without relaxing existing fences or permissions.
4. Execute token-frugal real Codex J1/J3/J4/J5 plus deny/allow and drift controls within the bound. Follow PLAN's one focused experiment cycle and one repair/retest before a checkpoint; unresolved invariants remain explicit limits or owner decisions.
5. Reproduce checkpoint evidence, request independent review and hand off the exact candidate. Do not self-accept M2 or merge/publish.

## Verification

At every checkpoint reproduce the [documented clean-clone sequence](../../VERIFICATION.md) and **macOS arm64 + Linux x86_64 CI**, with pinned toolchain/lockfile and runner built from the verified archive. Preserve native runner classes **per directory**, original manifests/transcripts, unsupported reasons and separate supplemental counts. Preserve **both** public and diagnostic matrices with attempted counts, repetitions and named mutant/refusal reasons; update STATE with base/head at each checkpoint.

M1's comparison baseline is 20 tests; runner 206 pass / 73 unsupported / 1 skipped, plus two separate supplemental passes; public matrix 66 attempts (45/6/9/6 by pass/property-failure/defense-refusal/classifier-control), diagnostic 54 (39/3/9/3). These are historical bounded observations, not prescribed future totals or real Codex evidence.

M2 live commands and fixtures do not exist yet: implement and document them before claiming a run. Record every real journey using `docs/JOURNEYS.md`, including commands/exit statuses, PIO/harness/artifact hashes, authentication class and served model, identities, native events, configuration before/after, actual permissions, token usage/unknown liability, negative controls and untested segments. Use a fresh output directory; never overwrite M1 evidence. All six product journeys are still `not_evaluated`; no real harness has run through PIO.

## Handoff location

`docs/work/STATE.md` is the current navigation snapshot; this packet owns M2's durable scope/acceptance. Issue #5 owns assignment/progress. Save M2 checkpoint reports/evidence under `docs/work/m2/` without rewriting historical M1 records. The [continuation prompt](KICKOFF.md) is disposable: keep it pointed at the remaining work, then retire it after preserving decisions, results, unresolved items and evidence in the durable records.
