# PIO

Independent execution and recovery for existing agentic harnesses.

> Bootstrap documentation only. No product runtime, released API, installation command or performance claim is established here. The reviewed architecture is `architecture-v1-20260912`, published as `public-development-v1-20260913`. Canonical specifications are available through [the documentation map](docs/README.md). This README is an overview, not the full specification.

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

After a small shared contract is defined, support one real harness with a usable local interface, durable execution identity and inspectable results. Test interruption, lost acknowledgment, a surviving worker, late/conflicting completion and cancellation races. Then validate a second adapter with different lifecycle behavior. Report actual enforced/mediated/cooperative capabilities by tested version.

Develop alongside CBR; neither project waits for the other's complete feature set or the desktop application.

## Stack and status

Rust/Tokio and SQLite/content-addressed payloads are starting preferences, not installed dependencies. Adapter versions, sandbox backend and usage limits need targeted experiments. Puppetmaster supplies evaluated execution ideas, not an adopted controller/workflow engine.

For the shared sequence and self-development boundary, read [BOOTSTRAP](https://github.com/Combraton/combraton/blob/main/BOOTSTRAP.md). No runtime or license is provided by this bootstrap; the repository is public and its project license remains to be selected.

## Working on this repository

Read [AGENTS.md](AGENTS.md), [CLAUDE.md](CLAUDE.md), [the documentation map](docs/README.md), and [verification](docs/VERIFICATION.md). Use existing native harnesses for development. **Combraton self-development is deferred until usable v0.1 releases of all four projects.** Public visibility does not select a license; no project license has been added yet.
