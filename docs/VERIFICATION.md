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

Restart records `execution.recovery.decided` followed by `execution.host.changed`, bound to the advanced controller generation. An existing child retains its original process/slot identity. Pending marker-present work becomes ambiguous until release/absence evidence reconciles it; an already acknowledged release is preserved. The delivery evidence class **`child_release_marker`** means the identified fake child read its release message and wrote its marker. It is not model comprehension or native-provider acknowledgment. Recovery-state names are no longer used as delivery proof classes; the frozen optional `proof_class` enum is not extended or misused.

[Private packaging and user-job prototypes](../packaging/README.md) document layout, collision refusal, uninstall ownership checks, and the exact lifecycle command used in CI. `target/packaging` contains native-manager start/stop evidence and unchanged unrelated-`pio` witnesses.
