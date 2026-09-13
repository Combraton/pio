# Current session state — pio

This is a dated navigation snapshot. Reconcile it with Git, linked issues and current task evidence before acting. Issues own live progress; this file does not grant authority or maintain a second backlog.

- **Updated:** 2026-09-13T07:48:44+00:00.
- **Owner/last completed task:** Codex; user-requested prompt lifecycle and session continuity documentation. No runtime implementation task was assigned in this maintenance session. Inspect GitHub before assuming no other tasks exist.
- **Inspected source revision:** `main` at `cf4aba6e2ed39420cca654e08858f2ac9b37e048`, clean before this documentation change. This snapshot is introduced by the subsequent maintenance commit; use Git history for that commit's identity.
- **Completed before this change:** repository architecture/development setup and clone verification. Source inventory contains documentation and documentation-check infrastructure; no product runtime is present at the inspected revision.
- **This change:** explicit prompt lifecycle and human progress-update rules in `AGENTS.md` and `CLAUDE.md`; this restart entrypoint. Shared workflow and handoff-template updates live in Combraton.
- **Current validation:** `python3 scripts/check_docs.py --workspace ..` and `git diff --check` both exited 0 on the maintenance working tree based on the inspected revision. All five sibling documentation checks passed with no unresolved cross-repository file links. These checks validate documentation structure, not product behavior. Product tests and live harness instruction-loading checks have not been run.
- **Decisions/uncertainty:** preserve standalone product ownership and the accepted standalone-first sequence. Implementation stack and detailed release-scope choices are not resolved by instruction files. See the [shared release gates](https://github.com/Combraton/combraton/blob/main/docs/STANDALONE-RELEASES.md).
- **Task resources:** no product process, model worker or test service was started by this maintenance session. Other sessions' resources have not been inventoried; inspect relevant resources on resume.
- **Prompt disposition:** no existing one-off prompt was completed or deleted by this maintenance task. The workspace-local Protocol kickoff remains active because Protocol implementation has not started here; its owner must update or retire it after use. Reusable repository templates remain templates.
- **Next action:** Follow the Protocol release readiness decision, then begin the agreed standalone PIO release with a task and pinned Protocol version. Plan the native CLI/TUI and optional public CBR integration without a Combraton dependency.

At the next meaningful checkpoint replace stale observations with verified current state, preserve consequential history in Git/decision/task records, and link the actual assigned issue, branch, PR and task handoff. Do not accumulate transcripts here. Record exact test commands, exit status, evidence and remaining limitations for the work performed.
