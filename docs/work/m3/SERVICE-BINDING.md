# The Claude service binding — what is left, and what it costs

Written 2026-09-20 from a full read of the Codex provider integration, so the next session does not have to repeat it. This is the remaining gate item before the Claude live plan can be posted on [issue #7](https://github.com/Combraton/pio/issues/7): the plan describes driving Claude through the service, so it cannot be written before the service can.

## What already exists

- `pio_host::claude::claude_host` — the Claude codec, complete, on the shared lifecycle. It is reachable as `pio claude host STORE COMMAND INVOCATION`, which is the same shape `pio codex host` has, and the controller is the only thing that should ever launch it.
- `pio_claude::service_admission` — every pre-spawn decision, already exercised by the offline matrix.
- `pio claude fake-cli` — the labeled fake, and eleven matrix cases against it.

What is missing is the layer between them: the Protocol provider that turns `execution.submit` into a launched host and turns that host's events back into effects, deliveries and actions.

## The shape of the work

**`run_codex` is about 95 per cent adapter-agnostic.** Reading it against the Claude host's events, only four things are actually Codex:

1. `Controller::submit_codex` hardcodes `["codex", "host"]` in the argv it launches.
2. The journal namespace is `e["codex"]` and `e["codex_events_offset"]`.
3. `events_path` and `append_control` resolve the adapter prefix.
4. `codex_event`, which maps event kinds to effects.

Everything else — the dispatch marker, the launch-guard check, appending controls only after their command committed, the deadline stop, the `known_not_released` and `uncertain_no_respawn` reconciliations, and `drain_output` — is the same for any harness.

The same is true of the other provider methods: `codex_content`, `codex_admission`, `codex_apply`, `codex_effect`, `codex_observe_effect` and `codex_discovery` are generic apart from their namespace and source label.

## The decomposition that follows

Keep a **per-harness event codec** and share the rest, exactly as the host lifecycle was extracted:

1. `Controller::submit_harness(adapter, command, spec)`, with `submit_codex` left as a thin wrapper so the Codex path is untouched.
2. In the provider: `adapter()`, `native()` and `native_source()` replacing `codex()` and `codex_source()` at the seven branch points in `durable.rs`, `execution.rs` and `provider.rs`. With `adapter == "codex"` these are the same predicate, so Codex behaviour cannot change.
3. The journal namespace becomes the adapter string: `e[ns]`, `e[format!("{ns}_events_offset")]`. **Codex keeps `codex`**, because renaming a persisted field would break restart and reattach recovery for executions already in a store.
4. `run_native` replaces `run_codex` and calls `native_event`, which dispatches to `codex_event` or the new `claude_event`.
5. `claude_event` maps the kinds the Claude host emits. They were deliberately named to line up: `spawned`, `config_before`, `config_after`, `turn_start_sent`, `turn_acknowledged`, `action_requested`, `control_applied`, `control_rejected`, `turn_completed`, `host_error`. Three are new and have no Codex counterpart — `session_init`, `request_declined_by_pio` and `tool_uses` — and one Codex kind has no Claude counterpart, `thread_started`.
6. `claude_host_config` in `stream.rs`, which can call `pio_claude::service_admission` rather than repeating its checks, plus `Mode::Claude` and a `serve-claude` arm in the CLI.
7. The Claude matrix moves onto the service, the way the Codex matrix runs, and gains the cases that only exist there: restart and reattach without a duplicate launch, a lost host that is never respawned, and the deadline stop.

## The proof the refactor did nothing else

The same evidence the host-lifecycle extraction used, because the risk is the same: the **Codex matrix at 51 of 51** across its seventeen cases, the **public matrix at 72** (51 pass, 6 expected property failures, 9 expected defense refusals, 6 expected classifier failures), the **diagnostic matrix at 54** (39/3/9/3), caller recovery, and the pinned Protocol conformance counts CI records.

## What it is not

It is not a place to widen anything. The delivery proof stays the replay echo, the decision allowlist stays `allow` and `deny`, cancel stays SIGINT with usage recorded as unknown, and no journey is marked from any of it.
