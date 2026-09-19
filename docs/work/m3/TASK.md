# M3 task packet — Claude Code adapter

Durable task definition for [issue #7](https://github.com/Combraton/pio/issues/7). Successor to [M2](../m2/TASK.md), merged as `16fb2291`. Read [STATE](../STATE.md) first, then this, then the [kickoff prompt](KICKOFF.md) if you are a fresh session.

## What M3 must deliver

The second real adapter: PIO drives the user's installed **Claude Code** exactly as they configured it — their own login, or an API key where they configured one — through the same durable host, journal, effects and evidence discipline M2 established for Codex. The standalone CLI submits scoped repository work and records real native output and immutable result evidence.

PIO never selects or injects a model or provider. The one exception is a dated, test-only option for fixture runs, described below.

## The six scope items (owner, 2026-09-19)

1. **Qualify the installed version.** Bind the resolved executable, exact version and binary hash before any native work, the way `pio-codex` does. Refusals are data (`unresolved_executable`, `version_unavailable`, `unsupported_version`, …), never a crash or a panic. An unqualified executable never receives harness-specific arguments, and discovery reports it as not usable. **Version drift requires re-qualification**: the M0 readiness scan recorded 2.1.273 and 2.1.278 is installed today, which is the same situation that forced the Codex re-pin mid-M2.
2. **Observe the credential route, and prove the missing-route refusal.** Record which route actually served a run — subscription login or configured API key — as an observation, never a secret, and never a claim PIO cannot substantiate. A negative control with no usable route must refuse **before any native work** and report that as data. Discovery separates detected, version-supported, reachable and authenticated, and `usable` requires all of them, so a fresh install is truthfully not usable until a launch has observed authentication.
3. **One minimal run as configured.** Nothing passed: the user's own model, settings and permissions. This single run is the proof of the "harness as configured" route, as R1 was for Codex. Smallest useful brief, its own token stop, no retry on failure.
4. **Remaining runs on a cheaper explicit model**, under a **new dated test-only exception** recorded in M3's adapter ADR the way [ADR 003](../../decisions/003-codex-app-server-adapter.md) records Codex's. The service must refuse the model option unless the configuration names that dated token, and must refuse the token without a model so it cannot sit unused. Every receipt records **configured, requested and effective** model. The exception is removed or compiled out before release.
5. **A separate 1,000,000 token cap, stop at 80%.** Independent of the Codex cap. Usage comes from Claude Code's own reports; a run that ends without a usage report is unknown liability, never zero, and stops the sequence.
6. **Incremental, changed-key commits land before M3b or M4.** The journal still diffs whole state per command. No throughput claim until this lands.

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
