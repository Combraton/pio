# Version policy (D12)

PIO pins each harness to one exact, measured version and refuses any other.
This is deliberate, not a maintenance gap: qualification is what makes the
rest of this project's claims true (the command-line surface, the wire
schema or stream shapes, the permission and cancel behavior) are all
measured against one binary, and a harness that updates itself underneath
PIO can change any of them silently. A version PIO has not measured is a
version PIO has no evidence about, so it is refused rather than assumed
compatible.

All three harnesses update themselves outside PIO's control — Codex's own
installer, the Claude Code Homebrew cask, and OpenCode's npm package all
track latest — so this has already happened once for each of them:

| Harness | v0.1 pin | Previous pin | Re-pinned | Evidence |
| --- | --- | --- | --- | --- |
| Codex | **0.157.0** (`rust-v0.157.0`, source `00c972ed5d6ff6499317fd41b7f23605b8e6850d`) | 0.155.1 | 2026-09-25 | [`docs/work/m2/codex-qualification/README.md`](work/m2/codex-qualification/README.md), "Re-qualification at 0.157.0" |
| Claude Code | **2.1.281** | 2.1.278 | 2026-09-26 | [`docs/work/m3/claude-qualification/README.md`](work/m3/claude-qualification/README.md), "Re-pin to 2.1.281" |
| OpenCode | **2.0.11** | 2.0.1 | 2026-09-21 | `crates/pio-opencode/src/lib.rs` module doc |

The pin lives in one place per harness (`PINNED_VERSION` in `pio-codex`,
`pio-claude` and `pio-opencode`), with the committed identity it was
qualified against under `adapters/<harness>/<version>/`.

## What a user sees

A harness whose reported version does not equal the pin is refused
`unsupported_version` before any harness-specific argument is sent — the
version probe is the only thing that ran (`crates/pio-codex/src/lib.rs`,
`crates/pio-claude/src/lib.rs`, `crates/pio-opencode/src/lib.rs`). The
refusal itself carries three facts, so nobody has to go read this file to
learn them:

- `pinned` — the version PIO actually qualified and will run.
- `observed` — the version the installed executable reports.
- `detail` — one paragraph naming this policy and the exact zero-token
  commands that re-qualify the installed version (below).

## Re-qualifying a harness at zero model tokens

Every procedure below runs the harness only for its own version/help/auth
probes: no turn is sent, no model is called, and nothing the harness bills
for happens. Re-qualifying is the only way to move the pin; PIO never
qualifies itself against a version nobody has looked at.

**Codex.** `pio codex qualify --executable <path> --work <scratch>`
qualifies against the committed schema identity by default (pass
`--expected <file>` to diff against the *previous* identity first and see
exactly what moved). On refusal, regenerate the schema with
`pio codex schema-identity <dir>`, review what changed (a per-file diff, as
`docs/work/m2/codex-qualification/README.md` did for 0.155.1 to 0.157.0),
update `PINNED_VERSION`/`PINNED_TAG`/`PINNED_SOURCE` and the committed
`adapters/codex/<version>/` identity from the result, then re-run
`scripts/codex_host_matrix.py` and `scripts/codex_offline_probe.py`.

**Claude Code.** `python3 scripts/claude_requalify.py --out <scratch>`
qualifies the installed cask against the committed surface and stream
identity in one command, at zero tokens (its own docstring explains why:
authentication fails before any model call, so nothing is spent). Add
`--isolated` to also skip reading the owner's own configuration, or
`--update-baseline` to write a fresh baseline into `adapters/claude/` once
the drift has been reviewed (as the 2.1.278-to-2.1.281 re-pin did). Update
`PINNED_VERSION` from the result, then re-run `scripts/claude_host_matrix.py`.

**OpenCode.** `pio opencode surface-identity --executable <path>
--work <scratch>` captures the installed executable's command-line surface;
compare it with the committed `adapters/opencode/<version>/surface-identity.json`
to see what moved. Then `pio opencode qualify --executable <path>
--work <scratch>` (again, `--expected <file>` diffs against the previous
identity first). Update `PINNED_VERSION` and the committed identity from the
result, then re-run `scripts/opencode_host_matrix.py`.

None of the three procedures needs a live model call, a budget, or the
owner's credentials: every one runs in an isolated home/config directory
with no credential passed, exactly as qualification does inside PIO itself.

## Scope

This is a v0.1 policy for exact-version pins measured by hand, one owner
decision at a time (D12 is a recorded limitation, not a promise of
automatic tracking). It says nothing about which version is "better" or
whether a harness's self-update is safe to run generally — only that PIO
will not drive a version it has not qualified.
