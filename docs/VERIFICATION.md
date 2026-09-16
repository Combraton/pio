# Verification available now

PIO now has an M1 Cargo workspace, experimental fake-host process slice and pinned conformance pipeline. Its conformance-only Unix participant advertises Core and core-test with grants, events and capabilities. Protocol v0.1.0 is released separately. Documentation checks validate structure only.

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

Prerequisites: Rustup, Python 3.12+, Git, GitHub CLI (`gh`), C compiler/linker and network access. Authenticate `gh` for release asset downloads (CI uses its read-only `GH_TOKEN`). The pinned toolchain is Rust 1.97.1; Cargo.lock is committed. From a clean clone on macOS arm64 or Linux x86_64:

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
python3 scripts/conformance.py --out target/conformance
```

These are the commands in [runtime CI](../.github/workflows/runtime.yml). CI uses macOS 15 arm64 and Ubuntu 24.04 x86_64 and asserts the architecture. These are tested CI environments, not a broader minimum-OS support claim. Use `target/debug/pio` explicitly: an unrelated historical `pio` may already be installed on PATH. The binary supports `--version`, `participant`, `conformance --data-dir DIR --config FILE --socket PATH`, and the experimental `fake` namespace. The runner creates the private socket directory, launch configuration and credentials; no user credential or configuration is used. The conformance service exits when its launcher closes stdin. It exposes core-test only through this test entrypoint.

The conformance command downloads the six pinned release assets into a fresh temporary directory, runs the existing integrity verifier, extracts the verified source archive, builds `combraton-conformance` with its released lockfile, runs runner self-tests and fixture validation, and runs all fixtures against PIO's descriptor. No sibling clone or pre-existing cache is required. It preserves the runner's exit status and unmodified manifest/transcripts; `pio-report.json` reports native outcome classes, counts for each fixture directory, and every unsupported/skipped coverage limit. Vendored validation schemas are compared byte for byte with the verified archive; `schema-verification.json` records the result. CI uploads the rendered descriptor, target, pin receipt, environment, manifest and transcripts even on failures. The [Core checkpoint](work/m1/CORE.md) reports 164 passes, 115 unsupported and 1 skipped, with no failures. Unsupported outcomes are never counted as passes. Cargo unit tests exercise admission/conflicts, transition validity, generation fencing, strict encoding, atomic transaction rollback, response loss and effect obligation storage. Effect storage tests do not establish public effect lifecycle conformance. Process behavior is exercised by the separate fake-host matrix command; its failures fail CI.

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

This diagnostic JSON interface is not Core/Execution or an installable product CLI. Every execution is labeled `fake-host`. The child performs only bounded deterministic output and waiting. Core/Execution, grants, capabilities, event subscriptions, caller operation persistence, discovery/workspaces/usage and the released test controls remain to implement. Persistent launch guards and an external controller witness hold detected rollback; comprehensive backup/restore and storage-failure certification remain separate work. Do not remove guard/witness files to bypass a refusal.
