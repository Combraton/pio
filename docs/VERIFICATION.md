# Verification available now

PIO now has an M1 Cargo skeleton and pinned conformance pipeline. Its participant advertises no implemented profiles yet. Protocol v0.1.0 is released separately. Documentation checks validate structure only.

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
python3 scripts/check_docs.py
git diff --exit-code -- Cargo.lock
python3 scripts/conformance.py --out target/conformance
```

These are the commands in [runtime CI](../.github/workflows/runtime.yml). CI uses macOS 15 arm64 and Ubuntu 24.04 x86_64 and asserts the architecture. These are tested CI environments, not a broader minimum-OS support claim. Use `target/debug/pio` explicitly: an unrelated historical `pio` may already be installed on PATH. The skeleton only supports `--version` and `participant`; its conformance service refuses startup.

The conformance command downloads the six pinned release assets into a fresh temporary directory, runs the existing integrity verifier, extracts the verified source archive, builds `combraton-conformance` with its released lockfile, runs runner self-tests and fixture validation, and runs all fixtures against PIO's descriptor. No sibling clone or pre-existing cache is required. It preserves the runner's exit status and unmodified manifest/transcripts; `pio-report.json` reports native outcome classes and every unsupported/skipped coverage limit. CI uploads the rendered descriptor, target, pin receipt, environment, manifest and transcripts even on failures. Empty claims in this first skeleton mean no protocol coverage, not M1 acceptance. Cargo tests currently contain no behavior tests.

For offline asset reuse, append `--assets "$PIO_RELEASE_DIR"`; verification still runs. Choose a fresh output path for every run: existing evidence directories are never overwritten. Cargo downloads still need either network access or an existing registry cache. No real adapter, UI, memory-evaluation or end-to-end journey has passed.

For a change, report the command, exit status, environment, tested revision, real versus simulated dependencies, evidence location and untested limitations. Preserve the producer exit code when displaying shortened logs. Review the relevant diff against an explicit base/head.

## Standalone release evidence

The [Protocol pin procedure](work/standalone-0.1/PROTOCOL.md) uses Python 3.12+ (tested with 3.14.5) and downloaded release assets:

```sh
python3 scripts/verify_protocol_pin.py --assets "$PIO_RELEASE_DIR"
```

This checks pinned asset hashes, 539 bundle files and the released 420-file normative inventory. It does not build Protocol, run conformance or test PIO. A temporary modified-manifest negative control was refused with exit 1 for the expected checksum mismatch; see [pin evidence](work/standalone-0.1/evidence/pin-checks.json).

Follow [standalone release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md). Core no-optional-service tests, protocol conformance, real-adapter integration, comparative outcomes and UI usability are separate evidence classes. The [benchmarks repository](https://github.com/Combraton/benchmarks) owns cross-product scenarios/results, not this service's normative contract. No runtime benchmark has been implemented or run by the documentation setup.
