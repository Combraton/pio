# M3 task packet — Claude Code adapter

Durable task definition for [issue #7](https://github.com/Combraton/pio/issues/7). Successor to [M2](../m2/TASK.md), merged as `16fb2291`. Read [STATE](../STATE.md) first, then this, then the [kickoff prompt](KICKOFF.md) if you are a fresh session.

## What M3 must deliver

The second real adapter: PIO drives the user's installed **Claude Code** exactly as they configured it — their own login, or an API key where they configured one — through the same durable host, journal, effects and evidence discipline M2 established for Codex. The standalone CLI submits scoped repository work and records real native output and immutable result evidence.

PIO never selects or injects a model or provider. The one exception is a dated, test-only option for fixture runs, described below.

## Scope

Items 1–6 are the owner's of 2026-09-19. The rest are the independent reviewer's additions of 2026-09-20, measured on this workstation.

### 1. Qualify the installed version, and make re-qualification cheap

Bind the resolved executable, exact version and binary hash before any native work, the way `pio-codex` does. Refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, …), never a crash. An unqualified executable never receives harness-specific arguments, and discovery reports it as not usable.

**This target moves under you.** The M0 readiness record says 2.1.273; **2.1.278** is installed, through a Homebrew cask that tracks latest and **self-updates**. So qualification will be refused mid-milestone at some point, and that refusal is the feature working — Codex did exactly this during M2. Design for it: make re-qualification a cheap, scripted step, not a milestone-sized event.

### 2. Transport is an ADR decision, not an assumption

[ADR 001](../../decisions/001-standalone-stack.md) recommended a Python SDK bridge. The owner's later decision is that **PIO drives the installed `claude` executable as the user would**. The adapter ADR must **choose explicitly** between the SDK bridge and the executable's own structured input and output mode, and justify the choice against that decision and against the Anthropic third-party-login caveat recorded in the [README](../../../README.md). Do not inherit ADR 001's recommendation by default.

**PIO never reads, copies or passes credentials.** Whatever transport is chosen, the harness authenticates itself.

### 3. Credential route: observe it, and prove the missing-route refusal

Record which route actually served a run — the user's own login, or a configured API key — as an observation, never a secret and never a claim PIO cannot substantiate. A negative control with no usable route must refuse **before any native work** and report that as data. Discovery separates detected, version-supported, reachable and authenticated; `usable` requires all of them, so a fresh install is truthfully not usable until a launch has observed authentication.

### 4. Permissions

- **Never** use bypass permissions or the dangerous skip flag. Not in a test, not in a fixture, not behind a flag.
- Add a **guard equivalent to the Codex one**: refuse any requested permission mode broader than the user's configured default, which on this workstation is **accept-edits**. As with Codex, a setting the guard cannot resolve refuses rather than guesses.
- Because edits will not prompt under accept-edits, **design the deny and allow runs around a shell command approval**, not a file edit. That is the pair that actually exercises the decision path, as R5 and R6 did for Codex.
- Forward **only single-use allow and deny**. Never an "always allow", a session-scoped grant or a rule update — the same non-widening rule that limited Codex to `accept`, `decline` and `cancel`.

### 5. The user's plugins load in as-configured sessions

An as-configured session loads **eleven plugins, with hooks and outward-facing tools**. That is part of what "as the user configured it" means, and it is not optional to account for.

- **Record loaded plugin, hook and MCP server names in every receipt — names only**, never their content, arguments or output.
- **Write briefs that need none of them.** The fixture task must be completable without any plugin, hook or MCP server.
- **Decline every tool request outside the fixture workspace**, with a recorded reason, the way the Codex host declines requests it must not answer.
- Include an **offline matrix case for an out-of-fixture tool request**, so the refusal is proven against a labeled fake rather than hoped for.

### 6. Disclose every durable change

Snapshot, before and after each run: the **settings file**, **`~/.claude.json`**, and a **listing of the project transcript directory**. Report by digest with fixture labels, as `pio codex config-diff` does. **PIO edits and removes nothing** — not settings, not transcripts, not a permission the user granted.

### 7. Cost and model (owner decision, 2026-09-20)

The configured model is **Opus with 1M context and always-on thinking**, which is expensive per turn.

- The **single as-configured run** uses the smallest useful brief, a runner stop at **150,000 tokens**, and **no retry on failure**. It is the proof of the "harness as configured" route, as R1 was for Codex.
- **Measure Claude Code's usage reporting granularity offline first**, so that stop is set knowingly. M2 learned this the hard way: Codex reports roughly every 24,000 tokens, so a 50,000 limit actually stopped a run at 72,911. A limit below one reporting step cannot be enforced.
- **All remaining runs use Sonnet 5**, model id `claude-sonnet-5`, under a **new dated test-only exception** recorded in the adapter ADR the way [ADR 003](../../decisions/003-codex-app-server-adapter.md) records Codex's. The service refuses the model option unless the configuration names that dated token, and refuses the token without a model. It is removed or compiled out before release.
- **Confirm the model is available with a zero-token check before the first run**, as `--run model-list` did for Codex.
- **Budget: 1,000,000 tokens total across every run, including the as-configured one. Stop and report at 800,000.** Separate from the Codex cap.
- **Each Sonnet run carries its own runner limit of 250,000.**
- Every receipt records **configured, requested and effective model**, plus **cumulative spend against the cap**.

### 8. Concurrency

The independent reviewer's own Claude Code session runs on this machine. Keep to the **three concurrent live session limit**, and **never touch a session you did not start** — not to inspect it, not to clean it up.

### 9. The gate before any live run

Committed with **CI green**: the adapter ADR, qualification with its negative controls, the offline matrix against a labeled fake, and the missing-credential-route refusal. **Then post the live-run plan on [issue #7](https://github.com/Combraton/pio/issues/7) — fixture paths, exact brief texts, expected token cost, the stops — and wait for the go**, exactly as M2 did on #5.

### 10. Incremental commits

Incremental, changed-key commits land **before M3b or M4**. The journal still diffs whole state per command. No throughput claim until they do.

## Non-negotiables carried from M2

- **Live receipts come from a clean committed head**: `dirty: false`, with the binary's own hash recorded and built from that commit. M2 wasted 72,935 tokens re-running R1 because a staged rename was left in the tree.
- **PIO enforces no budget.** The M2 stops were the live runner's, applied through `execution.cancel`. Product budget enforcement is unproven and M3 may not assume it. A stop is also only as tight as the harness's usage reporting granularity.
- **Disclose every durable change** the harness makes to the user's configuration or state, snapshotted before and after each run and reported by digest. PIO never edits or removes the user's configuration.
- **Throwaway fixture repositories only.** Never a full-access sandbox, never an unsandboxed shell surface, never approving on the user's behalf, never widening standing permissions.
- **Journeys are marked from live evidence only**, in the shared model's vocabulary. A partial acceptance names its accepted properties and its remaining obligations; `not_evaluated` never counts as `pass`.
- **Protocol gaps go upstream** as versioned proposals with demonstrating evidence, never local schema widening. [#13](https://github.com/Combraton/protocol/issues/13), [#14](https://github.com/Combraton/protocol/issues/14) and [#15](https://github.com/Combraton/protocol/issues/15) are open; #14's content path and #15's reattach decision both affect this adapter.

## What M3 inherits and must not break

The durable host, journal, outbox, content-addressed spool, launch guards, host slot fences, controller generations and the ADR 002 capacity bound are all in place and covered by the matrices. The Codex adapter, its offline matrix (17 cases × 3) and its live receipts stay green. The conformance participant and its pinned runner results are unchanged.

## Definition of done

An adapter ADR in the shape of ADR 003; qualification with negative controls; an offline matrix against a labeled fake, so CI needs no Claude Code; the live runs above with receipts in `docs/work/m3/claude-live/`; an acceptance packet naming partial acceptances and remaining obligations; JOURNEYS updated from the live receipts; and CI green on macOS 15 arm64 and Ubuntu 24.04 x86_64.
