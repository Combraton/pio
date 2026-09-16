# Verification available now

PIO contains architecture, implementation-readiness documents and a release-integrity verifier, not a product runtime. Protocol v0.1.0 is released separately. The documentation checks below validate structure only.

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

## Product checks to add with implementation

The [journey-verification matrix](JOURNEYS.md) records the six required journeys and their shared evidence fields. All product journeys remain `not_evaluated` until their actual entry points exist and are exercised. A lower-level test cannot complete a TUI or real-adapter journey.

No runtime build, unit, adapter, memory-evaluation or end-to-end commands exist yet. Add actual reproducible setup/build/test commands here in the same change that introduces the corresponding code, including tool versions and fixtures. Do not manufacture a passing runtime status from documentation checks.

For a change, report the command, exit status, environment, tested revision, real versus simulated dependencies, evidence location and untested limitations. Preserve the producer exit code when displaying shortened logs. Review the relevant diff against an explicit base/head.

## Standalone release evidence

The [Protocol pin procedure](work/standalone-0.1/PROTOCOL.md) uses Python 3.12+ (tested with 3.14.5) and downloaded release assets:

```sh
python3 scripts/verify_protocol_pin.py --assets "$PIO_RELEASE_DIR"
```

This checks pinned asset hashes, 539 bundle files and the released 420-file normative inventory. It does not build Protocol, run conformance or test PIO. A temporary modified-manifest negative control was refused with exit 1 for the expected checksum mismatch; see [pin evidence](work/standalone-0.1/evidence/pin-checks.json).

Follow [standalone release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md). Core no-optional-service tests, protocol conformance, real-adapter integration, comparative outcomes and UI usability are separate evidence classes. The [benchmarks repository](https://github.com/Combraton/benchmarks) owns cross-product scenarios/results, not this service's normative contract. No runtime benchmark has been implemented or run by the documentation setup.
