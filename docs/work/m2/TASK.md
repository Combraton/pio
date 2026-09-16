# Task packet — M2 first real Codex adapter

Prepared 2026-09-16 for a **fresh builder session after the separately authorized merge of PR #4**. M2 has not started. Uses the shared [TASK template](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/templates/TASK.md) and development agreement `development-v2-20260913` at the same inspected commit.

## Task / feature and outcome

Implement [PLAN's M2 row](../standalone-0.1/PLAN.md): **Codex 0.146.0 native app-server behind the durable host**, with the standalone CLI submitting scoped repository work and recording actual native output and immutable result evidence. Prove J1, J3, J4 and J5 with real Codex without replacing its native loop or weakening permissions.

Predecessor: accepted [M1 issue #3](https://github.com/Combraton/pio/issues/3), [PR #4](https://github.com/Combraton/pio/pull/4), accepted head `2048e84b0e77b258444f8f30333f4b76584cdccc`. M2 issue: **not opened yet; create after the merge**, link this packet and issue #3, and record its URL here and in STATE. Do not infer a new issue number.

## Accountable owner

Fresh standalone PIO builder session, assignment pending. Owner acceptance and independent review remain separate from builder evidence. No helper is assigned by this packet. One implementation writer/worktree; no sibling writes.

## Repository and worktree

Repository: `https://github.com/Combraton/pio`. Proposed branch: `codex/m2-codex-app-server`, created from **verified merged main**, in one isolated `pio-m2` worktree after checking `git worktree list` and preserving existing worktrees.

**M2 base SHA: pending PR #4 merge; not yet knowable.** Observed remote main before merge: `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; this is the M1 base, not an authorized M2 base. Accepted M1 head: `2048e84b0e77b258444f8f30333f4b76584cdccc`. The final documentation-only successor is the commit adding this packet; resolve it from Git/PR history rather than treating the accepted runtime head as the merged base.

Before branch creation, fetch main, verify PR #4 is merged, obtain its actual merge commit, and verify that commit is contained in the fetched main. Record the selected main SHA as M2 base and the new branch head in this packet and `docs/work/STATE.md`. If main advanced further, inspect intervening changes before selecting the base. Never branch M2 from the unmerged M1 branch or assume a squash merge retains M1 ancestry.

## Scope / authority

Permitted M2 work after handoff: enforce the journal commit bound; qualify the exact Codex binary/version/schema; implement host-owned app-server transport, native identities/events, scoped submission, approvals, supported steering/interrupt, output/result capture and recovery; add real Codex journey evidence and the corresponding CLI/public API behavior. Preserve M1 journal/outbox/projections, caller identity, ordering/launch/slot/generation fences, loss reporting, collision-safe packaging and native policy.

Reserved owner decisions: license, evaluation thresholds/rubric, **model and spend authorization for live Codex runs**, merge, tag and publication. Record the authorized model, authentication route and spend bound before any live model run; existing credentials or a cached login are not spend approval. Continue offline implementation/testing while those decisions are outstanding. No model selection, paid trial, native permission relaxation or global configuration change is authorized by this packet alone.

Exclude Claude implementation (M3), TUI journey completion (M4), optional real CBR composition (M5), release qualification (M6), workflow policy and sibling-store writes. Do not count fake-host or headless fixture evidence as a real harness or TUI journey. Preserve all M1 evidence directories, the acceptance mapping and historical checkpoint documents.

## Acceptance

M2 is accepted only with PLAN's distinguishing behavior and evidence:

- Codex **0.146.0 app-server** runs behind the durable host; client/daemon detach does not own its transport. The CLI submits scoped repository work and records real native output/result.
- **J1:** real Codex discovery/qualified binary selection, bounded repository edit/test, output and result evidence; wrong/unsupported executable is refused, never advertised usable.
- **J3:** real Codex detach, daemon restart, reattach/reconcile without duplicate prompt/work, with native session/turn, kernel start identity, host slot and controller generation evidence. Lost host remains uncertain; the duplicate-launch control fails for its intended identity/count reason.
- **J4:** real Codex supported steering/cancellation with correlated native acknowledgment and observed outcome, truthful refusal/unsupported behavior and a suppressed-ack negative control. A write or cancel request alone never proves comprehension or stopped work. M2 proves API/CLI paths; TUI proof remains M4.
- **J5:** real Codex core journey with CBR and Combraton absent; no hidden Context dependency. Claims remain bounded to the actual paths/platforms exercised.
- Schema and version drift are refused before native work. Bind the resolved executable, exact version, binary hash and per-file canonical schema identity; retain raw hashes only as capture provenance.
- Exercise approval **deny and allow** with actual native requests, correctly bound execution/turn/controller and preserved native restrictions. No bypass permissions, fabricated approvals or unrestricted alternative API used to evade the grant.
- Record authentication class/route and the **owner-chosen model** for every live test, without secrets, alongside the authorized spend bound and actual run/artifact identities.

Prerequisites and carry-forwards are part of the acceptance work:

- **Before any live run**, enforce ADR 002's hard **32 MiB canonical projected-state and 32,768-record** admission/commit limits, including retained events and dedupe outcomes, before staging/commit. Specify capacity refusal/retention behavior; test the exact boundaries and prove refusal causes no new effect/spawn. Whole-state diff cost remains explicit; these limits are not a total-disk/spool-GC or throughput guarantee.
- Compare **canonical parsed JSON per schema file**, preserving arrays and values while normalizing object-key order. In particular, never compare raw bytes of `codex_app_server_protocol.v2.schemas.json` for compatibility: repeated same-binary generations are byte-nondeterministic but parsed-equal. Include parsed-equal/raw-different acceptance and meaningful schema-change refusal controls.
- Disclose and either avoid through a validated native-policy-preserving configuration path or explicitly account for `thread/start` with `cwd` and `workspace-write` writing a trusted-project entry into the user's `~/.codex/config.toml`. Capture before/after configuration in an isolated test environment; do not probe against the user's real config merely for discovery.
- Keep [Protocol #9](https://github.com/Combraton/protocol/issues/9) and [#10](https://github.com/Combraton/protocol/issues/10) open coverage limits until upstream resolution is verified. The undeclared subscription authorization-recheck barrier also remains explicit. Never widen the frozen schemas locally.
- Carry **M3**, without implementing or claiming it in M2: prove the API/provider credential source through a distinguishing initialization/account-source or provider observation; remove configured credentials while preserving cached login and require refusal specifically for the missing qualified route. claude.ai login remains unsupported until an approved route exists; preserve the user's normal login.

## Canonical inputs

- [AGENTS](../../../AGENTS.md), [STATE](../STATE.md), [PLAN](../standalone-0.1/PLAN.md), [ADR 001](../../decisions/001-standalone-stack.md), [ADR 002](../../decisions/002-protocol-journal.md), [JOURNEYS](../../JOURNEYS.md), [M1 acceptance corrections](../m1/ACCEPTANCE-CORRECTIONS.md), [CHECKPOINTS](../m1/CHECKPOINTS.md), [VERIFICATION](../../VERIFICATION.md).
- M1 acceptance at `2048e84b0e77b258444f8f30333f4b76584cdccc`; runtime `7b05342cbcaaa1813b2da02cf6bdb01a1ddc2673`. The historical acceptance report/index remains unchanged; the later owner disposition is in CHECKPOINTS and issue #3.
- [Protocol lock](../../../protocol.lock.json): **v0.1.0**, source **`cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`**. Verify release assets with the existing verifier and use the released contracts/fixtures. Needed gaps go upstream as **versioned proposals with demonstrating fixtures**; pin an approved revision explicitly before adoption.
- Codex **0.146.0**, tag `rust-v0.146.0`, peeled source **`e363b08c9175ac1cbe5893615dd2cb9ddf95043b`**. Use the [pinned app-server README](https://github.com/openai/codex/blob/e363b08c9175ac1cbe5893615dd2cb9ddf95043b/codex-rs/app-server/README.md), ADR 001 and [readiness provenance](../standalone-0.1/evidence/readiness.json). A previously inspected local binary is not automatically the selected executable or a reproducible build of that source.
- Shared TASK/workflow revision: `9af69ce966bfacf0deb03606d99f28a355d1f944`. Preserve PIO execution authority and public component boundaries.

## Dependencies

PR #4 must be separately authorized and merged before M2 branching. After verifying the merge, close accepted issue #3 and open the M2 issue linking this packet and predecessor, unless already done; inspect first to avoid duplicates. This post-merge issue transition is authorized by the owner; the merge itself is not authorized here.

Live testing additionally requires the enforced commit bound, qualified selected Codex executable/schema, isolated worktree/config/store, unchanged native permission policy, usable authenticated route, and owner-selected model/spend. Missing live-run decisions block live calls, not offline bound/adapter work. Protocol gaps retain explicit limits while upstream proposals are reviewed. CBR and Combraton are not runtime dependencies.

## Plan and next step

1. Reconcile the merge, worktree, branch and exact base/head; complete the post-merge issue transition and record the assignment/issue URL in STATE.
2. **First concrete implementation action:** implement and test ADR 002's 32 MiB / 32,768-record pre-commit gate using offline fixtures, exact-boundary and no-effect refusal evidence. Do not launch a real harness to do this.
3. Qualify the Codex version/schema and configuration side-effect handling, then connect app-server through the durable host without relaxing existing fences or permissions.
4. Once live-run prerequisites and reserved decisions are satisfied, execute bounded real Codex J1/J3/J4/J5 plus deny/allow and drift controls. Follow PLAN's one focused experiment cycle and one repair/retest before a checkpoint; unresolved invariants remain explicit limits or owner decisions.
5. Reproduce checkpoint evidence, request independent review and hand off the exact candidate. Do not self-accept M2 or merge/publish.

## Verification

At every checkpoint reproduce the [documented clean-clone sequence](../../VERIFICATION.md) and **macOS arm64 + Linux x86_64 CI**, with pinned toolchain/lockfile and runner built from the verified archive. Preserve native runner classes **per directory**, original manifests/transcripts, unsupported reasons and separate supplemental counts. Preserve **both** public and diagnostic matrices with attempted counts, repetitions and named mutant/refusal reasons; update STATE with base/head at each checkpoint.

M1's comparison baseline is 20 tests; runner 206 pass / 73 unsupported / 1 skipped, plus two separate supplemental passes; public matrix 66 attempts (45/6/9/6 by pass/property-failure/defense-refusal/classifier-control), diagnostic 54 (39/3/9/3). These are historical bounded observations, not prescribed future totals or real Codex evidence.

M2 live commands and fixtures do not exist yet: implement and document them before claiming a run. Record every real journey using `docs/JOURNEYS.md`, including commands/exit statuses, PIO/harness/artifact hashes, authentication/model, identities, native events, configuration before/after, actual permissions, cost/unknown liability, negative controls and untested segments. Use a fresh output directory; never overwrite M1 evidence. All six product journeys are still `not_evaluated` at this handoff; no real harness has run through PIO.

## Handoff location

`docs/work/STATE.md` is the current navigation snapshot; this packet owns M2's durable scope/acceptance. The post-merge M2 issue owns assignment/progress. Save M2 checkpoint reports/evidence under `docs/work/m2/` without rewriting historical M1 records. [KICKOFF](KICKOFF.md) is disposable: rewrite it to remaining work as M2 advances, then retire it after preserving decisions, results, unresolved items and evidence in the durable records.
