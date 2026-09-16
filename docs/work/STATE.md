# Current session state — PIO

Dated navigation snapshot; reconcile with Git and [issue #1](https://github.com/Combraton/pio/issues/1) before continuing. Issues own live progress; this file grants no authority.

- **Updated:** 2026-09-16. **Owner:** Codex, standalone PIO implementation lead.
- **Task/branch:** standalone 0.1 readiness; [PR #2](https://github.com/Combraton/pio/pull/2), `codex/standalone-readiness`, based on clean remote-matching `e65b7c02318e71e848ab7c8b3f8efab3489fb2d2`. Read actual Git/PR head on resume.
- **Deliverables:** [plan](standalone-0.1/PLAN.md), [accepted stack](../decisions/001-standalone-stack.md), [journeys](../JOURNEYS.md), [Protocol/CBR boundary](standalone-0.1/PROTOCOL.md), [handoff](standalone-0.1/HANDOFF.md).
- **Protocol correction:** v0.1.0 is released at `cbf8e4df9df2ca8a9b50264df6acace6e4c3a0fc`. Tag, manifest/assets, 539 bundle files and 420 normative entries verified. [Pin](../../protocol.lock.json) replaces the earlier pre-release observation; later main is not the contract.
- **Shared source:** Combraton `9af69ce966bfacf0deb03606d99f28a355d1f944`; other revisions are in the handoff. No sibling repository was edited.
- **Product status:** readiness only. No runtime, supported adapter, product build or journey pass. Codex 0.146.0 and Claude Code 2.1.273 are validation candidates; authentication unknown. Both remain in the accepted release scope. M0 was accepted by the owner as documentation at `85255d9` against `e65b7c0`; no runtime milestone was accepted.
- **Owner dispositions:** stack accepted; Python bridge conditional on M3; macOS arm64 + Linux x86_64 accepted with both-platform build/conformance CI from M1; Claude API-key/provider route accepted and claude.ai login unsupported until approved. License and confirmatory thresholds remain later decisions.
- **M1 carry-forward:** canonical per-file Codex schema comparison, trusted-project config side-effect accounting, named participant feature/control coverage and the owner’s exact acceptance evidence are recorded in the [plan](standalone-0.1/PLAN.md).
- **Checks:** release verifier and tampered-manifest negative control executed; Codex schema generation succeeded. [Initial validation](standalone-0.1/evidence/validation.json) and [owner-disposition follow-up checks](standalone-0.1/evidence/validation-followup.json) record documentation/diff evidence. Product journeys remain `not_evaluated`.
- **Resources:** one read-only MiniMax review worker completed. No PIO product service/host started; unrelated harness processes left alone. Historical installed `pio` is a different program.
- **Prompt disposition:** user attachment preserved; no owned disposable prompt retired. Separate Protocol kickoff stays under its owner's control.
- **Next:** await separately authorized merge of PR #2; only then branch M1 from main. This follow-up records the dispositions for the owner-requested ready transition; read live PR state before acting. No merge, tag or release publication authorized here. Update this snapshot at every M1 checkpoint.

Git preserves previous snapshots; the handoff preserves evidence, limitations and continuation details.
