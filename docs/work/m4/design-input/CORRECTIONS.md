# Corrections to the design input

The design input is the reviewer's document, copied into this worktree unmodified. This file records every change to that copy after the first import, so a reader can tell a correction from a silent edit.

## 2026-09-21 — item 160, orchestrate prerequisite 1

**Corrected by the reviewer**, after reading the pinned Protocol schemas. The reviewer names it as their own error rather than mine.

The original wording asked for a Protocol proposal so that a message could record which run wrote it, as though the whole of "who started whom" were missing. It is not.

- `execution.submit` already carries `payload.origin {initiator, depth, call_budget}`, and `execution.inspect` already returns `origin`. **PIO does not implement it.** That is a PIO task, and no Protocol issue is warranted for it.
- Budget pools and scoped grants exist in the pinned schemas and are implemented in PIO.
- What the schemas do **not** carry is the **author of a steering message**: `steering_entry` and the event record are closed and have no author or grant field. **Only that part needs a Protocol proposal**, filed with demonstrating evidence.

My own [screen-to-interface map](../SCREEN-INTERFACE-MAP.md) was wrong in the matching way and is corrected with it: I reported "origin with initiator, depth and call budget is not implemented" and concluded it was a Protocol gap. The first half was right — PIO does not implement it — and the second half was not. I had read PIO's **event** field `origin`, which takes `command` or `provider`, and never looked for `payload.origin` on submit. Two different things wearing the same name, and I checked the one that was easy to find.
