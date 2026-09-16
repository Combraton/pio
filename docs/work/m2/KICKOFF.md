# Fresh-session kickoff — M2 Codex adapter

## 1. Repository, branch and base

You are the standalone PIO M2 builder in a **new session**. Repository: `https://github.com/Combraton/pio`. Create **`codex/m2-codex-app-server` from merged main**, with one isolated `pio-m2` worktree, only after PR #4's separately authorized merge is verified. M2 was not started in the session that prepared this prompt.

**Base SHA: pending the merge of PR #4.** The observed pre-merge main is `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; it is not the M2 base. The owner accepted M1 at `2048e84b0e77b258444f8f30333f4b76584cdccc`; a final documentation-only successor adds this prompt. Fetch main, inspect PR #4's merged state/merge commit and verify main contains that commit. Record the selected main SHA as base and the new branch's head in TASK and STATE before editing code. If main has later commits, inspect them first. Do not assume a squash merge preserves the M1 commit ancestry. If PR #4 is still open, leave the branch uncreated and hand back the unmet merge prerequisite; this prompt does not authorize merging it.

## 2. Reading order — exact repository-relative paths

Read these in this order, reconciling the records with current Git rather than replaying historical plans:

1. `AGENTS.md`
2. `docs/work/STATE.md`
3. `docs/work/standalone-0.1/PLAN.md`
4. `docs/decisions/001-standalone-stack.md`
5. `docs/decisions/002-protocol-journal.md`
6. `docs/JOURNEYS.md`
7. `docs/work/m1/ACCEPTANCE-CORRECTIONS.md`
8. `docs/work/m2/TASK.md`

Then use the linked canonical inputs, `docs/VERIFICATION.md`, `docs/work/m1/CHECKPOINTS.md`, `README.md`, `docs/spec/SPEC.md` and `docs/spec/INTERNALS.md` for the bounded slice. Shared TASK/workflow was inspected at Combraton commit `9af69ce966bfacf0deb03606d99f28a355d1f944`; the packet links that template. No previous chat is required or authoritative over these records and the owner's recorded decisions.

## 3. Accepted work and explicitly unproven work

**M1 is owner-accepted at `2048e84`**, after the owner reproduced the full documented sequence from a clean clone, real launchd start/stop on a workstation, both CI platforms and an independent reorder mutant killed on the named ordering property. M0, the stack/scope dispositions and ADR 002 persistence direction are accepted. Preserve the M1 acceptance mapping and every stated limit; its acceptance-candidate report is a historical pre-acceptance record, supplemented by CHECKPOINTS and issue #3.

M1 proves the journal-backed Core/Execution slice, public durable **labeled fake-process** host, caller operation ledger, content-addressed output, recovery/order fences, truthful fake discovery and packaging skeleton at their recorded bounds. Scripted conformance is separate from process behavior. M1 baseline: 20 tests; runner 206 pass / 73 unsupported / 1 skipped; two supplemental passes separately; public matrix 66 attempts and diagnostic matrix 54, each with explicit classes/repetitions.

**All six product journeys remain `not_evaluated`; no real harness has run through PIO.** No native authentication, real-adapter support, end-to-end/TUI journey, release qualification, production throughput, total-disk/spool-GC bound or universal exactly-once property has been established. Process cancellation/workspace/usage adapters remain unavailable. Fake discovery reports native authentication unknown and usability false. Packaging is a skeleton with documented interruption/partial-uninstall limits. Do not turn milestone acceptance into claims beyond this evidence.

## 4. Protocol pin and upstream gaps

Use `protocol.lock.json`: Protocol **v0.1.0**, source **`cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`**. Verify its assets and build the pinned runner from its archive; do not substitute moving main or the reference provider implementation. Needed gaps go upstream as **versioned proposals with demonstrating fixtures**; never widen frozen schemas locally. Adopt a new contract only through an explicit approved pin update.

Protocol **#9 and #10 remain open coverage limits**, along with the undeclared subscription authorization-recheck barrier; verify upstream disposition before changing any claim. Detailed process witnesses remain independent journal/kernel evidence where the frozen public schema cannot express them.

Codex candidate: **0.146.0**, tag `rust-v0.146.0`, source **`e363b08c9175ac1cbe5893615dd2cb9ddf95043b`**. Qualify the selected resolved executable/version/hash. Compare canonical parsed JSON **per file**, preserving arrays/values and normalizing object keys; raw bytes of `codex_app_server_protocol.v2.schemas.json` vary across equivalent same-binary generations and must not trigger compatibility drift refusal.

## 5. Evidence discipline at every checkpoint

Reproduce the exact **clean-clone command sequence in `docs/VERIFICATION.md`**, with fresh stores and verified pinned assets, plus **two-platform CI on macOS arm64 and Linux x86_64**. Record commands, exit status, base/head, binary/source hashes, environment and actual limitations. Report the runner's native **per-directory outcome classes**, unsupported reasons, descriptor/manifest/transcripts and separate supplemental results. Reproduce **both matrices**, retaining attempted counts/repetitions, identity/count evidence and named mutation/refusal/wrong-reason results. Update **STATE at every checkpoint**, including base and head, current issue, active resources, results/limits and next action.

M2 adds real Codex **J1, J3, J4 and J5**, drift refusal and real native approval deny/allow without weakened permissions. Record authentication and chosen model for every live run using `docs/JOURNEYS.md`; unit tests, fake processes and headless fixtures do not substitute. Keep all M1 evidence directories and historical checkpoint documents intact. Save new M2 evidence separately.

## 6. Reserved owner decisions

The owner retains **license, evaluation thresholds/rubric, model and spend for live Codex runs, merge, tag and publication**. Do not infer authorization from credential presence or a prior cached login. Get and record the selected model/authentication route and authorized spend bound before live model calls; proceed with offline work while these are outstanding. Preserve native instructions, approvals, settings and permissions; no bypass, fabricated allow, unrestricted alternate API or global hook.

Carry M3 forward without implementing it now: distinguishing API/provider credential-source observation, and a negative control with **no configured credential but an existing cached login**, which must refuse specifically for the missing qualified route. claude.ai login remains unsupported until an approved route exists. Preserve the user's normal login; do not log out or copy tokens to simplify a test.

## 7. Environment gotchas

- **Worktree layout:** `pio` and `pio-m1` are separate worktrees of the PIO repository. At handoff, `pio` is still the old readiness checkout and `pio-m1` holds `codex/m1-core-host`; neither should be assumed to be main. Inspect `git worktree list`, status and branches; preserve both. Create only one M2 writer/worktree from verified merged main. Sibling `combraton`, `protocol`, `cbr` and `benchmarks` repositories remain untouched.
- **Unrelated `pio` on PATH:** use the absolute path to this checkout's `target/debug/pio`, or its explicit private installed executable. Never overwrite, upgrade or accidentally invoke the historical unrelated `pio`. Packaging defaults to a collision-checked `pio-standalone` alias.
- **Pull-request artifact naming:** checkout and artifact names use `github.event.pull_request.head.sha || github.sha`, never the synthetic PR merge SHA. Compare the recorded artifact/binary identity with the reviewed branch head.
- **Linux user manager:** the disposable Ubuntu CI VM runs `sudo systemctl start "user@$(id -u).service"` and exports `XDG_RUNTIME_DIR=/run/user/$(id -u)` before the systemd **user** job test. This provisions the runner's user manager, not a root PIO service. Do not silently replace unavailable manager evidence with direct process launch or run this CI provisioning against a workstation by habit. Test labels/runtime units are removed; temp evidence is retained outside Git.
- **Read-only-store fault:** the actual filesystem case uses SQLite files mode `0400` and store directory `0500`, with modes restored during cleanup. Run as a normal user: root can bypass permissions and invalidate the fault. Require the intended readonly refusal and independent process-table/spawn-marker absence. Public store faults come from runner/service launch configuration, never public request-field simulations. Existing-store observer opens must not rerun schema/meta writes; that caused Linux writer contention earlier.
- **Codex durable config side effect:** pinned `thread/start` with `cwd` and `workspace-write` can persist a trusted-project entry in the user's `~/.codex/config.toml`. Disclose and either avoid via a validated native-policy-preserving configuration path or explicitly account for the effect. Capture isolated before/after config evidence; do not probe the user's real config for discovery.

## 8. First concrete M2 action and prompt lifecycle

After verifying the merge, inspect issue state to avoid duplicates, **close accepted issue #3 and open the M2 issue linking `docs/work/m2/TASK.md` and the predecessor**. This transition is authorized only after the merge; it does not authorize the merge. Record the issue URL, base/head and ownership in TASK/STATE.

Then **implement and test ADR 002's 32 MiB canonical projected-state / 32,768-record pre-commit admission bound before any live run**. Use offline fixtures to cover the exact boundaries, events/dedupe accounting, explicit capacity refusal and no new effects/spawn on refusal. Record the bounded checkpoint before connecting the qualified app-server and scheduling owner-authorized live Codex work. Do not start with a live prompt or schema probe that changes the user's configuration.

This kickoff is **disposable**, not a permanent task authority. As M2 advances, rewrite it to the remaining work and link current STATE/task/issue evidence. Before retirement, preserve decisions, results, unresolved items and evidence in durable records; then delete this prompt or replace it with a clearly labeled retired/superseded notice and update active links, following `AGENTS.md`. Never rewrite historical M1 evidence to make later work appear previously proven.
