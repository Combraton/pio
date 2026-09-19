# PIO

Independent execution and recovery for existing agentic harnesses.

> M1 has an experimental fake-host recovery slice and a journal-backed Core/Execution conformance participant. The scripted fake executor implements the five planned Execution features, public effects and bounded event output. Real adapters remain unimplemented. **M1 is accepted by the owner at `2048e84` (2026-09-16) and merged as `9cf7047`.** M2, the first real adapter (Codex), is in progress on [issue #5](https://github.com/Combraton/pio/issues/5); no real harness has run through PIO yet. This is milestone acceptance, not a released API, real-adapter qualification or a performance claim. The reviewed architecture is `architecture-v1-20260912`, published as `public-development-v1-20260913`. Canonical specifications are available through [the documentation map](docs/README.md). This README is an overview, not the full specification.

PIO supervises real harnesses without replacing their native reasoning, investigation, editing and testing loops. A CLI, CI job, CBR memory investigation or another control plane can use PIO without Combraton or CBR being installed.

## Responsibilities

- Durable request, invocation, attempt, native-session, host and workspace identities.
- Adapter capability reporting, execution admission, scoped permissions and resource accounting.
- Output capture, delivery observations, cancellation, reconnection and reconciliation.
- Fenced ownership, unresolved usage liability and immutable completion receipts.

Journal invocation before dispatch. A lost acknowledgment or expired lease does not prove work stopped. Reconcile ambiguous effects before retrying. Execution completion does not mean project acceptance; an opaque process cannot be promised exactly-once external effects.

## Independence and protocol

Implement the relevant [protocol](https://github.com/Combraton/protocol) Core/Execution semantics and optional evidence/context bindings. A standalone caller supplies work scope and judges results. [Combraton](https://github.com/Combraton/combraton) selects project workflow readiness when composing the full system; PIO decides whether the authorized execution can run here now.

[CBR](https://github.com/Combraton/cbr) may submit a separately granted investigation or supply a context packet. PIO does not choose which project knowledge is mandatory. Waiting for initial context must not hold the scarce execution resource that its preparation needs. CBR direct-model work does not require PIO.

## First milestone

Protocol [v0.1.0](https://github.com/Combraton/protocol/releases/tag/v0.1.0) is released and its exact commit/assets are recorded in [protocol.lock.json](protocol.lock.json). The [standalone readiness plan](docs/work/standalone-0.1/PLAN.md) is accepted as the M0 documentation checkpoint, with Codex-first followed by Claude Code; both remain in the full standalone release target. Stack and platform scope are accepted, with build and conformance CI on macOS arm64 and Linux x86_64 from M1 onward. The [M1 acceptance corrections](docs/work/m1/ACCEPTANCE-CORRECTIONS.md) record the accepted public-process work and its pre-acceptance evidence, following the scripted [Execution checkpoint](docs/work/m1/EXECUTION.md) and [Core checkpoint](docs/work/m1/CORE.md). No real-adapter support is established yet.

Test interruption, lost acknowledgment, a surviving worker, late/conflicting completion and cancellation races. Report actual enforced/mediated/cooperative capabilities by tested version. The [journey matrix](docs/JOURNEYS.md) separates real CLI/TUI, recovery and optional CBR evidence.

Develop alongside CBR; neither project waits for the other's complete feature set or the desktop application.

## Stack and status

Rust/Tokio, SQLite/content-addressed payloads and Ratatui are [accepted implementation choices](docs/decisions/001-standalone-stack.md), with an experimental fake-host slice now present; real-adapter behavior remains unvalidated. Harnesses in test scope: Codex 0.155.1 (M2; re-pinned from 0.146.0 by owner decision on 2026-09-19), Claude Code 2.1.273 (M3), and OpenCode v2.0.1 and Hermes Agent v0.20.1 (added by the owner on 2026-09-16). None is supported yet; support follows per-version qualification. **v0.1 release scope remains Codex and Claude Code;** OpenCode and Hermes Agent are test-scope adapters that do not gate v0.1 unless the owner promotes them. Test scope is not release scope. The Claude Python bridge remains conditional on the M3 experiment. Exact adapter candidates and bounded experiments are recorded there. Puppetmaster supplies evaluated execution ideas, not an adopted controller/workflow engine.

For the shared sequence and self-development boundary, read [BOOTSTRAP](https://github.com/Combraton/combraton/blob/main/BOOTSTRAP.md). No supported runtime release is provided. The project is licensed under [MIT](LICENSE).

## Harness models and authentication

PIO drives the harnesses you already have installed, **exactly as you configured them**: your own login, models and providers, instructions, settings and permissions. PIO never selects or injects a model or provider, never adds a provider entry to a harness configuration, never logs you out, never copies tokens and never bypasses permissions. Recorded evidence names the harness and the model that actually served each run.

- **Codex:** your installed Codex with its own login and configured model. PIO never selects Codex's full-access sandbox or its unsandboxed shell-command surface. Development tests run Codex only in throwaway fixture repositories.

  **Codex will write to your configuration.** Starting a thread in a workspace with a writable sandbox makes Codex add a trusted-project entry for that path to `$CODEX_HOME/config.toml`, exactly as ordinary Codex use does. It happens once per workspace, the entries accumulate, and **PIO never removes them** — removing a trust decision you made is not PIO's to do. PIO snapshots the configuration before and after every run and reports each entry it caused, by path digest. Measured at 0.155.1: a thread started with no sandbox requested on an untrusted project gets `readOnly` and writes nothing; requesting `workspace-write` is what trusts the project.
- **Claude Code (owner decision, 2026-09-16):** PIO drives your installed Claude Code with your own existing login, as you would at the keyboard, or by API key when you have one configured. This supersedes the earlier position that claude.ai login is unsupported. Anthropic's [SDK guidance](https://code.claude.com/docs/en/agent-sdk/overview) says third-party products must not offer claude.ai login and rate limits without prior approval. PIO's position is that it operates your own installation on your own machine and never provisions, resells, stores or copies credentials; the owner accepts this policy risk knowingly, and it is not a compliance claim. M3 must show which route served every run, apply an explicit precedence rule when both a login and an API key exist, and refuse with a specific missing-route reason when neither is usable.
- **OpenCode and Hermes Agent (test scope):** run with the models their users configured. Hermes is driven only through an isolated profile that carries model configuration and nothing else, never a Hermes home that runs other scheduled work.

No adapter or authentication journey has passed yet. Details: [ADR 001 amendment](docs/decisions/001-standalone-stack.md#amendment--owner-decisions-after-m1-acceptance-2026-09-16).

## Working on this repository

Read [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), [the documentation map](docs/README.md), and [verification](docs/VERIFICATION.md). Use existing native harnesses for development. **Combraton self-development is deferred until usable v0.1 releases of all four projects.** The project license is [MIT](LICENSE).

## Standalone-first validation

Own the standalone CLI/TUI and its user-attributed optional CBR client. Keep caller context policy separate from execution-core admission; discovery is not proof of capability. Core use must pass with CBR absent. See [release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md), [PIO client semantics](https://github.com/Combraton/pio/blob/main/docs/spec/STANDALONE-CLIENT.md) and [benchmarks](https://github.com/Combraton/benchmarks). PIO and CBR develop in parallel against the agreed Protocol release surface; accepted standalone releases precede thin Combraton implementation.


The current development checkpoint also exposes `serve-fake` over the public Unix API: it launches an explicitly labeled fake OS process through the durable host. A separate caller ledger and content-addressed output spool are implemented. Process discovery explicitly reports unknown native authentication and `usable: false`. The [private packaging skeleton](packaging/README.md) provides collision-safe installation and native user-job prototypes; it preserves an unrelated `pio` on PATH. See [verification](docs/VERIFICATION.md) for commands and exact limits. This is not a real harness adapter or release; all six product journeys remain `not_evaluated`.

M1 merged as `9cf7047`. M2 is active: see the [M2 task packet](docs/work/m2/TASK.md), [current state](docs/work/STATE.md) and [issue #5](https://github.com/Combraton/pio/issues/5).
