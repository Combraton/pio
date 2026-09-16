# ADR 001 — standalone implementation stack

- Status: **accepted by the owner**, 2026-09-16, following review of PR #2 at `85255d9` against `e65b7c0`. The Python bridge remains conditional on the M3 experiment. This is a design decision, not runtime acceptance. **Amended by the owner on 2026-09-16 after M1 acceptance**; see [the amendment](#amendment--owner-decisions-after-m1-acceptance-2026-09-16), which supersedes the sections it names.
- Date: 2026-09-16. Author: Codex. Authority: explicit owner acceptance in the implementation-readiness task; recorded in the [plan dispositions](../work/standalone-0.1/PLAN.md#owner-decision-dispositions).
- Scope: PIO internals and distribution; preserves [accepted baseline](https://github.com/Combraton/combraton/blob/9af69ce966bfacf0deb03606d99f28a355d1f944/docs/architecture/BASELINE.md). It changes no released Protocol artifact.

## Accepted direction

Use Rust 1.97.1 as the initial build-toolchain candidate, Tokio for bounded asynchronous I/O, SQLite through rusqlite for the journal/projections/outbox, content-addressed output files, Clap for CLI parsing and Ratatui/Crossterm for terminal presentation. Pin resolved package versions/checksums in the M1 lockfile; no dependencies have been installed by this checkpoint. The installed Rust toolchain and Protocol toolchain both report 1.97.1; that is compatibility evidence for investigation, not a successful PIO build.

Use one package with separately invocable client, daemon and host modes. Logical modules remain separate: `protocol` validation and negotiation; execution core; durable host/adapter; standalone caller policy and operation identities; CLI/TUI. A client persists its pending operation identity before calling the service. It never owns the authoritative execution journal. Host observations reach the journal through authenticated fenced messages; host-local spool/launch records are owned PIO state, not a second independent product store or a direct CBR write path.

The public transport implements Protocol's authenticated Unix socket binding. The Codex adapter's JSON messages are **not** the public Protocol framing: Codex omits the JSON-RPC header; Protocol requires it. Keep independent codecs and strict Protocol integer/duplicate-key/size rules. Generic JSON deserialization that silently accepts duplicate keys is insufficient.

## Decisions, evidence and alternatives

| Choice | Reason and consequence | Alternative and validation |
|---|---|---|
| Rust/Tokio core | Follows accepted starting preference; bounded tasks, typed ownership and native deployment fit a supervisor. Tokio documents that dropping a child handle does not normally kill the process; neither handle lifetime nor async cancellation is recovery | Go is viable for static binaries; TypeScript offers direct SDK ergonomics; Python lowers bridge complexity but adds a full runtime dependency. M1 tests explicit lifecycle ownership, packaging and shutdown; no language is claimed intrinsically crash-safe |
| SQLite, single writer, WAL, `synchronous=FULL`, transactional outbox | Local atomic admission/event/receipt transactions, replayable projections. Payloads written and synced before sealing; schema migrations are explicit writes | PostgreSQL adds a service prerequisite; flat logs require custom indexes/transactions. SQLite WAL needs same-host shared memory and does not support a network filesystem. FULL trades latency for commit durability; OS/storage failure still needs testing. M1 crash/disk-full/restore tests decide viability |
| Durable per-session host under OS supervision | Terminal and daemon connection lifetime must not own native transport. The host owns native child pipes, approval waits, bounded spool, launch record and controller fence | A daemon owning every child is simpler but loses native transports on daemon crash. A host per execution can isolate better but complicates intentional multi-turn native sessions. M1 defines slot/session/attempt mapping before coding; no auto-respawn of ambiguous work |
| User service, independent host jobs | Prototype launchd user jobs on macOS and systemd user units on Linux; explicit install/uninstall, no root or global hooks. Separate host lifetime must survive daemon restart | Plain detached subprocesses need explicit descriptor/session handling and reliable ownership enumeration. Do not use broad kill-by-name or `KillMode=process` as a shortcut. Validate actual supervisor child cleanup; service-manager presence alone proves nothing |
| Ratatui/Crossterm TUI | Reuses Rust domain client; explicit keyboard/resize state over public operations. Ratatui supplies rendering, not durable execution | Textual/Python and Bubble Tea/Go are credible but introduce a second presentation runtime. M4 usability experiment chooses based on observed behavior, not screenshots |
| Git CLI worktrees | Isolated write checkouts with recorded base, dirty/untracked coverage and lease epoch; preserve user checkout | Shared writer serialization is an explicit alternative. Neither is a sandbox; use harness-native restrictions only at their observed enforcement level. Hard requirements that cannot be enforced are refused |
| Versioned archives and reproducible source build | CLI/daemon/host binary plus optional Claude bridge dependencies, manifests/checksums, rollback/export instructions; collision-aware installation | Package managers may follow. Do not overwrite the existing unrelated `pio` binary. Side-by-side absolute-path invocation until the owner chooses replacement; no automatic harness upgrade |

Primary library/OS documentation inspected on 2026-09-16: [Tokio process lifecycle](https://docs.rs/tokio/latest/tokio/process/), [SQLite WAL](https://sqlite.org/wal.html), [SQLite synchronous](https://sqlite.org/pragma.html#pragma_synchronous), [Ratatui](https://ratatui.rs/), [Apple launchd jobs](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html). Linux supervisor semantics need a pinned-source/manual check in M1; the upstream web manual could not be retrieved in this checkpoint. No Linux process experiment has run.

Core/Execution persistence is refined by [ADR 002](002-protocol-journal.md): move Core onto the shared journal before Execution; the earlier blob is a conformance stopgap.

## Native harness interface findings

### Codex

Candidate: **0.146.0**, tag `rust-v0.146.0`, peeled source `e363b08c9175ac1cbe5893615dd2cb9ddf95043b`. Read the [pinned app-server source documentation](https://github.com/openai/codex/blob/e363b08c9175ac1cbe5893615dd2cb9ddf95043b/codex-rs/app-server/README.md) and [current official documentation](https://learn.chatgpt.com/docs/app-server). Local `generate-json-schema` exited 0: 275 JSON files, listing digest in [inspection evidence](../work/standalone-0.1/evidence/readiness.json). The installed binary was hashed; no reproducible-build equivalence to the Git source is claimed.

Owner review reproduced byte-nondeterminism in `codex_app_server_protocol.v2.schemas.json` across runs of that same binary, with identical parsed JSON. The raw listing digest identifies the original capture, not a stable schema compatibility fingerprint. M2 must compare canonical parsed JSON per file (preserve arrays and values, normalize object-key order), never the raw bytes of that file; retain raw hashes only as capture provenance.

Owner source review also found that `thread/start` with `cwd` and a `workspace-write` sandbox at this pinned commit writes a trusted-project entry into the user's `~/.codex/config.toml`. This is a durable native side effect outside the task worktree. PIO must disclose it and either avoid it through a validated configuration path that preserves native policy, or explicitly account for it in the launch/effect contract and evidence. Experiment 3 must observe before/after configuration state in an isolated test environment; do not exercise it against the user's real configuration merely to probe discovery. This follow-up records the owner's evidence; it did not run a live thread or modify configuration.

Recommend a host-owned app-server over stdio, using explicit thread/turn IDs, streaming events, native approval requests and interrupt. Native resume is historical-session continuation, not proof a killed live turn continued. Advertise steering only after real acknowledgment/outcome tests. Never select the unrestricted `thread/shellCommand` or experimental process API to evade the execution grant. Current documentation and local help label app-server experimental; qualify the exact surface and treat version drift as requiring re-probe. Stable schema fields do not make every transport or endpoint production-supported upstream.

### Claude Code

Candidate: **2.1.273**. Discovery via the historical local dispatcher initially reported 2.1.270; direct version/help subsequently found 2.1.273 and a symlink to that installation. The record cannot establish what caused the change. This is why adapter checks must bind the resolved executable, version and hash. No authentication probe or model run occurred.

Recommend an optional thin Python bridge using official `claude-agent-sdk` **0.2.153**, source `763922b0de2c9fb5371504d73ae54cde49536fe1`, with `ClaudeSDKClient`, an explicit selected `cli_path` and structured input/output. This source was inspected, not installed or executed. It declares Python >=3.10; select and lock the distributed interpreter/dependencies during the M3 packaging trial.

Pinned source establishes three consequential details:

- [Client](https://github.com/anthropics/claude-agent-sdk-python/blob/763922b0de2c9fb5371504d73ae54cde49536fe1/src/claude_agent_sdk/client.py) exposes streaming conversation and interrupt controls; do not replace its loop with model API calls.
- [Transport construction](https://github.com/anthropics/claude-agent-sdk-python/blob/763922b0de2c9fb5371504d73ae54cde49536fe1/src/claude_agent_sdk/_internal/transport/subprocess_cli.py#L565) supplies an empty system prompt when none is selected. PIO must explicitly select the native `claude_code` preset and verify project instructions, settings and tool configuration; SDK defaults are not evidence of native fidelity.
- [Transport close](https://github.com/anthropics/claude-agent-sdk-python/blob/763922b0de2c9fb5371504d73ae54cde49536fe1/src/claude_agent_sdk/_internal/transport/subprocess_cli.py#L945) closes stdin, waits, then may terminate/kill the child. Detaching the TUI must not close this bridge. A surviving bridge/host and a restarted SDK conversation are different recovery paths.

[Official Python reference](https://code.claude.com/docs/en/agent-sdk/python) documents explicit CLI selection and permission callbacks; [permission documentation](https://code.claude.com/docs/en/agent-sdk/permissions) establishes that callbacks are not universal interception of already allowed tools. Preserve effective native rules; no bypass mode, fabricated approvals or global hooks. Admission reports actual enforcement coverage.

[Anthropic's SDK overview](https://code.claude.com/docs/en/agent-sdk/overview) restricts offering claude.ai login/rate limits through third-party SDK products without prior approval and points to API-key authentication. The owner accepted API-key or provider authentication as PIO's qualified route; **claude.ai login is unsupported until an approved route exists**. This limitation is also in the README. Switching from SDK to raw CLI does not itself establish an approved product authentication route.

PIO will invoke the user-selected installed CLI via explicit `cli_path`, conditional on version qualification. Executable selection and authentication selection are separate. M3 must verify explicit API/provider authentication for that child process and reject missing or ambiguous configuration rather than silently falling back to cached claude.ai credentials. Preserve the user's normal interactive login and native permission/settings behavior; do not log them out, copy their tokens or change global configuration as a convenience. Whether a cached login technically works is not evidence that PIO can advertise it as supported.

Alternatives: direct Rust CLI streaming removes the Python bridge but requires validated native control/approval semantics; TypeScript SDK bridge is viable with a Node runtime. A generic PTY wrapper can preserve interactive text but cannot supply equivalent acknowledgment or recovery guarantees. Reconsider only after the bounded M3 trial, without shrinking the two-harness release silently.

## Recovery and enforcement rules to implement

Persist launch intent before spawn; acquire a fenced host slot; record host/process start identity before prompt release; persist effect uncertainty before native I/O. A recovered host permits inspection and cursor replay before mutation. A stale controller cannot release prompts or answer approvals. OS PID reuse, missing native session, live descendants, lost ack and restored old database keep explicit uncertainty until reconciled. A bare PID or store UUID cannot prove ownership or detect all rollback.

Semantic events and completion receipts require durable retention. Spool limits may lose telemetry only with explicit dropped-range evidence; semantic-store failure stops new effects. Observed tokens, native reported cost, estimates, subscription coverage and unknown liability remain separate. Native controls may refuse cancellation; cancellation does not settle external effects.

The accepted stack supersedes the README's unresolved stack preference; it does not supersede architectural boundaries or turn experiments into passes. macOS arm64 and Linux x86_64 are accepted targets, with both-platform build and conformance CI required from M1 onward. The Python bridge remains conditional on M3. Failed experiments remain in the task evidence; a changed direction requires an explicit amendment. M0 is accepted only as a documentation checkpoint; merge, tag and publication remain separate authorizations.

## Amendment — owner decisions after M1 acceptance (2026-09-16)

Recorded at the start of M2: base `9cf70474c28f549650e6b48e8be20ae88426a1b0` (merge of PR #4), [issue #5](https://github.com/Combraton/pio/issues/5). These owner decisions amend the sections named here; the text above remains the record of the earlier position. They establish no runtime behavior, harness support or journey result.

### Harnesses run as the user configured them

PIO drives the harness executables the user already has installed, with the user's own login, configured models and providers, instructions, settings and permissions. **PIO never selects or injects a model or provider** and never adds a provider entry to a harness configuration. A kickoff draft on the same day routed MiniMax through a Codex custom model provider; the owner withdrew it before any such configuration was created.

The harness test scope expands from the two initial candidates to the four harnesses installed on the owner's workstation:

| Harness | Local `--version` observation, 2026-09-16 | Role |
|---|---|---|
| Codex | `codex-cli 0.146.0` | M2 adapter; the user's own login and configured model |
| Claude Code | `2.1.273 (Claude Code)` | M3 adapter; authentication route below |
| OpenCode | `opencode v2.0.1` (`@opencode/cli`, invoked as `opencode2`) | Additional adapter; the owner has MiniMax, GLM and Kimi models configured. Preferred for heavy-usage testing |
| Hermes Agent | `v0.20.1 (2026.8.13)` | Additional adapter; the owner has MiniMax configured |

These are version-command observations, not qualification: each adapter still binds the resolved executable, exact version and binary hash. A separately installed `opencode` reporting 1.18.18 is a different version and is not selected. Test scope is not a support claim; advertised support follows per-version qualification evidence.

**v0.1 release scope remains Codex and Claude Code.** OpenCode and Hermes Agent are test-scope adapters and do not gate the v0.1 release unless the owner promotes them. Test scope is not release scope. The owner confirmed on 2026-09-16 that their adapters come after M3 (PLAN row M3b).

### Live-run spend and concurrency bounds

- **Codex:** at most **1,000,000 tokens** across all its tests.
- **Claude Code:** a separate **1,000,000 tokens** across all its tests. The two caps are per harness, not shared.
- **MiniMax through OpenCode and Hermes:** at most **300,000,000 tokens** combined.
- **GLM and Kimi through OpenCode:** they bill separately from MiniMax. Until the owner sets per-provider caps, which must happen before M3b starts, their usage counts against the 300,000,000 cap **as an interim proxy only** and is recorded as proxy-counted.
- Count tokens per run from each harness's own usage reports and record them per journey in [STATE](../work/STATE.md). A run without a usage report is recorded as unknown liability, never as zero. Stop live work on a harness and report when any cap reaches 80 percent.
- At most **three concurrent live sessions** across all harnesses.
- Comparative measurements name the harness and the model that actually served each run. Results from different models are not equivalent.

### Codex configuration side effect

This amends the Codex finding above. Live Codex runs use the **user's real Codex home**, as an installed product would. Every run captures that configuration before and after, reports any trusted-project entry that `thread/start` added, and changes nothing else. Raw snapshots contain private paths and stay outside Git; the repository records digests and a redacted difference. Offline tests keep using isolated fixtures, and PIO still never reads the user's configuration merely to probe discovery.

### Claude Code authentication

This supersedes PLAN disposition 3 and the Claude Code authentication paragraphs above. **Owner product decision:** PIO drives the user's installed Claude Code executable with the user's own existing login, as a human at the keyboard would, and drives it by API key when the user has one configured.

Anthropic's [SDK documentation](https://code.claude.com/docs/en/agent-sdk/overview) says third-party products must not offer claude.ai login and rate limits without prior approval. PIO's position is that it operates the user's own installation on the user's own machine and never provisions, resells, stores or copies credentials. **This is a policy risk the owner accepts knowingly, not a technical or compliance claim.**

M3 must prove, for every live run, which route served it, from the CLI's initialization or account-source output or from provider-side signals. It defines and tests an explicit precedence rule for when both a login and an API key are present. The negative control: with neither a configured API key nor a usable login, PIO refuses with the specific missing-route reason; it neither succeeds silently nor prompts for login on the user's behalf. PIO never logs the user out, copies tokens, edits global Claude settings for convenience or bypasses permissions.

For M3 experiment 4, the primary criterion is fidelity to the user's own configuration, instructions, settings, permissions and login. Compare the direct CLI stream interface with the Python SDK bridge on that basis. The SDK bridge remains acceptable only if it spawns the user's installed CLI through an explicit path and preserves all of these.

### License, design and walkthrough

- **License:** MIT, matching Protocol; see [LICENSE](../../LICENSE). Nothing is distributed before M6.
- **M4:** before implementation, the independent reviewer supplies TUI mockups produced with Claude Design and iterated against reference terminal interfaces. The builder implements against them and records every deviation with its reason.
- **M6:** before any release candidate is accepted, the reviewer works through the full product as a user would, on a runnable build, with the user's own installed harnesses and existing logins on the walkthrough machine. Findings are review evidence, not builder evidence.
- **Thresholds:** unchanged. Evaluation thresholds and rubric are frozen after the pilot, based on pilot variance, and never moved after confirmatory results are seen.

### Constraints for later milestones

Recorded by the owner on 2026-09-16, after the amendment above was committed.

- **Hermes isolation.** Hermes runs only under an isolated Hermes profile that carries model configuration and nothing else. The owner's real Hermes home runs their scheduled messaging check-ins and other scheduled jobs and **must never be driven by PIO**. If Hermes cannot be isolated that way, defer the Hermes adapter and say so; do not fall back to the real home.
- **Codex live runs.** Every live Codex run works in a **throwaway fixture repository path**, never a user project. PIO never selects Codex's full-access sandbox (`dangerFullAccess`, configuration value `danger-full-access`) or the unsandboxed shell-command surface (`thread/shellCommand`). Consistent with the existing no-weakening rule, PIO also does not select `externalSandbox`, under which Codex enforces no sandbox, or the unsandboxed `process/spawn` API. Every trusted-project entry a run adds to the user's Codex configuration is disclosed. Because each fixture path is new, these entries accumulate; PIO reports them and does not edit the configuration to remove them.
