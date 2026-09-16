# Current session state — PIO

Dated navigation snapshot; reconcile with Git and [issue #1](https://github.com/Combraton/pio/issues/1) before continuing. Issues own live progress; this file grants no authority.

- **Updated:** 2026-09-16. **Owner:** Codex, standalone PIO implementation lead.
- **Task/branch:** standalone 0.1 readiness; [draft PR #2](https://github.com/Combraton/pio/pull/2), `codex/standalone-readiness`, based on clean remote-matching `e65b7c02318e71e848ab7c8b3f8efab3489fb2d2`. Read actual Git/PR head on resume.
- **Deliverables:** [plan](standalone-0.1/PLAN.md), [proposed stack](../decisions/001-standalone-stack.md), [journeys](../JOURNEYS.md), [Protocol/CBR boundary](standalone-0.1/PROTOCOL.md), [handoff](standalone-0.1/HANDOFF.md).
- **Protocol correction:** v0.1.0 is released at `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. Tag, manifest/assets, 539 bundle files and 420 normative entries verified. [Pin](../../protocol.lock.json) replaces the earlier pre-release observation; later main is not the contract.
- **Shared source:** Combraton `9af69ce966bfacf0deb03606d99f28a355d1f944`; other revisions are in the handoff. No sibling repository was edited.
- **Product status:** readiness only. No runtime, supported adapter, product build or journey pass. Codex 0.146.0 and Claude Code 2.1.273 are validation candidates; authentication unknown. Both remain in the full proposed release.
- **Owner checkpoint:** stack/hosting and Claude SDK bridge; macOS arm64 + Linux x86_64 scope; Claude authentication route. See [source findings](../decisions/001-standalone-stack.md). License and confirmatory thresholds are later decisions.
- **Checks:** release verifier and tampered-manifest negative control executed; Codex schema generation succeeded. [Validation](standalone-0.1/evidence/validation.json) records documentation/diff checks. Product journeys remain `not_evaluated`.
- **Resources:** one read-only MiniMax review worker completed. No PIO product service/host started; unrelated harness processes left alone. Historical installed `pio` is a different program.
- **Prompt disposition:** user attachment preserved; no owned disposable prompt retired. Separate Protocol kickoff stays under its owner's control.
- **Next:** owner review of M0 decisions, then M1 core/host/packaging experiments. No merge, tag or release publication authorized here.

Git preserves previous snapshots; the handoff preserves evidence, limitations and continuation details.
