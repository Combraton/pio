# Decision records

This directory owns PIO-local implementation decisions. The accepted architecture is in [the baseline](https://github.com/Combraton/combraton/blob/main/docs/architecture/BASELINE.md). [ADR 001](https://github.com/Combraton/combraton/blob/main/docs/decisions/001-standalone-first-and-evaluation.md) records the accepted standalone-first build/client/evaluation direction. Concrete SDK/model/renderer selections remain unresolved.

Use one small file per meaningful decision. Include title, status (proposed/accepted/superseded), date, owner/authority, concrete problem, affected contracts, alternatives, selected choice, primary evidence or experiment, consequences, verification and superseded sections.

Wire/compatibility decisions belong in Protocol; cross-system authority changes belong in Combraton. Link the owning decision instead of maintaining independent copies. An experiment result does not silently select a product direction. Keep ordinary local choices lightweight and record material selections in their implementation PR.
