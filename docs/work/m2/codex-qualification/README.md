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
