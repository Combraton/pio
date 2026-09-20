# Verification available now

PIO now has an M1 Cargo workspace, experimental fake-host process slice and pinned conformance pipeline. Its conformance-only Unix participant advertises Core/core-test and the five PLAN Execution features with public effects and bounded events, all through a labeled fake executor. Protocol v0.1.0 is released separately. Documentation checks validate structure only.

From this repository's root:

```sh
python3 scripts/check_docs.py
git diff --check
```

Python 3 standard library is sufficient; there is no package install step. The script checks required entrypoints, the local `CLAUDE.md` import, ordinary Markdown file targets, balanced fences and private machine paths in Markdown. It exits nonzero on an error. It reports cross-repository links it could not check.

When all five clones are siblings under one directory, also run from each repository root:

```sh
python3 scripts/check_docs.py --workspace ..
```

This additionally resolves Combraton GitHub main-file links against the sibling checkouts, including benchmarks. It does not prove those checkouts match the remote branches. Record their commits when using the result as integration evidence.

The GitHub Actions documentation job runs the first script on pushes and pull requests, with read-only contents permissions. It does not fetch sibling repositories. Remote URL reachability, Markdown fragment targets, Mermaid rendering, source-manifest consistency, semantic correctness, live harness instruction loading and product behavior need separate inspection. The script is intentionally small and is not a general Markdown parser.

## M1 build and conformance

The [journey-verification matrix](JOURNEYS.md) records the six required journeys and their shared evidence fields. All product journeys remain `not_evaluated` until their actual entry points exist and are exercised. A lower-level test cannot complete a TUI or real-adapter journey.

Prerequisites: Rustup, Python 3.12+, Git, GitHub CLI (`gh`), C compiler/linker and network access. An existing macOS GUI user domain or Linux systemd user manager is required for the packaging lifecycle test (CI checks the actual manager; no substitute process launch). Authenticate `gh` for release asset downloads (CI uses its read-only `GH_TOKEN`). The pinned toolchain is Rust 1.97.1; Cargo.lock is committed. From a clean clone on macOS arm64 or Linux x86_64:

```sh
rustup toolchain install 1.97.1 --profile minimal --component rustfmt --component clippy
rustc --version
cargo --version
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --locked -- -D warnings
python3 scripts/check_docs.py
git diff --exit-code -- Cargo.lock
python3 scripts/fake_host_matrix.py --out target/fake-host --repetitions 3
python3 scripts/public_host_matrix.py --out target/public-host --repetitions 3
python3 scripts/client_recovery.py --out target/client-recovery
python3 scripts/codex_host_matrix.py --out target/codex-host --repetitions 3
python3 scripts/packaging_check.py --out target/packaging
python3 scripts/conformance.py --out target/conformance
```

These are the commands in [runtime CI](../.github/workflows/runtime.yml). CI uses macOS 15 arm64 and Ubuntu 24.04 x86_64 and asserts the architecture. These are tested CI environments, not a broader minimum-OS support claim. Use `target/debug/pio` explicitly: an unrelated historical `pio` may already be installed on PATH. The binary supports `--version`, `participant`, `conformance --data-dir DIR --config FILE --socket PATH`, and the experimental `fake` namespace. The runner creates the private socket directory, launch configuration and credentials; no user credential or configuration is used. The conformance service exits when its launcher closes stdin. It exposes core-test only through this test entrypoint.

The conformance command downloads the six pinned release assets into a fresh temporary directory, runs the existing integrity verifier, extracts the verified source archive, builds `combraton-conformance` with its released lockfile, runs runner self-tests and fixture validation, and runs all fixtures against PIO's descriptor. No sibling clone or pre-existing cache is required. It preserves the runner's exit status and unmodified manifest/transcripts; `pio-report.json` reports native outcome classes, counts for each fixture directory, and every unsupported/skipped coverage limit. Vendored validation schemas are compared byte for byte with the verified archive; `schema-verification.json` records the result. CI uploads the rendered descriptor, target, pin receipt, environment, manifest and transcripts even on failures. The [Execution checkpoint](work/m1/EXECUTION.md) records the latest per-directory results. The earlier [Core checkpoint](work/m1/CORE.md) remains historical evidence. A separate `pio-regressions/` manifest/transcript runs PIO supplemental fixtures against the same pinned runner and is never added to the released suite count. Protocol issue [#9](https://github.com/Combraton/protocol/issues/9) tracks the dependency-fixture coverage gap. Unsupported outcomes are never counted as passes. Cargo unit tests exercise admission/conflicts, transition validity, generation fencing, strict encoding, atomic transaction rollback, response loss and effect obligation storage. Public effect lifecycle claims are backed by the pinned Execution fixtures; unit tests alone would not establish them. Process behavior is exercised by the separate fake-host matrix command; its failures fail CI.

For offline asset reuse, append `--assets "$PIO_RELEASE_DIR"`; verification still runs. Choose a fresh output path for every run: existing evidence directories are never overwritten. Cargo downloads still need either network access or an existing registry cache. No real adapter, UI, memory-evaluation or end-to-end journey has passed.

For a change, report the command, exit status, environment, tested revision, real versus simulated dependencies, evidence location and untested limitations. Preserve the producer exit code when displaying shortened logs. Review the relevant diff against an explicit base/head.

## Standalone release evidence

The [Protocol pin procedure](work/standalone-0.1/PROTOCOL.md) uses Python 3.12+ (tested with 3.14.5) and downloaded release assets:

```sh
python3 scripts/verify_protocol_pin.py --assets "$PIO_RELEASE_DIR"
```

This checks pinned asset hashes, 539 bundle files and the released 420-file normative inventory. It does not build Protocol, run conformance or test PIO. A temporary modified-manifest negative control was refused with exit 1 for the expected checksum mismatch; see [pin evidence](work/standalone-0.1/evidence/pin-checks.json).

Follow [standalone release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md). Core no-optional-service tests, protocol conformance, real-adapter integration, comparative outcomes and UI usability are separate evidence classes. The [benchmarks repository](https://github.com/Combraton/benchmarks) owns cross-product scenarios/results, not this service's normative contract. No runtime benchmark has been implemented or run by the documentation setup.

## Experimental fake-host evidence

The corrected matrix runs eighteen cases three times (54 attempted runs): caller detach/daemon restart/reattach; duplicate and conflicting commands; SQLite write refusal before spawn; crashes after intent, host claim, release and receipt; lost host; stale controller; controller-generation rollback; lost-admission rollback within the same generation; the J3 marker-counter mutant, three replay-defense mutants, a wrong-reason classifier control, fenced release with known non-delivery, and a filesystem-level read-only store fault. Each case records its attempted repetition, observations and result. The mutant must fail the same `single_launch_identity_count` property used by the positive recovery case, specifically because two actual child starts were observed. A timeout, crashed test runner or unrelated assertion fails the matrix.

`matrix.json` includes source-file inventory and binary digests, platform, attempts and outcome counts. Per-case files preserve the journal/outbox, child-written append-only spawn/release markers, before/after identities and generations, and daemon logs. The journal-failure case also inspects the OS process table independently. CI uploads these beside the Protocol artifacts. Temporary stores are isolated under `/tmp` and retained for diagnosis; owned fake processes are killed only after comparing their kernel start identity. These tests use no real adapter, CBR, Combraton or user harness configuration.

To inspect the experimental path manually, use a new private directory and separate terminals:

```sh
mkdir -m 700 /tmp/pio-fake-example
target/debug/pio fake daemon /tmp/pio-fake-example
# In another terminal (each request connection closes after its response):
target/debug/pio fake request /tmp/pio-fake-example '{"op":"submit","id":"demo","payload":{"duration_ms":10000}}'
target/debug/pio fake request /tmp/pio-fake-example '{"op":"inspect","id":"demo"}'
```

This diagnostic JSON interface is not Core/Execution or an installable product CLI. Every execution is labeled `fake-host`. The child performs only bounded deterministic output and waiting. The journal-backed conformance participant separately implements Core/Execution and scripted discovery/workspaces/usage. Durable caller operations, content-addressed payloads and integration with the surviving-process host are exercised in the public checkpoint below. Neither the scripted observations nor the lower-level matrix alone establish the complete public durable-host path. Persistent launch guards and an external controller witness hold detected rollback; comprehensive backup/restore and storage-failure certification remain separate work. Do not remove guard/witness files to bypass a refusal.


## Public process and caller checkpoint

`target/debug/pio serve-fake --data-dir PRIVATE_DIR --config SERVICE_JSON --socket PRIVATE_DIR/public.sock` serves the public Unix binding independently of terminal stdin. Its explicit local configuration is `{"format":"pio-fake-service/1","protocol":{"format":"combraton-conformance-config/1","principal":"owner","credentials":[{"credential":"ccred1.owner.<43-character-test-token>"}],"executor":{"host_id":"durable-fake-host"}},"fake_host":{"duration_ms":10000,"fault":""}}`. Use an actual 43-character credential suffix. The matrix generates isolated configurations, credentials and stores; none are external provider credentials. `executor.script` is rejected in this mode. The conformance participant continues to use the separately labeled scripted test adapter.

The public process mode supports submit, inspect, reconcile, output, discovery and controller claims. Workspace/budget adapter behavior and cancellation are explicitly unavailable there; the five-feature runner claim belongs to the scripted participant, not a native adapter. The shared mutex service loop still serializes commands; durable child ownership is in detached `pio-host` processes. The process daemon also polls independently of connected clients, so detach does not stop observation.

`target/debug/pio client submit --store CALLER_DIR --socket PRIVATE_DIR/public.sock --credential-file CREDENTIAL_FILE --request ENVELOPE_JSON [--basis BASIS_JSON]` first commits the exact frozen command envelope and optional user-selected scope/policy basis. `target/debug/pio client reconcile --store CALLER_DIR --socket PRIVATE_DIR/public.sock --credential-file CREDENTIAL_FILE` reconciles pending identities after restart. An empty reconciliation stays pending; it does not create a new command. Use a separate private caller directory. This is a low-level attributed CLI, not the planned TUI or a completed product journey.

Public matrix artifacts include authenticated command/query transcripts, result-schema validation against unchanged vendored schemas, journal/outbox witnesses, child-created spawn/release markers and kernel start identities. The native schema cannot expose those detailed identities in `inspect`; they remain independent artifact witnesses. Each case records attempt/repetition and named property/refusal class. The diagnostic matrix remains separate test tooling.

`client_recovery.py` proves write-before-network on an absent endpoint, no spawn on empty reconciliation, recovery from a committed-but-lost response with one child, real fake-child output/exit, and digest-only output references in the journal. CAS object corruption is a Cargo test. These results do not qualify a real harness, native authentication or any end-to-end journey.


## M1 acceptance corrections

The public matrix now has 22 cases × 3 repetitions = 66 attempts. The diagnostic matrix remains 18 × 3 = 54. Public additions cover discovery, a crash between protocol dispatch-marker and host invocation-intent commits (exit 95), an actual admission-before-marker mutant and its wrong-reason control. A read-only journal oracle correlates execution ID to host command identity and compares the first dispatch-intent observation's sequence with invocation intent. Ordinary dispatch cases require exactly one pair. The cut records a marker but no host intent; restart conservatively records ambiguity and never spawns. Journal-sequence witnesses are preserved in `journal-order.json`.

Process-mode discovery reports the built-in labeled fake executable as detected, recognized, version-supported and reachable. Native authentication is **unknown**, so `usable` is false under the frozen all-positive rule. Protocol caller authentication is not harness authentication. Discovery itself spawns no child.

Restart records `execution.recovery.decided` followed by `execution.host.changed`, bound to the advanced controller generation. An existing child retains its original process/slot identity. Pending marker-present work becomes ambiguous until release/absence evidence reconciles it; an already proven release is preserved. The delivery evidence class **`child_release_marker`** means the identified fake child read its release message and wrote its marker. It produces the `delivered` determination, not native-provider acknowledgment or model comprehension. Recovered ambiguity resolves to `delivered` or `not_delivered` through reconciliation events with the matching frozen outcome. Recovery-state names are no longer used as delivery proof classes; the frozen optional `proof_class` enum is not extended or misused.

[Private packaging and user-job prototypes](../packaging/README.md) document layout, collision refusal, uninstall ownership checks, and the exact lifecycle command used in CI. `target/packaging` contains native-manager start/stop evidence and unchanged unrelated-`pio` witnesses.

## M2 capacity bound

ADR 002's [capacity bound](decisions/002-protocol-journal.md#capacity-bound-2026-09-16) adds `pio-core` and `pio-protocol` Cargo tests: 28 in total at `5be40f9`, and **31** after the admission headroom and proposal demonstration at `aa7ad78`. They cover: the exact byte and record limits and one over each; refusal before any staged row, using abort triggers; growth through unchanged records; accepted shrinking and no-op commits; subject, event and dedupe records counted by a real command; configured event retention changing the result; refused `execution.submit` creating nothing; the at-limit stall after a committed dispatch marker; queries and replays staying available during capacity-refused background commits, while other store failures stay `unavailable`; and equality with the canonical encoding length. An ignored probe measures commit cost at the record bound: `cargo test -p pio-core --locked [--release] -- --ignored --nocapture measure_commit_cost`. Its timings are observations, not a pass/fail property.

The headroom tests cover the exact admission thresholds for records and bytes (hard commits accept the same state), a submit refused one over the threshold while an ordinary put above it is accepted, and a submit admitted exactly at the threshold that runs to exit beyond it with no hard-limit refusal while a second submit is refused. `pinned_errors_cannot_distinguish_capacity_from_transient_commit_failure` checks the [Protocol #13](https://github.com/Combraton/protocol/issues/13) demonstration against the frozen error schema.

The public matrix adds `capacity_refusal_no_spawn` (23 cases × 3 = 69 attempts at `5be40f9`) and then `capacity_headroom_admitted_completes`, for **24 cases × 3 = 72 attempts**. The headroom case pads an execution-free store to 31,125 records. One durable submit must be admitted exactly at 31,130, which the case checks by counting the admission fact's new keys. A second submit must return `unavailable` with one `admission_projection_records` log line. The admitted child must be delivered with `child_release_marker` and exit 0 while the projection grows past 31,130, with no hard-limit refusal, one spawn marker and no process or execution record for the refused identity. It stops a durable service with no executions and uses the test-only `target/debug/pio fake fill-projection DIR RECORDS` tooling to pad the projection to 32,767 records through the same bounded commit. After restart, one public `execution.submit` must return `unavailable` / `same_command`. Since `aa7ad78` it is refused first by the admission threshold, so its log line names `admission_projection_records`. A public `execution.controller.claim`, which admits no work, must also return `unavailable`, with a second log line naming the hard `projection_records` limit. Journal, outbox, invocation rows and projection rows must be unchanged; spawn markers, process-table matches and an execution record for the refused identity must be absent; `execution.inspect` must answer `not_found`. The tooling then removes its filler and the same store and configuration must admit and spawn exactly one ordinary child, which checks that the refusal is not vacuous. The filler tooling is not a product command and never runs inside the service.

## M2 Codex qualification

`pio-codex` binds a user-selected Codex executable before any native work. `target/debug/pio codex qualify --executable PATH --work DIR [--expected IDENTITY]` resolves the npm wrapper the way the pinned wrapper does and records wrapper, Node (path, hash, version) and native binary hashes. It runs `--version` for the selected and native binaries, requires 0.155.1, and only then generates the app-server JSON schemas. Each file's canonical parsed JSON digest is compared with the checked-in [schema identity](../adapters/codex/0.155.1/schema-identity.json) (312 files). The previous [0.146.0 identity](../adapters/codex/0.146.0/schema-identity.json) is kept as the record the re-pin was measured against. Raw digests are provenance only. It exits 0 when qualified and 3 when refused; refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, `native_version_mismatch`, `schema_generation_failed`, `schema_drift`). The executable always runs with an isolated `CODEX_HOME`. `pio codex config-snapshot --codex-home DIR` and `pio codex config-diff BEFORE AFTER [--fixture-root DIR]` record configuration digests and project trust entries, reporting paths only as digests plus a `fixture` / `outside_fixture_root` label.

Cargo tests (with labeled fake executables, so CI needs no Codex) cover canonical JSON, parsed-equal/raw-different acceptance, changed/added/removed schema refusal naming files, the isolated `CODEX_HOME`, an unsupported version never receiving Codex arguments, a missing executable refused as data, three npm wrapper layouts, the checked-in identity's own listing digest, and configuration trust-entry disclosure. They also cover the thread-settings guard, including a configured `approval_policy = "untrusted"` refused as unresolved rather than compared. `scripts/codex_host_matrix.py`'s file-change case proves only command approvals carry `kind`. Writing a fake executable and exec'ing one are serialized inside each test binary: a sibling test's fork inherits the still-open write descriptor and Linux then refuses the exec with `ETXTBSY`, which failed one Ubuntu CI run at `56dac9f`.

`python3 scripts/codex_offline_probe.py --executable PATH --out FRESH_DIR` needs an installed Codex and is not part of CI. It qualifies, then runs five cases, each with its own isolated `CODEX_HOME` and credential variables removed, sending only `initialize` and `thread/start` for a throwaway fixture repository and recording the configuration difference: the settings live runs request; absent settings on a fresh project; absent settings with the project already trusted, which is what the thread-settings guard assumes; `approvalPolicy: "untrusted"` requested per thread, which R5 and R6 need; and `approval_policy = "untrusted"` configured, which 0.155.1 refuses to start with. Workstation evidence and findings are in [codex-qualification](work/m2/codex-qualification/README.md). None of this is a live run, real journey or model-backed result.

## M2 Codex adapter offline matrix

`target/debug/pio serve-codex --data-dir DIR --config FILE --socket PATH` serves the public Unix API with the ADR 003 Codex adapter. A `pio-codex-service/1` configuration names the selected executable, its explicit environment, the Codex home, the fixture root, and optional thread settings (`danger-full-access` and approval policy `never` are refused). For a real executable, startup qualifies it and refuses with `codex_not_qualified` before any native work. `labeled_fake: true` runs the labeled test double `pio codex fake-app-server` instead; its evidence is labeled `pio-fake-app-server`, and discovery reports it as not Codex and never usable.

`python3 scripts/codex_host_matrix.py --out target/codex-host --repetitions 3` runs 17 cases × 3 (51 attempts) against the fake app-server, with independent markers written by the fake, read-only journal and host events, and the process table:

- a J1-shaped turn: `provider_ack_id` delivery from the `turn/start` response, spooled output, observed token usage, exit 0, fixture trust-entry disclosure, and brief bytes kept out of the journal;
- approval decline and accept delivered natively, with a repeat answer `not_found`;
- interrupt observed as `cancelled`;
- steering acknowledged with behavior `not_observed`;
- the suppressed-acknowledgment control never claiming `acknowledged`;
- a missing brief, a fixture outside the root, and a content digest mismatch refused before any app-server starts;
- the `kind` 0.155.1 added to command approvals recorded with the action: `command` on the accepted case, `writeStdin` on the declined one, and `command` again where the optional field is absent;
- an unqualified executable refused at service start;
- a permission-grant request refused natively, with no action surfaced and nothing answered, failing on the named assertion `permission_grant_surfaced_as_action` rather than on a timeout if one ever is;
- daemon restart with the same host and app-server, one turn and an advanced generation;
- a lost host after acknowledgment reported `unknown` with no respawn;
- discovery reporting authentication only after a launch observed it;
- widening approval decisions (`acceptForSession`, execpolicy and network-policy amendments) refused as `invalid_envelope`, with no control or native answer;
- a 3-second execution deadline stopping the turn with a real `turn/interrupt`, recorded `deadline_stop` outcome `interrupted` and a clean app-server exit;
- a configured `read-only` sandbox making a `workspace-write` request refuse before any app-server starts.

Every matrix submit carries the live timeouts (`delivery` 120 s, `execution_deadline` 600 s) unless a case tests the deadline.

The matrix proves adapter plumbing only. It runs no real Codex, uses no model and establishes no journey.

## M2 live Codex runs

`python3 scripts/codex_live_run.py --run R1|…|R6|model-list|discovery|wrong-executable` drives the user's installed Codex through `pio serve-codex`. It is **not part of CI**, needs an authenticated Codex, and spends real tokens for R1 to R6. Each run uses its own private store and a throwaway fixture repository under `$HOME/pio-m2-live/fixtures`; raw transcripts, task output, configuration copies and the usage ledger stay under `$HOME/pio-m2-live/private` at mode 0700. The public receipt in [codex-live](work/m2/codex-live/) holds digests, identities and observed facts only, with `$HOME` and credentials redacted.

Stops are the **runner's** rules, not the product's: PIO enforces no budget, and every stop below is `scripts/codex_live_run.py` watching observed usage and calling `execution.cancel`. No run starts once cumulative observed usage reaches 800,000 tokens, which is 80% of the owner's 1,000,000 Codex cap; a run whose observed usage passes its own limit is interrupted with `execution.cancel`; a run that ends without a usage report stops the sequence. R1 carries a 50,000 limit and R2 to R6 carry 250,000. A limit can only be enforced when the harness reports usage, so a run stops at the first report above it, not at the limit itself.

`model-list`, `discovery` and `wrong-executable` start no turn and cost nothing. `discovery` queries `execution.discovery.list` twice, against a fresh store and against the store a completed run left behind, to show authentication moving from `unknown` to observed without any app-server starting.

Results and the journey marks they support are in the [M2 acceptance packet](work/m2/ACCEPTANCE.md).

## M3b OpenCode qualification

`pio-opencode` binds the user-selected OpenCode executable before any native work. `target/debug/pio opencode qualify --executable PATH --work DIR [--expected SURFACE]` resolves and hashes the binary, requires version 2.0.1, and digests the command-line surface — the top-level help plus six subcommand helps — against the checked-in [surface identity](../adapters/opencode/2.0.1/surface-identity.json). Exit 0 qualified, 3 refused; refusals are data. Measured: the installed 2.0.1 qualifies with zero drift, and pointing it at Claude Code refuses `version_unavailable` without running a single OpenCode-specific argument.

`target/debug/pio opencode service-admit --config FILE --work DIR` makes the pre-session decisions: the settings allowlist, an environment that must carry no credential variable and nothing outside `PATH`, `HOME`, `USER`, `OPENCODE_CONFIG_DIR`, the dated model exception, and the **owner's exclusion of the Juspay Grid provider**, which is refused outright rather than left unevaluated. The record carries `session_started: false` and the owner's own background service by digest, so a run can prove it did not move.

`session_configuration_guard` is the owner's rule of 2026-09-20: **refuse unless the session's reported provider and model equal the requested ones.** It exists because a missing route does not refuse — measured, it silently substitutes a free built-in model. Unlike Claude Code's `system/init`, ACP reports the session configuration **before any prompt**, so this refusal precedes delivery, and the guard records `checked_before_delivery: true` to keep the two from being read as the same guarantee.

Tests cover an unsupported version refused before any further argument, surface drift naming the command that moved, a session reporting the requested model, a silent downgrade to another provider, a different model from the right provider, a session reporting nothing at all, the excluded provider, a model without the dated exception and a run without a model, and a credential variable in the environment.

`target/debug/pio opencode fake-acp` is a **labeled fake OpenCode ACP server**. It speaks the shapes measured from 2.0.1 at zero tokens — the `initialize` result, and `session/new` returning `configOptions` with the model the session will use — plus the `session/request_permission` exchange, which comes from the **ACP specification and is not measured against 2.0.1**; ADR 005 records it as unverified and PIO forwards no decision whose single-use form it has not measured. It runs no model, tool or command and labels everything `pio-fake-opencode-acp`. It records any forbidden flag it is passed, and any field on a permission outcome that would make the decision outlive the request.

`python3 scripts/opencode_host_matrix.py --out DIR --repetitions 3` is the **offline OpenCode matrix**, run in CI. Ten cases, each three times, against that fake, and **never anywhere near the owner's running service**: the fake is a child of the matrix on stdio, and every case records the owner's `serve --service` process before and after and fails if it moved.

The case that carries the owner's rule is `silent_downgrade_refused_before_any_prompt`. The session reports `opencode/nemotron-3.5-lightning-free` while the run requested a MiniMax model; the guard refuses on both provider and model, and the proof is that **no prompt is ever sent** — `session_created` appears in the fake's markers and `prompt_received` does not. The others cover the requested model allowed, a wrong model from the right provider, a session reporting nothing, the owner-excluded provider, a model without the dated exception, three credential variables, an unqualified executable, the forbidden flags with a control that proves the detector works, and an always-allow option offered and never taken.

`unqualified_executable_refused` caught a real defect on its first run: `service_admission` computed a qualification verdict and admitted the configuration anyway, so an executable with seven drifting commands would have been allowed to start. A verdict nothing acts on is not a check.

Two of the twelve run **through the service**, `pio serve-opencode`: a turn that completes, and — the one that matters — **a downgraded session refused before any prompt**. The host reaches `session/new`, sees the session report a different provider, refuses, and the view records `failed_before_delivery`; the fake's markers show `session_created` and **no** `prompt_received`. This adapter can refuse while the brief is still inside PIO, which the Claude adapter cannot.

A run's receipt carries `delivery_proof_class: null`, because this harness returns no acknowledgment identifier (ADR 005 §7), and `owner_service_untouched`, which the host asserts before completing.

**Scope, stated plainly:** the other ten cases drive the adapter and the fake directly. Restart, reattach and host-loss are **not** yet covered for this harness the way they are for Codex and Claude Code.

## The shared host lifecycle

Codex, Claude Code and OpenCode differ in the protocol they speak and in nothing else that matters to the host. `pio_host::harness::Lifecycle` holds the part that is literally shared, once: detach and the M1 launch fences (invocation identity, no launch already recorded, host slot free), the claim, the guard event that refuses a request broader than the user's configured default, the spawn marker binding the child to the qualification record, the park, the **release gate** held across the first native write, the append-only event file, the control file with at-most-once application, the deadline stop, and the receipt or the known-not-released failure.

What a harness says on the wire stays in its own module; the lifecycle never parses a harness message. The event and control files keep each adapter's own prefix, so extracting this changed no path the service reads.

**The proof that the extraction changed nothing** is that every existing count is unchanged: the offline Codex matrix is **51 of 51** across its 17 cases, the public Unix process matrix is **72** (51 pass, 6 expected property failures, 9 expected defense refusals, 6 expected classifier failures), the diagnostic fake-host matrix is **54** (39/3/9/3), and caller recovery passes.

## M3 Claude Code qualification

`pio-claude` binds the user-selected Claude Code executable before any native work. `target/debug/pio claude qualify --executable PATH --work DIR [--expected SURFACE]` resolves the path to the binary that actually runs, hashes it, requires version 2.1.278, and then digests the command-line surface: the top-level help plus seven subcommand helps, compared with the checked-in [surface identity](../adapters/claude/2.1.278/surface-identity.json). Claude Code publishes no schemas, so the surface is what the adapter can pin; it is byte-identical across runs and costs 50 ms, which keeps re-qualification cheap for a cask that self-updates. Exit 0 when qualified and 3 when refused; refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, `surface_drift`). Every invocation uses an isolated `CLAUDE_CONFIG_DIR` and an explicit environment with no credential variable.

A help digest cannot see the wire, so a second artefact is pinned: the [stream identity](../adapters/claude/2.1.278/stream-identity.json) — the `system/init` key set, the capability list, the message sequence, the `result` key set and the product's own default mode and model.

`target/debug/pio claude auth-route --executable PATH (--home DIR | --isolated DIR)` observes which credential route is configured and exits 3 when none is usable. `--home` observes the route the user actually has; `--isolated` is the negative control, and the two differ in exactly one thing so a refusal cannot be an artefact of the environment. The child environment is an allowlist — `PATH`, `HOME`, `USER`, and `CLAUDE_CONFIG_DIR` only when isolating — because measured, a child without `USER` reports a working login as absent. It records only `loggedIn`, `authMethod`, `apiProvider` and `subscriptionType`; `auth status` also prints the account's email and organization, which are dropped at that boundary and appear in no record. `target/debug/pio claude settings-snapshot --config-dir DIR` reports the configured permission mode, the allow-entry count, the model, the always-thinking setting and the enabled plugin names, with a digest of the file.

Cargo tests use labeled fake executables, so CI needs no Claude Code: surface digesting and its listing, drift naming changed, added and removed commands, qualification in an isolated configuration, an unsupported version that never receives Claude-specific arguments, a missing executable refused as data, the route recorded with account identity dropped and proven absent from the record, a missing route reported unusable, a route observable only when `USER` is passed, the permission-mode guard, the settings snapshot across both settings files, both checked-in identities, stream drift naming the field that moved, the single-use decision allowlist with the widening fields absent, and every tool use recorded with targets outside the fixture flagged and a shell command reported as not classifiable rather than assumed contained.

`target/debug/pio claude fake-cli` is a **labeled fake Claude CLI**. It speaks the stream shapes measured from 2.1.278 over stdio — including the awkward one, that `system/init` is not emitted until a message reaches stdin — but runs no model, tool or command, and labels everything `pio-fake-claude-cli`. It answers `--version`, `--help` and `auth status` as well, so one executable can be qualified, drifted and driven. A scenario in `PIO_CLAUDE_FAKE_SCENARIO` selects the version, a help suffix that moves the surface digest, the credential route, a permission request, the tool uses reported and the usage total.

It **records** a permission response that carries a widening field rather than refusing it, so the matrix proves PIO never sends one instead of trusting that it does not; one test drives that detector deliberately to show its absence elsewhere is evidence. Integration tests cover a completed turn with the replay echoing exactly what was sent, nothing emitted before input, an allow that is single-use with the input unchanged, a counted deny, the widening detector, tool uses recorded with an out-of-fixture target flagged, and the non-streaming surfaces.

`pio serve-claude --data-dir DIR --config FILE --socket PATH` is the Claude service. It validates a `pio-claude-service/1` configuration, runs every pre-spawn decision through the same admission code the offline matrix exercises, writes the admission record beside the store, and **refuses to start** when that record refuses. `pio claude host` is launched only by the service controller, through the same `submit_harness` path, launch guard and host-slot fence the Codex host uses.

`target/debug/pio claude service-admit --config FILE --work DIR` makes every decision a service must make **before it spawns a turn**, in a recorded order: the settings allowlist, the environment (which must carry no credential variable and nothing outside `PATH`, `HOME`, `USER`, `CLAUDE_CONFIG_DIR` — plus the fake's scenario variable, accepted only when `labeled_fake` is true), the dated test-only model exception, the permission mode against the user's own settings, qualification, and the credential route. Exit 3 on refusal, with the reasons as data. The record states `stream_spawned: false`: observing the route does run `auth status`, which makes no model call, takes no brief and starts no session, and the record says so rather than implying it.

`python3 scripts/claude_host_matrix.py --out DIR --repetitions 3` is the **offline Claude matrix**, run in CI. Eleven cases, each three times, against the labeled fake: a turn that completes; the replay acknowledging delivery by exact echo; a permission decision denied and allowed; an out-of-fixture request **classified and declined by PIO** with the tool use still recorded; an unclassifiable request surfaced rather than auto-allowed; a widening decision never sent; and, before any spawn, an unqualified executable, a missing credential route, a permission mode that is not the configured default, and surface drift — each refused. The out-of-fixture case uses an absolute path that begins with the fixture and then climbs out of it, because a relative one would be declined even by a broken classifier and would prove nothing. The drift case pins the fake's own surface as a baseline and then moves one help, so it proves drift rather than merely proving the fake is not Claude Code.

Six of the seventeen run **through the service**, `pio serve-claude` over the public Unix API, which is the only place journal, delivery and usage behaviour can be observed:

- a turn that completes, whose view carries `delivery: acknowledged` with evidence class `native_replay_echo` and proof class `provider_ack_id`, `exit: {code: 0}`, the containment statement, and an observed `claude.tokens.total`;
- a real configuration pointed at an unqualified executable, where the service **refuses to start at all** and the admission record says `stream_spawned: false`;
- a control request PIO will not act on, which still receives the protocol's **error control response** — recording a decline while sending nothing would leave a real harness waiting forever, and the fake records that it was answered;
- a harness that **ignores the interrupt**: SIGINT, a bounded wait, then SIGKILL, with the escalation recorded and usage left **unknown rather than zero**, because a killed child sends no `result`;
- a **daemon restart**, which reattaches to the running host and never re-sends the brief: exactly one `turn_received` marker across both generations;
- a **host lost after release**, where the brief was delivered and then the host and child were killed. Delivery stands, because the replay echo already proved it; the runtime becomes `unknown` and the exit `unavailable`; nothing is re-sent and no second harness is started.

That last case caught a real defect. The provider's adapter-switch check, which stops a restarted daemon resuming someone else's executions, knew only about Codex, so a restarted Claude service refused its own work with `cannot switch persisted execution adapter mode`. It now reads the expected source from the harness table.

**Scope, stated plainly:** the other eleven cases drive the adapter and the fake directly, so they prove the adapter's decisions and the stream shapes rather than the durable host's behaviour.

`python3 scripts/claude_requalify.py --executable PATH --out FRESH_DIR` needs an installed Claude Code and is not part of CI. It is the single re-qualification command: it compares both pinned identities, rerunning the zero-token probe to reach the stream half, and exits 0 clean, 3 on drift and 4 if the user's configuration moved under it. It makes **no model call** and leaves the user's configuration byte-identical, which it checks across `settings.json`, `settings.local.json` and `~/.claude.json`. Findings are in [claude-qualification](work/m3/claude-qualification/README.md). None of this is a live run, a real journey or a model-backed result.
