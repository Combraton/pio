# M1 acceptance corrections

Owner review of PR #4 at `baf2faed1ce5cf2fee57bbb36e99973439561310` accepted the public-host checkpoint with five bounded corrections. Base remains `900bc03cfb0c898faa14f5e8b68afe4ac7ff26fe`; one implementation worktree, no sibling writes.

## Outcome and sequence

1. Compare journal sequence numbers for each execution's dispatch-intent observation and host invocation intent. Exercise a crash between those commits, a real reordering mutant, and a wrong-reason classifier control.
2. Expose truthful labeled fake-process discovery. Separate release-marker delivery evidence from recovery classifications, and persist recovery decisions and host changes on restart.
3. Add collision-safe private installation/uninstallation and launchd/systemd user-job prototypes. Exercise actual manager start/stop on each CI platform without changing the unrelated installed `pio`.
4. Reproduce both matrices, caller recovery, Cargo checks and pinned runner from clean source. Preserve per-directory classes, supplemental separation, exact-head CI artifacts, and issue #3 mapping.

## Boundaries and acceptance

No real adapter, native authentication, production packaging, or product-journey claim. Frozen Protocol schemas stay unchanged. Fake-host fault controls remain service launch configuration only. Journal and kernel witnesses are read-only test observations. Ordering controls must fail on the named sequence property, not a crash or unrelated assertion. Packaging must refuse collisions and modified-file removal, preserve state, and record actual service-manager observations.

Dependencies: pinned Protocol release; existing journal, process host, caller ledger and CI. Next: ordering oracle and runtime cut points, then recovery/discovery and packaging, then clean-clone/two-platform receipts. M1 acceptance remains the owner's decision.

## Local implementation checkpoint

At parent head `baf2fae`, the five corrections are implemented. Local checks: 19 Cargo tests, build, fmt and Clippy pass; 22-case public smoke passes all four expected classes. A scoped parked-child regression preserves restart ambiguity until release absence is confirmed. The ordering wrong-reason control uses an actual daemon exit 91 after correctly ordered admission, which the classifier rejects as an ordering kill. Two launchd packaging smoke runs prove install/start/stop/uninstall, unrelated-`pio` preservation, binary modification refusal and manifest-tamper refusal. Docs checks pass.

Read-only Sol review found and prompted fixes for ambiguous-to-pending regression and vacuous oracle acceptance. Packaging review prompted manifest integrity checks, explicit non-transactional uninstall limits, and native user-manager setup/provenance on disposable Linux CI. Temporary stores are retained; test processes and jobs were stopped. Full clean-clone and exact-head CI evidence is next; local smoke is not M1 acceptance.

Follow-up before handoff: the initial implementation `52e1240` completed the clean-clone sequence with runner 206/73/1 plus two separate supplemental passes; native user-job start/stop succeeded on both CI platforms. Read-only review then identified that a recovered negative delivery could emit a reconciliation outcome outside the frozen enum. The follow-up maps release observations to `delivered`, recovered absence to `not_delivered`, and keeps reconciliation outcomes within `delivered | not_delivered | unknown`. It adds a journal-rebuild unit regression and reads/asserts the real public reconciliation event in the parked-child case. Twenty Cargo tests and Clippy pass locally. Exact-head evidence must be refreshed for this follow-up before the acceptance handoff.
