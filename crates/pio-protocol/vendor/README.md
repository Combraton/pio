# Pinned Protocol schemas

These Core, core-test, Execution, transitive common-definition and conformance launch schemas are unmodified files from Protocol v0.1.0, source commit `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. Their MIT license is retained in LICENSE. They define validation only; PIO does not include or invoke the released reference provider.

The [conformance command](../../../scripts/conformance.py) verifies the release archive using [protocol.lock.json](../../../protocol.lock.json), then compares every vendored JSON file byte for byte against that verified archive before building or running the released runner. Schema changes belong upstream; do not widen these copies locally.

Common definitions from Context, Evidence and Knowledge are required references in the frozen Execution schemas. Vendoring them does not claim those profiles. The verifier currently checks 64 unchanged schema files.
