# Protocol 0.1 consumer boundary

The authoritative release is [v0.1.0](https://github.com/Combraton/protocol/releases/tag/v0.1.0), exact commit `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. The [lock](../../../protocol.lock.json) records independently computed hashes. Read the [released consumer handoff](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/work/release-0.1/CONSUMERS.md), [release limitations](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/release/0.1/README.md) and normative documents at that commit.

Historical candidate wording remains inside some frozen documents, including “Until then, pin nothing” in the handoff and the candidate header in STREAM. The accepted release manifest/tag/record resolves release status; it does not authorize editing frozen artifacts or ignoring normative behavior. Sibling main is newer and was inspected only as repository state.

## Pin verification performed

- Remote annotated tag object: `71856e599200dde618ecd197d985ff230165b71f`; peeled commit exactly matches the requested baseline.
- Manifest source tree and tested-head tree both equal `ad57cc4ef067834c20d868dbcc844f70b8ef223f`. Local Git independently returned the same source tree.
- Downloaded all six release assets. Verified the five entries in `SHA256SUMS`, every one of 539 source-bundle file checksums, and the 420-file normative inventory. Inventory listing digest: `80b39377b10685c29bb5823e69ee539ef91bb1eace5b049f2894b603ce08b41d`.
- GitHub [tagged-commit CI run](https://github.com/Combraton/protocol/actions/runs/35014862971) reported completed/success at the exact source commit. This is upstream CI evidence; this session did not rerun Protocol's build/conformance suite and has no PIO conformance result.

The reproducible local verifier is [verify_protocol_pin.py](../../../scripts/verify_protocol_pin.py). Download into an empty directory, then run:

```sh
gh release download v0.1.0 --repo Combraton/protocol --dir "$PIO_RELEASE_DIR"
python3 scripts/verify_protocol_pin.py --assets "$PIO_RELEASE_DIR"
```

`PIO_RELEASE_DIR` is a caller-chosen temporary/cache directory. Hash checks establish content identity relative to the recorded GitHub release; they are not a cryptographic signature or independent authenticity attestation. Source archive, normative schemas/text and fixtures stay frozen together. The M1 conformance job must build the runner from this archive and register PIO's own participant; no moving-main dependency or reference executor substitution.

## Provider and consumer roles

PIO serves `core/1` and `execution/1`. Execution requires Core events, capabilities and effects. Plan to implement grants for standalone scoped callers. Full-release feature coverage includes discovery, output, actions, controller ownership, workspaces, usage, supported steering/continuation and the optional context integration features below. Capability negotiation reports what actually works for each tested adapter/version; transport support alone is not a positive capability probe.

PIO does not become a Context, Knowledge, Evidence or Verification provider by calling one. CBR serves Core/Knowledge/Context and may host its packet bytes through Evidence. The Evidence endpoint can be a different provider. CBR may call PIO Execution under a separate investigation grant; PIO owns the resulting execution facts.

## Exact optional CBR interactions

| Boundary | Public operations/features | PIO responsibility |
|---|---|---|
| Connect | `core.authenticate` on socket sessions, `core.feature_dependencies`, `core.negotiate`, capability/grant queries | Per-provider audience and credentials; Core/Context majors and item obligation features required only where the request needs them. Missing required feature never becomes optional |
| Prepare context | `context.request.submit`, `context.request.inspect`, `context.packet.inspect`, Core event read/subscribe | Standalone caller stores stable command/request identity, caller-selected items, basis, budget, `selected_by`, obligation and advisory fallback. No preparation deadlock or automatic recursive enrichment |
| Fetch exact bytes | Negotiate `evidence/1` at the artifact's provider; `evidence.inspect`, `evidence.fetch`; retention holds/releases when selected and authorized | Assemble bytes and hash locally against the immutable reference. Packet excerpts from `context.packet.inspect` are not complete-byte evidence, even when they cover the apparent content |
| Execute a binding | `execution.context`, `execution.context_revalidation`; `execution.claim_revalidation` when claims gate work | Carry exact packet/revision/artifact digest, applicability conditions, context request, per-audience fetch grants and selected obligation. Claim revalidation requires context revalidation and `context.claims` at the Context provider |
| Check boundaries | `context.packet.inspect` plus observable local tree/dirty/environment/authority conditions | Re-read at admission, initial dispatch including after queue/recovery, and named supported transitions. Return match/mismatch/unavailable observations; no semantic-truth judgment |
| Correct or supersede | `context.updates`, packet facts and Core events; authorized successor `execution.submit` for changed bindings | Supersession alone is not invalidity unless `require_current` was selected. Required-item correction invalidates reliance even on a pinned revision. Never silently replace packet bytes or mutate an existing binding |
| Cancel/expand | `context.request.cancel`, `context.expand` when negotiated | Subscriber cancellation is not blanket cancellation of shared work; expansion respects separate Evidence rights |
| Publish eligible output | `evidence.upload.prepare`, `evidence.upload.append`, `evidence.seal`, `evidence.upload.abandon` as needed; `execution.evidence_outputs` | Explicit producer grant and durable outbox; actual capture coverage and sealed references. Publish to the named Evidence service, not automatically to a CBR database |
| CBR investigation | CBR calls `execution.submit`, inspect/watch/reconcile/cancel under its own grant | Preserve `origin` job/depth/call budget; no Combraton workflow ID and no client-side CBR enrichment loop. Waiting consumer does not hold the scarce execution slot or exclusive writer needed for preparation |

Normative references: [Execution §§5, 11–14](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/spec/profiles/EXECUTION.md), [Context §§3–10, 14](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/spec/profiles/CONTEXT.md), [Evidence](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/spec/profiles/EVIDENCE.md), [Stream](https://github.com/Combraton/protocol/blob/cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc/docs/spec/bindings/STREAM.md).

Advisory outages show gaps and permit scoped work. Required-before-start outages hold dispatch and release preparatory capacity. Required-before-transition holds only the named enforceable transition; native investigation can continue. If the adapter cannot mediate that boundary, refuse the required guarantee rather than claiming a prompt instruction enforces it. No CBR configuration means no CBR calls for ordinary core work.

PIO normally obtains claim validity observations through Context's `context.claims` packet facts; it need not grow an unrelated Knowledge browser or evaluator. A helper review suggested the full Knowledge/Verification client surface; that expansion is rejected as unnecessary for this agreed slice.

## Conformance and cross-repository handoff

Run the released runner against PIO's participant for advertised Core/Execution features and socket binding. Client-only Context/Evidence composition requires its own client descriptor and positive/negative composition tests. Deterministic fault adapters remain explicitly simulated; real Codex/Claude and real CBR journeys are additional gates. The released reference Execution provider is scripted and is not a production adapter library.

Release limitations remain visible: no Windows, Coordination or Remote trust; unsigned receipts; no fairness bound; mixed-implementation proof is only the released S-A/S-B composition coverage, not general Execution-provider interoperability. Known omitted fixtures do not license untested claims in PIO.

Proposed coordination, without sibling edits: CBR should expose an authenticated Context endpoint with selected obligation features and `context.claims`, identify its Evidence endpoint and supply audience-scoped read grants; later expose the scoped producer path for evidence publication. Both builders can refer to this exact pin and J6. A newly exposed wire gap belongs in Protocol with a distinguishing fixture and a versioned successor, never a local schema extension.

Shared `combraton/docs/STANDALONE-RELEASES.md` at `9af69ce` still says all gates are planned. Its owner should update the Protocol status with the release/CI references above; PIO does not edit the sibling. The workspace-level Protocol kickoff prompt belongs to its owner and was left untouched.
