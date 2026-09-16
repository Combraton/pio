# PIO

Independent execution and recovery for existing agentic harnesses.

> M1 implementation has started with a Cargo skeleton and reproducible build/conformance pipeline. No protocol support, real adapter, released API or performance claim is established here. The reviewed architecture is `architecture-v1-20260912`, published as `public-development-v1-20260913`. Canonical specifications are available through [the documentation map](docs/README.md). This README is an overview, not the full specification.

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

Protocol [v0.1.0](https://github.com/Combraton/protocol/releases/tag/v0.1.0) is released and its exact commit/assets are recorded in [protocol.lock.json](protocol.lock.json). The [standalone readiness plan](docs/work/standalone-0.1/PLAN.md) is accepted as the M0 documentation checkpoint, with Codex-first followed by Claude Code; both remain in the full standalone release target. Stack and platform scope are accepted, with build and conformance CI on macOS arm64 and Linux x86_64 from M1 onward. No PIO runtime or adapter support is established yet.

Test interruption, lost acknowledgment, a surviving worker, late/conflicting completion and cancellation races. Report actual enforced/mediated/cooperative capabilities by tested version. The [journey matrix](docs/JOURNEYS.md) separates real CLI/TUI, recovery and optional CBR evidence.

Develop alongside CBR; neither project waits for the other's complete feature set or the desktop application.

## Stack and status

Rust/Tokio, SQLite/content-addressed payloads and Ratatui are [accepted implementation choices](docs/decisions/001-standalone-stack.md), with a Cargo skeleton now present; behavior remains unvalidated. The Claude Python bridge remains conditional on the M3 experiment. Exact adapter candidates and bounded experiments are recorded there. Puppetmaster supplies evaluated execution ideas, not an adopted controller/workflow engine.

For the shared sequence and self-development boundary, read [BOOTSTRAP](https://github.com/Combraton/combraton/blob/main/BOOTSTRAP.md). No supported runtime release or license is provided; the repository is public and its project license remains to be selected.

## Claude authentication boundary

PIO will invoke the user-selected, already-installed Claude Code executable when its version is qualified. The selected authentication path is an explicit API key or supported provider configuration. **claude.ai login/subscription authentication is unsupported in PIO until an approved route exists**, including when the installed CLI is already logged in. Selecting that executable does not establish support for its cached credentials. M3 must verify the selected authentication route and prevent silent fallback while preserving the user’s normal CLI login and native permissions. No Claude adapter/authentication journey has passed yet. See [the ADR](docs/decisions/001-standalone-stack.md) and [Anthropic’s SDK guidance](https://code.claude.com/docs/en/agent-sdk/overview).

## Working on this repository

Read [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), [the documentation map](docs/README.md), and [verification](docs/VERIFICATION.md). Use existing native harnesses for development. **Combraton self-development is deferred until usable v0.1 releases of all four projects.** Public visibility does not select a license; no project license has been added yet.

## Standalone-first validation

Own the standalone CLI/TUI and its user-attributed optional CBR client. Keep caller context policy separate from execution-core admission; discovery is not proof of capability. Core use must pass with CBR absent. See [release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md), [PIO client semantics](https://github.com/Combraton/pio/blob/main/docs/spec/STANDALONE-CLIENT.md) and [benchmarks](https://github.com/Combraton/benchmarks). PIO and CBR develop in parallel against the agreed Protocol release surface; accepted standalone releases precede thin Combraton implementation.
