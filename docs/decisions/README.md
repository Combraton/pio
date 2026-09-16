# Decision records

This directory owns PIO-local implementation decisions. The accepted architecture is in [the baseline](https://github.com/Combraton/combraton/blob/main/docs/architecture/BASELINE.md). [ADR 001](https://github.com/Combraton/combraton/blob/main/docs/decisions/001-standalone-first-and-evaluation.md) records the accepted standalone-first build/client/evaluation direction. PIO-local stack selection is recorded below; conditional SDK and later model/product decisions remain explicit.

Use one small file per meaningful decision. Include title, status (proposed/accepted/superseded), date, owner/authority, concrete problem, affected contracts, alternatives, selected choice, primary evidence or experiment, consequences, verification and superseded sections.

- [PIO ADR 001 — standalone implementation stack](001-standalone-stack.md): accepted by the owner 2026-09-16; Python bridge conditional on M3, runtime verification still pending. This PIO-local number is distinct from Combraton's cross-system ADR 001.

- [PIO ADR 002 — one journal for Core and Execution](002-protocol-journal.md): selected before Execution; replaces the Core blob stopgap with journal-backed projections.

Wire/compatibility decisions belong in Protocol; cross-system authority changes belong in Combraton. Link the owning decision instead of maintaining independent copies. An experiment result does not silently select a product direction. Keep ordinary local choices lightweight and record material selections in their implementation PR.
