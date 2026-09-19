# Codex qualification and offline probe evidence

Evidence for [issue #5](https://github.com/Combraton/pio/issues/5), recorded 2026-09-16 on the macOS arm64 workstation. **This is not a live run.** The selected executable ran only with an isolated `CODEX_HOME` and no provider credentials, and no turn was sent. The user's real Codex home was not read or written; its digest was identical before and after each run. Home-relative paths are shown as `~` and scratch paths as `<scratch>`. Raw transcripts and configuration copies stayed outside Git.

## Files

| File | Command | Result |
| --- | --- | --- |
| [qualification.json](qualification.json) | `target/debug/pio codex qualify --executable ~/.local/bin/codex --work <scratch>` | **qualified**, exit 0. `npm_node_wrapper`: wrapper `codex.js` (package 0.146.0, sha256 `134063e1…`), Node first on this shell's PATH (`~/.hermes/node/bin/node`, v22.23.0), native `aarch64-apple-darwin` binary (sha256 `ae1d3ffe…`, layout 0.146.0). Selected and native `--version` both 0.146.0. 275 schema files, canonical listing `65c3bade…` equals [the checked-in identity](../../../../adapters/codex/0.146.0/schema-identity.json); zero drift. |
| [drift-control.json](drift-control.json) | same, with `--expected` pointing at a copy of the identity whose digest for `ApplyPatchApprovalParams.json` was replaced | refused `schema_drift`, exit 3, naming exactly that file as `changed` |
| [wrong-executable-control.json](wrong-executable-control.json) | `pio codex qualify --executable ~/.local/bin/opencode2 …` | refused `version_unavailable`, exit 3; schema generation `skipped` so no Codex-specific arguments reached the other harness |
| [offline-probe-summary.json](offline-probe-summary.json) | `python3 scripts/codex_offline_probe.py --executable ~/.local/bin/codex --out <scratch>` | qualified, then `initialize` and `thread/start` (`cwd` = throwaway fixture repository, `sandbox: "workspace-write"`, `approvalPolicy: "on-request"`) both succeeded; app-server exited 0 after stdin closed |

## Findings

- **Trusted-project side effect confirmed at 0.146.0.** `thread/start` created `config.toml` in the isolated home containing only `[projects."<fixture>"] trust_level = "trusted"`. The capture reports one added fixture entry and no other change.
- **The client sent no model request, but the app-server pre-connects.** Its stderr shows an attempted websocket connection to the Responses endpoint, refused with `401 Unauthorized` because the isolated home had no credentials. With a real login this connection may succeed before any turn; live runs must count it as possible network activity, and PIO must never describe `thread/start` as offline.
- `thread/start`'s response projects the effective sandbox (`workspaceWrite`, no network, no extra writable roots), `approvalPolicy`, `model` and `modelProvider` (`openai` in the isolated home). This is where a live run can observe the configured model and the effective restrictions before any turn.
- The app-server writes its normal session databases (`state`, `logs`, `memories`, `goals`) under `CODEX_HOME`, as ordinary Codex use does.
- Which Node runs the wrapper depends on PATH. On this shell it is Hermes's bundled Node, not a system Node. The durable host's environment must be qualified as it will actually run; a user job with a different PATH can select a different runtime.
- Byte nondeterminism reproduced: two generations differ in raw bytes only in `codex_app_server_protocol.v2.schemas.json`, while all 275 canonical digests are equal.

## Re-qualification at 0.155.1, recorded 2026-09-19

The owner updated their Codex, so 0.146.0 is no longer installed. Everything above stays as the 0.146.0 record. The files below were produced on the same macOS arm64 workstation and, like the ones above, come from isolated Codex homes with no turn sent.

| File | Command | Result |
| --- | --- | --- |
| [version-refusal-at-the-previous-pin.json](version-refusal-at-the-previous-pin.json) | `pio codex qualify` while PIO still pinned 0.146.0 | refused `unsupported_version` (`observed: 0.155.1`), exit 3, schema generation `skipped`, so no Codex-specific argument reached the new binary. This is the refusal that made the re-pin necessary |
| [qualification-0.155.1.json](qualification-0.155.1.json) | `target/debug/pio codex qualify --executable ~/.local/bin/codex --work <scratch>` | **qualified**, exit 0. Wrapper `codex.js` (package 0.155.1, sha256 `61b0194f…`), Node `~/.hermes/node/bin/node` v22.23.0 (`cc616967…`), native `aarch64-apple-darwin` binary (sha256 `8eaf1ad1…`). 312 schema files, canonical listing `93b723a8…` equal to [the checked-in identity](../../../../adapters/codex/0.155.1/schema-identity.json); zero drift |
| [drift-control-0.155.1.json](drift-control-0.155.1.json) | same, with `--expected` pointing at a copy of the identity whose digest for `v2/ThreadStartResponse.json` was replaced | refused `schema_drift`, exit 3, naming exactly that file as `changed` |
| [wrong-executable-control-0.155.1.json](wrong-executable-control-0.155.1.json) | `pio codex qualify --executable ~/.local/bin/opencode2 …` | refused `version_unavailable`, exit 3; schema generation `skipped` |
| [schema-drift-0.146.0-to-0.155.1.json](schema-drift-0.146.0-to-0.155.1.json) | canonical per-file comparison of schemas generated by each version's own binary | 312 files, **37 added, none removed, 71 of the original 275 changed** |
| [offline-probe-summary-0.155.1.json](offline-probe-summary-0.155.1.json) | `python3 scripts/codex_offline_probe.py --executable ~/.local/bin/codex --out <scratch>` | five isolated-home cases, all as described below |

### Findings at 0.155.1

- **The previous identity reproduces from an independent copy of the binary.** The `rust-v0.146.0` release asset `codex-aarch64-apple-darwin.zst` (sha256 `bb99ed84…`) unpacks to a binary whose sha256 is `ae1d3ffe…`, byte-identical to the native binary the npm platform package had installed, and regenerating its schemas reproduces the checked-in 0.146.0 canonical listing `65c3bade…` exactly. The drift above is therefore measured between two verified captures, not against a remembered number.
- **The absent-setting default depends on project trust.** With the project already trusted and nothing configured, `thread/start` projects `workspaceWrite` and `on-request`, which is exactly what the thread-settings guard assumes. On a fresh, untrusted project the same request projects `readOnly` and writes no trust entry. Requesting `workspace-write` is what trusts the project, and that is the one configuration change a live run makes.
- **`approval_policy = "untrusted"` no longer loads.** With it in `config.toml` the app-server exits 1 before answering `initialize`, printing `approval_policy = "untrusted" is no longer supported; remove this setting`. The guard treats a configured `untrusted` as unresolved and refuses before spawning.
- **Requesting `untrusted` for one thread still works.** `thread/start` with `approvalPolicy: "untrusted"` succeeds and projects `untrusted`, so the R5 and R6 plan is unchanged.
- **Command approvals gained `kind`.** Optional, `command` or `writeStdin`, defaulting to `command`. File-change approvals have no such field. PIO records it with the action.
- **A new `remoteControl/status/changed` notification arrives before `thread/started`.** PIO ignores notifications it does not map, and did so here.
- Raw schema bytes were identical across the two 0.155.1 generations in this run. The 0.146.0 nondeterminism is not disproven by one matching pair, so the canonical comparison stays.
