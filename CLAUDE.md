# Claude Code in PIO

@AGENTS.md

The imported file is this repository's common working agreement. Follow its scope, invariants, verification and handoff requirements; the specification links are reading pointers, not instructions to preload every document.

Before implementing adapter behavior, inspect the actual harness documentation/source and the PIO recovery contract. A CLI exit code alone is insufficient evidence of cancellation, delivery or safe retry.

For a large ambiguous task, inspect the relevant sources and make a bounded plan before implementation. For ordinary scoped fixes, proceed without approval rituals. Give subagents explicit relevant constraints and require source-linked results; do not assume their context contains every instruction you read.

When reading or editing another repository, explicitly read its root instructions and relevant specification. Directory access does not guarantee instruction loading. Keep one task owner and preserve other worktrees.

After changing instruction files, confirm the next session loaded them using the installed client's context inspection. Do not use `/init` to replace reviewed instructions with generic generated text.
