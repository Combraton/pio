#!/usr/bin/env python3
"""L1 — a lead that starts runs, through one `serve-opencode` service.

Owner approvals `owner-2026-09-22-m4b-lead-tool` and "Owner approval,
2026-09-23 — L1, the amended plan", both posted on issue #12: one service; the
lead and both led runs on `minimax-coding-plan/MiniMax-M3`; call budget 2; cap
2,000,000 for the lead sequence with its stop at 1,600,000 read from
**charged**; rehearsal against the labeled fake first; the owner at the desk
for the live run.

**One code path.** `--rehearse` and the live run differ in the harness binary
and its environment and in nothing else a row depends on. The labeled fake
launches the lead tool the way OpenCode 2.0.11 was measured doing
(`lead_tool_probe.py`) and scripts the calls a model would make; the live run
gives the same tool to the owner's own OpenCode. A rehearsal charges a ledger
of its own, so the charging code is the code the live run uses.

**Every row carries its expected value, and a mismatch fails the run.** A row
that cannot be observed in a rehearsal says so and is not counted as proven. A
*record* is an observation with no expected value, and is never counted.

**Every exit writes the receipt and charges the ledger** — an exception,
`SystemExit`, `KeyboardInterrupt` or `SIGTERM` included. Whatever still runs is
cancelled, usage is read from the views before the service is released, a run
whose usage is unknown is charged an allowance, and the error is recorded and
raised again (review 44).

**Every run's spend is bounded while it runs.** OpenCode reports a turn's
usage once, at its end, and it reports the **last model step's** tokens, not
the turn's: three M3b turns that each made a tool call recorded one step of
two (read back from the owner's own store, for sessions PIO started). So the
runner meters each run from what its session sends — each tool call is at
most one more model step, and no step's context can exceed a fixed base plus
every byte the session has sent so far — and cancels a run whose bound passes
its ceiling, or a lead that has made more than `CALL_CEILING` tool calls.
OpenCode does not end a turn on `session/cancel` (M3b, R4), so the host kills
it ten seconds later; until then the lead's own tool holds every call, so the
lead cannot take another step through it.

**Charged from the owner's store where it can be read.** Owner decision,
2026-09-24: the runner may read, read-only, the `session_message` rows of the
sessions PIO started from the owner's OpenCode store, and nothing else in it.
A run is charged the sum of its steps as recorded there, with the bound (the
reported total times the steps the turn could have taken) beside it; where
the store cannot be read, or its last step disagrees with the report, the
bound is charged. That charge is **measured from the store's recorded steps,
which is not the bill**: OpenCode 2.0.11 ships hidden agents (title, summary,
compaction) whose calls a session's steps may not hold, so that the store
holds every billed call is not proven, and the owner reconciles each live run
against the MiniMax console (review 46).

**Reserved before anything can spend.** Immediately before the lead's submit,
the first thing that can spend, each run's worst-case share is written to the
ledger as a `reserved` line, and the exit path replaces each with its charge.
The service starts first: starting it qualifies the harness and spends
nothing, and one that never becomes ready leaves no reservation behind
(review of L3, F4). A runner killed from outside, which no
exit path survives, leaves its reservation standing, and a watchdog in its own
session cancels whatever still runs and stops the service (review 45). Run the
live runner detached, never under a tool's timeout.

**After a SIGKILL the reservations stand, and so does the stop.** Nothing
knows what a killed run spent, so its three `reserved` lines (1,395,200 in
all) stay in the ledger and count as charged. The next attempt is refused
before it starts, because 1,395,200 already charged plus another 1,395,200
at worst passes the 1,600,000 stop. Replacing a reservation with a figure is
**the owner's act, dated and recorded in the ledger**, never the runner's
(review 46).

**`--plan L1b`** is L1's shape, briefed so that what L1 could not show is
shown: `alpha` works for thirty seconds before it counts, so the lead's
`read_run` waits for it (a row reads, from the tool's own log, a read that
took at least 20 s and returned `exited`); and `beta` reads a placeholder
`beta.env` with OpenCode's read tool, which 2.0.11 asks about by default, so
the request comes to the desk. Owner approval, 2026-09-24, with review 48's
amendments. A desk nobody asked, and a history with no session in it, can
decide nothing: those rows are **inconclusive**, never passed (review 47).

What only the live run can show: that a real model uses the tool at all,
that what the runs report is what the files say, and what the owner's OpenCode
does with a permission prompt.

`--mutant` (rehearsal only) changes one thing and must fail a named row:

- `no-tool` submits the lead without the tool, so the host sends
  `mcpServers: []` and no child is ever started;
- `reports-refused-as-started` gives the tool the bug the first rehearsal
  caught: a refused admission reported as a start;
- `grant-may-answer` puts `execution.respond_action` in the lead's grant;
- `grant-no-steer` takes `execution.steer` out of it;
- `wrong-child` has each led run report one line too many;
- `wrong-relay` has the lead relay one line too many;
- `lead-without-brief` sends the lead's brief without its bytes, which is how
  the first rehearsal's lead came to be refused;
- `third-admitted` gives the lead a budget of three, so the third start the
  runner probes is admitted: it must be charged its share on a ledger line
  of its own, since nothing reserved it;
- `lead-submit-invalid` sends the lead's brief with bytes that do not match
  its digest, so the service refuses the envelope and no execution exists:
  the lead is charged nothing, not its share;
- `desk-silent` never answers the desk, so the host's single-use reject lands
  after the delivery timeout; the run must still finish and leave a receipt;
- `lead-loops` has the lead keep calling `read_run` after it has its answers,
  so the runner must cancel it at the call ceiling;
- `interrupted` raises `KeyboardInterrupt` mid-run; the receipt and the
  charge must be written anyway;
- `runner-killed` has the runner SIGKILLed once the desk has answered: there is
  no receipt, the ledger must still hold every run's reservation, and the
  watchdog must have stopped the service;
- `release-refused` has the cleanup's guard refuse the tree at the end, as it
  refused L1's live tree; the run must fail on it, and the tree is then
  released for real;
- `setup-fails` makes the scenario fail after the tree exists and before
  anything is reserved: the tree must be gone, and there is no receipt;
- `helper-elsewhere` puts `small_model` on another provider in the
  rehearsal's own OpenCode configuration: the runner must refuse to start,
  before anything is reserved, and leave no tree and no receipt (owner
  decision, 2026-09-25);
- `no-wait` (L1b) has `alpha` finish in a second, so no read waits;
- `no-ask` (L1b) has nobody ask, so the desk row must be inconclusive, not
  passed;
- `alpha-outlasts` (L1b) has `alpha` work 62 s, past the tool's 55 s wait:
  the waiting row must still **hold**, and only through a read that came back
  still running at the limit;
- `wrong-model`, `reviewer-elsewhere`, `no-pre-allow` and `child-overspends`
  (L3) have Codex answer another model, route approvals elsewhere, drop the
  pre-allowance, or take one step past a child's ceiling;
- `lead-asked-in-openai-form` (L3) has Codex ask about the lead's tool in a
  mode PIO does not recognise, so PIO declines it by itself: the
  pre-allowance row must fail and say so;
- `lead-asked-by-user-input` (L3) asks it by `item/tool/requestUserInput`,
  Codex's route when its elicitation route is off: the row must fail too;
- `child-asks-permissions` (L3) has `beta` ask a permission grant, which PIO
  declines by itself: the desk row must fail, because PIO decided it;
- `service-never-ready` has the service refuse its configuration at start: the
  runner must say so at once, and leave no reservation and nothing charged,
  since nothing was submitted;
- `first-number-of-all` (L3) has the runner read every message a child said,
  as it did, instead of its answer: the fake's children say what they are
  about to run first, as Codex was measured doing, so the count row fails;
- `ceiling-cancel-never-sent` and `stop-charged-reported` (L3) play
  `child-overspends` with the runner changed: its meter decides the stop and
  never sends it, so the row that reads the host's own interrupt fails; or it
  charges a stopped run only what it reported, so the charge row fails.
  `child-overspends` itself must hold both of those rows;
- `usage-suppressed` (L3) has Codex report no usage at all, and `alpha` work
  past the silence bound: the meter must stop the lead and `alpha` for their
  silence, and every run is charged its whole share;
- `asked-silent` (L3) is `usage-suppressed` with both children asking a
  command approval first, which the desk answers: `alpha` must still be
  stopped for its silence;
- `lead-heavy` (L3) has each of the lead's steps cost nearly a whole step in
  flight (29,000, so its total passes the hold before it reaches the
  ceiling) and the lead read on: its tool must withhold a result,
  the runner stop the lead, and the lead end within its share;
- `lead-tool-error-past-hold` and `lead-shell-past-hold` (L3) have the lead,
  past its hold, get a lead-tool error (an argument `read_run` does not take)
  or a result from Codex's own shell: the error must be held like any result,
  the shell's result met by a stop; `tool-error-ungated` (errors skip the
  gate) and `meter-ignores-items` (the meter blind to such results) must each
  fail the row that checks every result against the hold;
- `subagent-spawned` (L3) has `alpha` spawn a sub-agent as Codex 0.157.0
  does: the runner must stop `alpha` when the agent appears and charge it a
  step in flight on each of its threads, and "No run spawned a sub-agent"
  fails; `subagent-not-stopped` skips the stop and
  `subagent-uncounted` charges `alpha`'s own thread alone, and each fails
  the row that judges it;
- `overrides-on` (L3) turns Codex's unmetered features off per launch under
  the fake's own token, over a Codex home whose own configuration turns every
  one of them on (aliases too): the preflight must find each off by the
  override, every thread must carry the whole set, the child that would spawn
  a sub-agent must not, no memory pipeline may write, and every row must hold;
- `subagents-unguarded`, `subagents-multi-agent-only`, `memory-unguarded`,
  `memory-alias`, `goals-unguarded`, `web-search-unguarded` and
  `image-generation-unguarded` (L3) apply the live check to the rehearsal's
  own Codex home, with every other unmetered feature off by its own keys:
  sub-agents by Codex's default and with `multi_agent = false` alone,
  memories on and on by its alias `memory_tool` after `memories = false`,
  and goals, web search and image generation by Codex's defaults; each must
  refuse, before anything is reserved, for its own feature and no other;
  `override-misses-alias` sends the fake's token with a modelled override
  that lacks `memory_tool`, beside the owner's `memory_tool = true`, which
  still decides: it must refuse too (review of L3, round 4, U1);
  `decision-absent` sends no decision over the owner's own keys as read on
  2026-09-26 (`memories = true` and nothing else): all five must be named
  and refused, as a real Codex without L3's decision would be (owner
  decision, 2026-09-26, "Per-launch override");
- `lead-exit-uncarried` (L3) drops the lead's list of what PIO declined, as
  if its exit carried none and it was not seen to exit: the pre-allowance
  row must be inconclusive, not held; `owner-service-absent` empties both
  listings of the owner's OpenCode service: that row must be inconclusive;
- `meter-dies` (L3) fails the meter thread as the third start begins: the
  meter must cancel every run still going itself, within seconds, before
  alpha reaches its ceiling, and the run ends with the error;
- `probe-over-share` (L3) is `third-admitted` with the admitted third run
  taking one 150,000-token step: it is metered and stopped like any run,
  charged through the same branches (at least its share, and at least what
  it was seen to spend plus a step in flight), and the share row, which now
  covers probes, fails; `probe-charged-flat` charges it its flat share, as
  before, and the floor row fails;
- `late-step-after-halt` (L3) interrupts the runner while `alpha`'s
  40,000-token step is in flight, so the host reports it only after the meter
  has stopped: the charge, read from the host's files, must cover it and the
  floor row hold; `stale-meter-charged` charges from the meter's fold alone,
  and the floor row fails;
- `goal-continued` (L3) has Codex start a turn on `beta`'s own thread by
  itself once `beta`'s has ended, as a goal's continuation does: the host,
  reading the thread for its grace period, interrupts it, `beta` is charged
  as cut short, and the row that looks for such a turn fails;
  `continuation-uncharged` charges `beta` as ended by itself, and the floor
  row fails;
- `memory-pipeline-ran` (L3) has the fake write, at its first turn, what
  Codex's memory pipeline writes under the Codex home: the memory row must
  fail, listing each path by a Codex-chosen name or a digest, and no name a
  session gave may reach the receipt;
- `step-past-in-flight` (L3) has `alpha` take a 40,000-token step: the
  runner must stop it, and the in-flight row fails;
- `stopped-past-share` (L3) has `alpha`, stopped at its ceiling, ignore the
  stop and answer: it is charged its 90,000 and a step, past its share, and
  the share row fails; `stopped-charge-capped` caps that charge at the share,
  and the floor row fails; `deadline-interrupted` has the host's own
  deadline interrupt `alpha`, which is charged as a stopped run, and
  `interrupted-charged-reported` charges it its report alone, and the floor
  row fails;
- `child-renamed` (L3) has the lead start and read its children as
  `alpha_run` and `beta_run`: its tool refuses both, since only the plan's
  children may be started, so the start row fails and nothing runs;
  `child-renamed-unchecked` lets them through the tool, and the runner must
  still meter the renamed `alpha`, stop it at its ceiling, cancel it and
  charge both on lines of their own, so the ceilings row fails;
- `lead-tool-deferred` (L3) sends the lead's tool without `omit_tools_from`,
  as L3's first live run did: on `gpt-5.6-terra`, which runs code-mode-only,
  Codex defers its tools behind `exec` and never names them, so the fake's
  lead, like the live one, says it cannot access the tool and starts
  nothing, and the start row fails;
- `experimental-feature-on` (L3) turns on `features.exec_permission_approvals`
  in the rehearsal's own Codex configuration: the runner must refuse to
  start, before anything is reserved, and leave no tree and no receipt;
  `experimental-alias-on` turns it on by Codex's legacy alias
  `request_permissions`, beside the canonical key set false, which the
  alias overrides: the runner must refuse the same way;
- `stop-ignored` (L3) has `alpha`, past its share, acknowledge the interrupt
  and go on, and the runner interrupted: `alpha` is still running when its
  usage is read, is charged what it reported plus a step, and the share row
  fails; `not-exited-charged-share` charges it only its share, and the floor
  row fails;
- `qualified-elsewhere` (L3) puts in the store the qualification serve-codex
  would write for a Codex whose native binary changed: the row that compares
  it with the committed record must fail; `qualified-as-committed` plants the
  committed record itself, and that row must **hold**.
"""
import argparse
import base64
import contextlib
import hashlib
import json
import os
import re
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
import codex_live_run
import opencode_host_matrix as matrix
import opencode_live_run
import opencode_live_run as live
from approval_desk import ASKS
from board_fold import Caller
from check_private_paths import redact
from lead_tool import HOLD, METER_WAIT, READ_WAIT, final_answer, message_text
from public_api import command

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
TOOL = ROOT / 'scripts' / 'lead_tool.py'
OPENCODE_SEQUENCE = 'M4-lead-opencode'
OPENCODE_MODEL = 'minimax-coding-plan/MiniMax-M3'
PROVIDER = 'conformance-provider'
BUDGET = 2
OPENCODE_SEQUENCE_CAP = 2_000_000
OPENCODE_SEQUENCE_STOP = 1_600_000
DELIVERY_TIMEOUT = 300
EXECUTION_DEADLINE = 900

# --- The plans. L1 is the first live lead run. L1b is the same shape, briefed
# so that the lead's read waits and a request comes to the desk: owner
# approval of 2026-09-24, with review 48's amendments. OpenCode 2.0.11's
# shipped default asks before reading `*.env` (review 48, static; the owner's
# own configuration overrides no permission, checked 2026-09-24), and PIO
# relays a request inside the workspace to the desk.
PLANS = {
    'L1': dict(
        approval='Owner approval, 2026-09-23 — L1, the amended plan (issue #12)',
        files={'alpha.md': 7, 'beta.md': 4},
        briefs={'alpha.md': 'Report the number of lines in alpha.md. '
                            'Answer with the number alone.',
                'beta.md': 'Report the number of lines in beta.md. '
                           'Answer with the number alone.'},
        waits=None, asks=None),
    'L1b': dict(
        approval='Owner approval, 2026-09-24 — L1b, with the amendments of '
                 'review 48 (issue #12)',
        files={'alpha.md': 7, 'beta.env': 4},
        briefs={'alpha.md': 'First run the shell command sleep 30. Then report the '
                            'number of lines in alpha.md. Answer with the number '
                            'alone.',
                'beta.env': 'Use your file read tool, not a shell command, to read '
                            'beta.env. Then report the number of lines in beta.env. '
                            'Answer with the number alone.'},
        # alpha works for thirty seconds, so a read of it waits; beta reads a
        # placeholder `.env` file, so OpenCode asks and the desk relays.
        waits='alpha', asks='beta'),
    # Owner approval, 2026-09-24 (L3 on Codex), the decisions of 2026-09-25,
    # and the sizing of 2026-09-26 (CODEX, below): `gpt-5.6-terra`; each child
    # runs one command, `sleep N && wc -l`; the lead's own two tools are
    # pre-allowed per launch; Codex re-pinned to 0.157.0. `alpha` sleeps long
    # enough to be steered while it runs, which is L3's claim: a steer under
    # the lead's grant, delivered to a running Codex turn.
    'L3': dict(
        harness='codex',
        approval='Owner approval, 2026-09-24 — L3; owner decisions of 2026-09-25 (issue '
                 '#12); owner decision of 2026-09-26, "More headroom": lead 150,000, child '
                 '60,000, in flight 30,000, sequence cap 565,000 and stop 452,000, Codex cap '
                 '1,090,000 and stop 872,000',
        # Owner decision, 2026-09-26 ("Per-launch override"): L3's Codex
        # threads are launched with Codex's five unmetered features off, per
        # launch, under this dated decision (pio-protocol
        # FEATURES_OFF_DECISIONS); the owner's config.toml is not changed.
        # Sent for this plan's threads only.
        features_off_decision='owner-2026-09-26-l3-codex-unmetered-features-off',
        files={'alpha.md': 7, 'beta.md': 4},
        briefs={'alpha.md': 'Run the shell command `sleep 30 && wc -l alpha.md`. Then '
                            'report the number of lines in alpha.md. Answer with the '
                            'number alone.',
                'beta.md': 'Run the shell command `sleep 5 && wc -l beta.md`. Then '
                           'report the number of lines in beta.md. Answer with the '
                           'number alone.'},
        waits='alpha', asks=None),
}
# Every plan names its harness; L1 and L1b are OpenCode's.
for _plan in PLANS.values():
    _plan.setdefault('harness', 'opencode')

# --- What differs between the two harnesses a lead runs on, in one place.
#
# Codex (L3): the owner's sizing. Owner decision, 2026-09-26 ("More
# headroom", answering Q5 on the 405,000 worst case): lead 150,000, child
# 60,000, a step in flight 30,000, so the worst case is 450,000; the L3
# sequence (M4-lead-codex) cap 565,000 with its stop at 452,000; the Codex
# cap raised from 1,000,000 to 1,090,000 and its stop from 800,000 to
# 872,000 (codex_live_run.CAP, STOP_AT). It replaces 2026-09-24's lead
# 125,000, child 50,000, sequence cap 400,000 and stop 320,000. Codex's
# reported total covers every step of a turn (review 48, from M2's host
# events), so it is what a run is charged, and it arrives after every step,
# so the runner can stop a run by it. A step on this harness and model was
# measured at about 22,000 to 24,500 tokens (M2 R4 to R6); a run stopped at
# its ceiling can finish at most the one step in flight, at most 30,000.
# The lead makes five steps, one at a time; each child runs one command,
# in two steps.
CODEX = dict(
    model='gpt-5.6-terra', model_provider='openai', sequence='M4-lead-codex',
    sequence_cap=565_000, sequence_stop=452_000, lead_ceiling=150_000,
    child_ceiling=60_000, in_flight=30_000, ledger='Codex ledger',
    live=codex_live_run,
    # Where the rehearsal's Codex comes from, in the receipt (owner decision
    # for L3, 2026-09-25: the record says so).
    record=dict(
        shapes="0.157.0's generated app-server schema: ThreadItem mcpToolCall and "
               'commandExecution, McpServerElicitationRequestParams; the labeled fake '
               'follows it',
        mcp_approvals='when Codex asks before an MCP tool call, what it offers and how it '
                      'reads the answer: read from its source at rust-v0.157.0 '
                      '(codex-rs/core/src/mcp_tool_call.rs), not measured. The file differs '
                      'from rust-v0.155.1; the functions that decide, build the request and '
                      'read the answer do not (requires_mcp_tool_approval_for_mode, '
                      'maybe_request_mcp_tool_approval, custom_mcp_tool_approval_mode, '
                      'build_mcp_tool_approval_elicitation_request, _meta, _question and '
                      '_fallback_message, parse_mcp_tool_approval_elicitation_response, '
                      'normalize_approval_decision_for_mode). request_mcp_tool_user_approval, '
                      'which sends the request, changed (analytics on a refused answer), and so '
                      'did the persistence helpers PIO never triggers'))
WAITED = 20
# The longest the lead's tool can take to answer a call once its result is
# ready: the wait for a fresh meter pass and a hold.
METER_WAIT_AND_HOLD = METER_WAIT + HOLD
# The committed qualification records, one per pin (M2 and its re-pins).
QUALIFICATIONS = ROOT / 'docs/work/m2/codex-qualification'
# Never run unmetered (review of L3, F9): a Codex run that has been active
# this long, all told, since its reported total last rose is stopped and
# charged its whole share. Time waiting at the desk does not count. Codex
# reports a step once its tool has finished (M2 R5, R6), so a lead's longest
# honest silence is a model step plus a 55 s read_run, and a child's a model
# step plus its command (30 s for alpha).
USAGE_SILENCE = 150

# --- The bound on what one attempt can spend.
#
# A lead needs two starts and a read per child. `read_run` waits up to 55
# seconds for its run, so a child held at the desk for the whole 300 seconds
# it may wait for an answer costs six reads, and sixteen calls leave room for
# more than twice that.
CALL_CEILING = 16
# Every M3b turn on this harness and model began at 7,960 to 8,076 input
# tokens before the brief said anything; the lead adds its tool's schema and
# a longer brief. Rounded up.
STEP_BASE = 10_000
OPENCODE_LEAD_CEILING = 450_000
OPENCODE_CHILD_CEILING = 90_000
# The lead's last step: the one in flight when it is stopped. Its next call
# through the tool is held until the kill, so there is no step after it. It
# holds at most its context at the stop plus one tool result: OpenCode's own
# truncation (50 KiB, not measured by PIO) for a built-in tool, 2,000
# characters for `read_run`.
LAST_STEP = 51_200
# A child has no tool of PIO's to hold. It runs until the host's kill, ten
# seconds after the cancel, and the fastest step M3b measured on this model
# took 0.96 seconds: at most eleven more steps. A child step in the M3b
# fixture was 8,135 to 8,392 tokens; this is rounded up, and it is **not**
# enforced — PIO cannot bound the size of a child's step.
KILL_STEPS = 11
CHILD_STEP = 12_000
OPENCODE_LEAD_SHARE = 2 * OPENCODE_LEAD_CEILING + LAST_STEP
OPENCODE_CHILD_SHARE = OPENCODE_CHILD_CEILING + KILL_STEPS * CHILD_STEP
OPENCODE_WORST_CASE = OPENCODE_LEAD_SHARE + 2 * OPENCODE_CHILD_SHARE
OPENCODE_BOUND = dict(
    call_ceiling=CALL_CEILING, step_base=STEP_BASE, lead_ceiling=OPENCODE_LEAD_CEILING,
    child_ceiling=OPENCODE_CHILD_CEILING, last_step=LAST_STEP, kill_steps=KILL_STEPS,
    child_step=CHILD_STEP, worst_case=OPENCODE_WORST_CASE,
    estimate='(tool calls + 1) x (step base + bytes the session has sent)',
    lead='at most its ceiling, plus the step in flight at the stop '
         '(at most its ceiling again, plus one tool result)',
    child='at most its ceiling, plus eleven steps before the kill',
    assumes=['a token is at least one byte',
             'everything added to a context after the first step is sent as a '
             'session update, which PIO spools',
             'the runner reads every meter at least once per model step '
             '(every half second; the fastest measured step took 0.96 s)',
             'no built-in tool returns more than OpenCode truncates to',
             "a child's step stays near the size measured in M3b"])

FEATURES = ('core.events', 'core.capabilities', 'core.effects', 'core.grants')
EXECUTION_FEATURES = (*matrix.FEATURES, 'execution.steering')
CONTENT = 'pio.combraton.dev/content'
LEAD_TOOL = 'pio.combraton.dev/lead-tool'
# The lead's own two tools, pre-allowed on Codex (owner decision, 2026-09-25).
PRE_ALLOWED = ('start_run', 'read_run')
# The lead tool's MCP server name, as the lead's session knows it.
LEAD_SERVER = 'pio-lead'
# PIO's own settings in the lead tool's server table on Codex (L3's first
# live run, 2026-09-26: the lead never saw its tool). `gpt-5.6-terra` runs
# code-mode-only (`models-manager/models.json:676`, rust-v0.157.0): the model
# sees `exec` and `wait`, and an ordinary MCP server's tools are deferred
# behind `exec` and never named (`core/src/mcp_tool_exposure.rs:90-94`,
# `core/src/tools/spec_plan.rs:234-266`). With `code_mode` and `deferred`
# omitted they are `DirectModelOnly`, in the model's own tool list
# (`tools/src/tool_executor.rs:68-72`; `spec_plan.rs:528-566`, `:761-772`).
# `required` has Codex refuse `thread/start` if the server fails to start
# (`codex-mcp/src/connection_manager/required.rs:15-58`, from
# `core/src/session/mcp_runtime.rs:148`); `startup_timeout_sec` bounds its
# start (`config/src/mcp_types.rs:250-256`, `:440-447`). The field names and
# types are `config/src/mcp_types.rs:229-256` and `:372-385`; the surfaces'
# names `protocol/src/config_types.rs:396-407`. The owner's lead-tool
# approval (`owner-2026-09-22-m4b-lead-tool`) covers PIO's own server.
LEAD_TOOL_SETTINGS = dict(omit_tools_from=['code_mode', 'deferred'], required=True,
                          startup_timeout_sec=30)
UNDER_GRANT = 'pio.combraton.dev/under-grant'
APPROVAL = 'pio.combraton.dev/approval'
DECISION = 'pio.combraton.dev/decision'
# What a run's host declined by itself, never put to a caller, on the run's
# `execution.exit.observed` (review of L3, CH-2/F1).
NATIVE = 'pio.combraton.dev/native-declines'
ELICITATION = 'mcpServer/elicitation/request'


# The runner's own probes' briefs.
THIRD_BRIEF = 'a third run the budget does not allow'
ATTACHED_BRIEF = 'give my child a tool'
CREDENTIAL_BRIEF = 'a spec with a credential value in it'


def child_brief(name):
    return PLAN['briefs'][name]


# The lead's step in the plays that take it past its hold: a thousand
# tokens under a whole step in flight. With whole steps, and the hold one
# step under the ceiling (120,000 and 150,000), the report that passes the
# hold is the one that reaches the ceiling, and the ceiling's stop would
# pre-empt the hold these plays exist to exercise.
LEAD_PAST_HOLD_STEP = 29_000


def lead_brief():
    a, b = FILES
    return (f'There are two files in this workspace, {a} and {b}. Use the '
            'pio-lead tool, and nothing else, to do this. Call start_run twice: '
            "name 'alpha' with the brief '" + child_brief(a) + "', and name "
            "'beta' with the brief '" + child_brief(b) + "'. Then call "
            'read_run for alpha and for beta, again until its runtime is exited. Do '
            'not read the files yourself and do not start any other run. Finish '
            f'with exactly two lines, {a}: <number> and {b}: <number>, using '
            'the numbers the two runs reported.')


def select_plan(name):
    """Bind the names every step reads to one plan's lead, files, briefs and
    harness."""
    global PLAN, LEAD, FILES, CHILDREN, RUNS, APPROVALS, BRIEF, HARNESS, live
    global MODEL, SEQUENCE, SEQUENCE_CAP, SEQUENCE_STOP, LEAD_CEILING, CHILD_CEILING
    global LEAD_SHARE, CHILD_SHARE, WORST_CASE, BOUND, HOLD_ABOVE, PROBES, PROBE_SHARES
    global PROBE_BRIEFS
    PLAN = dict(PLANS[name], name=name)
    LEAD = name
    FILES = PLAN['files']
    CHILDREN = {f.split('.')[0]: f for f in FILES}
    RUNS = (LEAD, *[f'{LEAD}.{c}' for c in CHILDREN])
    # The runner's own starts under this lead, each expected to be refused,
    # and charged apart if one is not (review of L3, round 2, SB-5).
    PROBES = (f'{LEAD}.third', f'{LEAD}.attached')
    HARNESS = PLAN['harness']
    if HARNESS == 'codex':
        live = CODEX['live']
        MODEL, SEQUENCE = CODEX['model'], CODEX['sequence']
        SEQUENCE_CAP, SEQUENCE_STOP = CODEX['sequence_cap'], CODEX['sequence_stop']
        LEAD_CEILING, CHILD_CEILING = CODEX['lead_ceiling'], CODEX['child_ceiling']
        step = CODEX['in_flight']
        # Codex reports a step once its tool has finished (M2 R5, R6), so a
        # report that crosses a ceiling arrives with the next step already
        # begun: the crossing step and the one in flight when the stop lands
        # (review of L3, A7). A child has no tool of PIO's to hold, so that
        # is its ceiling plus two steps. So is the lead (review of L3, round
        # 2, SB-1). Its tool withholds any response once the lead has
        # reported more than its ceiling less one step, which keeps a lead
        # that uses only its tool within its ceiling plus one step; and the
        # meter stops it when a result its tool never saw comes back past
        # that. But such a result (Codex's own shell, another MCP server, a
        # refused approval) is back, and the next step begun, before
        # anything can see it, when the lead may be just under its ceiling:
        # the ceiling plus two steps is the bound.
        HOLD_ABOVE = LEAD_CEILING - step
        LEAD_SHARE = LEAD_CEILING + 2 * step
        CHILD_SHARE = CHILD_CEILING + 2 * step
        WORST_CASE = LEAD_SHARE + 2 * CHILD_SHARE
        # With sub-agents on (Codex 0.157.0's default), each run could also
        # start sub-agents in one step before the stop lands, each with a
        # step in flight: at most three resident per run for multi-agent V2
        # (max_concurrent_threads_per_session 4, less the root; the version
        # gpt-5.6-terra's bundled catalog entry names), six for V1
        # (DEFAULT_AGENT_MAX_THREADS). Read from source at rust-v0.157.0, not
        # measured (review of L3, round 3, SPEND-2).
        SUBAGENTS_ON = {version: WORST_CASE + len(RUNS) * resident * step
                        for version, resident in (('v2', 3), ('v1', 6))}
        BOUND = dict(
            call_ceiling=CALL_CEILING, lead_ceiling=LEAD_CEILING,
            child_ceiling=CHILD_CEILING, in_flight=step, hold_above=HOLD_ABOVE,
            lead_share=LEAD_SHARE, child_share=CHILD_SHARE, worst_case=WORST_CASE,
            estimate="Codex's own running total for the thread, reported after every step",
            lead='at most its ceiling, plus the step that crossed it (reported only after '
                 'its tool) and the step in flight when the stop lands. Its tool withholds '
                 f'any response once it has reported more than {HOLD_ABOVE}, and the meter '
                 f'stops it when a result its tool never saw comes back past {HOLD_ABOVE}: '
                 'that keeps a lead using only its tool within its ceiling plus one step, but '
                 'a result its tool never sees starts a step before anything can stop it',
            child='at most its ceiling, plus the step that crossed it (reported only after '
                  'its command) and the step in flight when the stop lands',
            usage_silence=USAGE_SILENCE,
            unmetered=dict(
                features=list(UNMETERED),
                off=f'{WORST_CASE}: with every one of them off on every thread, by the '
                    "owner's own config.toml or per launch under a recorded owner decision "
                    '(pio_codex::features_off), the bound above stands; the live runner '
                    'refuses to start otherwise, naming each feature not off and the key '
                    'that decided it (review of L3, round 4, U1)',
                subagents_on=SUBAGENTS_ON,
                subagents_on_how=f'{WORST_CASE} plus, for each of the {len(RUNS)} runs, a '
                                 'step in flight on every sub-agent it could start in one step '
                                 'before the stop lands: three for multi-agent V2, six for V1 '
                                 '(rust-v0.157.0, not measured)',
                memories_on='on the order of 900,000 input tokens an attempt: Phase 1 sends '
                            "up to two of the owner's idle sessions per app-server to the "
                            'extraction model, three app-servers an attempt, and Phase 2 runs '
                            'a consolidation agent (rust-v0.157.0, not measured)'),
            assumes=["Codex's reported total covers every step of a turn (review 48), and a "
                     "run's usage is the sum over its threads (round 3)",
                     'a step on this harness and model is at most 30,000 tokens '
                     '(measured about 22,000 to 24,500, M2 R4 to R6); the runner checks every '
                     'report and stops a run whose step is larger, and charges its largest '
                     'step in flight (review of L3, round 3, SPEND-4)',
                     "every unmetered, default-on feature of Codex 0.157.0 is off on every "
                     "L3 thread (sub-agents, memories, goals, standalone web search and image "
                     "generation), by the owner's own config.toml or per launch under a "
                     'recorded owner decision; the runner refuses to start otherwise (round 4, '
                     'U1). Behind that, a run whose sub-agent appears anyway is stopped at once '
                     'and charged a step on each of its threads (round 3, SPEND-2), a turn '
                     'Codex starts by itself on a run\'s thread after the run\'s own has ended '
                     'is interrupted by the host, read for three seconds after that turn, and '
                     'its run charged as cut short (round 4, SPEND-9), and a memory pipeline '
                     'that writes anyway fails the memory row (round 3, S6). A '
                     'setting from a managed configuration (requirements.toml, MDM, cloud) '
                     'is not read, and could override either route',
                     "Codex's remote compaction makes a model call whose usage goes to "
                     "analytics and the rollout budget, not the thread's total "
                     '(core/src/compact_remote_v2.rs:304-310, 374; rust-v0.157.0). It runs '
                     "when a step's context passes the model's auto-compact limit, 90% of "
                     "gpt-5.6-terra's 272,000-token window, 244,800 "
                     '(models-manager/models.json; protocol/src/openai_models.rs:525-536): a '
                     'step that large is already past the one the bound assumes, and its run '
                     'is stopped and charged its largest step in flight. The owner\'s '
                     'model_auto_compact_token_limit, which could lower the limit, is not '
                     'read; with it lowered, compaction would spend outside this bound (not '
                     'measured)',
                     "at each thread's start Codex sends one request with generate: false, "
                     'its instructions and tools, over the WebSocket transport, to warm it '
                     '(core/src/session_startup_prewarm.rs:306-320; core/src/client.rs:1966, '
                     '2071-2120), and records no usage for it: whether it is billed, and for '
                     'how much, is not established; one per thread, three an attempt, outside '
                     'this bound',
                     'what Codex asks the model only when a client asks it to, which PIO never '
                     'does, cannot occur: a review (review/start), a thread title (an '
                     'ephemeral thread whose source is thread_title, '
                     'core/src/session/session.rs:1643-1646), a realtime conversation, a '
                     "compaction on request; nor the guardian reviewer, which runs only when "
                     'approvalsReviewer is not user, which every run asserts from '
                     "thread/start's answer before its first turn",
                     "a tool of the owner's own MCP servers, apps or plugins, which Codex "
                     'loads on every thread, runs outside Codex, and what it spends is not in '
                     "Codex's report; a call to one asks first unless the owner's settings "
                     'pre-approve it (not read), and an ask fails the desk row',
                     'Codex retries a dropped stream up to 5 times on each transport, so up '
                     'to 10 on the step that falls back from WebSocket to HTTPS, and a failed '
                     'request up to 4 times, by default (model-provider-info/src/lib.rs:64-65, '
                     '487-498; core/src/responses_retry.rs:96-111, rust-v0.157.0); the '
                     "owner's model_providers settings can raise either, up to 100, and are "
                     'not read. The first WebSocket retry of a step is not surfaced in a '
                     'release build (responses_retry.rs:119-121). Codex records usage only on '
                     'a completed response, so what a dropped attempt is billed, if anything, '
                     'is never reported: the receipt counts each retry Codex surfaced '
                     '(willRetry) per run and charges none, and this bound leaves them out (not '
                     'measured; round 4, SPEND-10)',
                     'Codex reports a step once its tool has finished, before the next '
                     "step calls a tool (M2 R5, R6), and the report reaches the runner's "
                     'meter within 1.5 s',
                     'a model step takes longer than a stop takes to reach Codex (about '
                     '1.5 s against the labeled fake)',
                     'Codex honours turn/interrupt (M2), before the tool\'s 30 s hold ends'])
    else:
        live = opencode_live_run
        MODEL, SEQUENCE = OPENCODE_MODEL, OPENCODE_SEQUENCE
        SEQUENCE_CAP, SEQUENCE_STOP = OPENCODE_SEQUENCE_CAP, OPENCODE_SEQUENCE_STOP
        LEAD_CEILING, CHILD_CEILING = OPENCODE_LEAD_CEILING, OPENCODE_CHILD_CEILING
        LEAD_SHARE, CHILD_SHARE, WORST_CASE = (OPENCODE_LEAD_SHARE, OPENCODE_CHILD_SHARE,
                                               OPENCODE_WORST_CASE)
        HOLD_ABOVE = None
        BOUND = OPENCODE_BOUND
    # Each probe's share, should the service admit it, and its brief, by
    # which the journal names its invocation.
    PROBE_SHARES = {f'{LEAD}.third': CHILD_SHARE, f'{LEAD}.attached': LEAD_SHARE,
                    'credential-check': LEAD_SHARE}
    PROBE_BRIEFS = {f'{LEAD}.third': THIRD_BRIEF, f'{LEAD}.attached': ATTACHED_BRIEF,
                    'credential-check': CREDENTIAL_BRIEF}
    APPROVALS = dict(model_exception=live.MODEL_EXCEPTION,
                     lead_tool='owner-2026-09-22-m4b-lead-tool',
                     **{name.lower(): PLAN['approval']})
    BRIEF = lead_brief()


select_plan('L1')

# Not a row: the runner refuses to start, before anything is reserved, when
# one of Codex's unmetered features is not off (review of L3, round 4, U1).
UNMETERED_REFUSAL = "The runner refuses a Codex whose unmetered features are not all off"


def all_off_but(*left, features=None):
    """A Codex `config.toml` that turns each unmetered feature off by the
    owner's own keys, except those named `left`, and then sets `features`."""
    top = [] if 'standalone web search' in left else ['web_search = "disabled"']
    table = {} if 'memories' in left else {'memories': False}
    table.update({} if 'goals' in left else {'goals': False})
    table.update({} if 'image generation' in left else {'image_generation': False})
    table.update(features or {})
    agents = [] if 'sub-agents' in left else ['[agents]', 'enabled = false']
    lines = [*top, '[features]', *(f'{k} = {json.dumps(v)}' for k, v in table.items()), *agents]
    return '\n'.join(lines) + '\n'


# What each check mutant plants in the rehearsal's own Codex home.
UNMETERED_PLANTED = {
    'subagents-unguarded': all_off_but('sub-agents'),
    'subagents-multi-agent-only': all_off_but('sub-agents', features={'multi_agent': False}),
    'memory-unguarded': all_off_but('memories', features={'memories': True}),
    'memory-alias': all_off_but('memories', features={'memories': False, 'memory_tool': True}),
    'goals-unguarded': all_off_but('goals'),
    'web-search-unguarded': all_off_but('standalone web search'),
    'image-generation-unguarded': all_off_but('image generation'),
    'override-misses-alias': '[features]\nmemory_tool = true\n',
    # The owner's own configuration as read by those keys alone on
    # 2026-09-26 (memories = true, nothing else), and no decision sent.
    'decision-absent': '[features]\nmemories = true\n',
    # Every feature on in the owner's own configuration, each by its own key
    # and its alias where it has one: the override must beat them all. It
    # ends with a blank line, as the fake's trust entry expects, so that
    # entry is the only change the configuration row sees.
    'overrides-on': 'web_search = "live"\n[features]\nmemories = true\nmemory_tool = true\n'
                    'goals = true\nimage_generation = true\nimagegenext = true\n'
                    'multi_agent = true\nmulti_agent_v2 = true\n[agents]\nenabled = true\n\n'}
# The mutants the live check applies to in a rehearsal, with the feature
# each must be refused for, and the words that say why.
UNMETERED_REFUSED = {
    'subagents-unguarded': ('sub-agents', 'names multi-agent v2, so sub-agents are on'),
    'subagents-multi-agent-only': ('sub-agents', 'names multi-agent v2, so sub-agents are on'),
    'memory-unguarded': ('memories', "features.memories = true, the owner's config.toml"),
    'memory-alias': ('memories', "features.memory_tool = true, the owner's config.toml"),
    'goals-unguarded': ('goals', "Codex's default: on"),
    'web-search-unguarded': ('standalone web search', "Codex's default: cached"),
    'image-generation-unguarded': ('image generation', "Codex's default: on"),
    'override-misses-alias': ('memories', "features.memory_tool = true, the owner's config.toml"),
    'decision-absent': ('all five', "features.memories = true, the owner's config.toml")}
# The mutants whose lead has a budget of three, so the runner's third start
# is admitted.
THIRD_ADMITTED = ('third-admitted', 'probe-over-share', 'probe-charged-flat')
# The mutants that send the fake's own token.
OVERRIDE_MUTANTS = ('overrides-on', 'override-misses-alias')
# The mutants that play a Codex whose unmetered features are on: L3's
# decision is not sent for them, so the fake does what each feature does,
# and the rows behind the override are shown to catch it.
FEATURES_ON_MUTANTS = ('subagent-spawned', 'subagent-not-stopped', 'subagent-uncounted',
                       'memory-pipeline-ran', 'goal-continued', 'continuation-uncharged')

MUTANTS = {
    'no-tool': 'Only the lead got the tool',
    'reports-refused-as-started': 'A third start is refused by PIO',
    'grant-may-answer': 'The lead may not answer an approval',
    'grant-no-steer': 'A steer while the child runs',
    'wrong-child': 'Each child reported the true count',
    'wrong-relay': 'The lead relayed the true counts',
    'lead-without-brief': 'The lead was admitted',
    # The lead's submit is answered with an error, so no execution exists:
    # it is charged nothing, not its share (review of L3, round 2, SB-6).
    'lead-submit-invalid': 'The lead was admitted',
    # A budget of three, so the probe the lead's budget should refuse is
    # admitted: it must be charged its share, off no reservation (SB-5).
    'third-admitted': 'A third start is refused by PIO',
    'desk-silent': 'Every approval was decided at the desk',
    'lead-loops': 'Every run stayed within its ceilings',
    'interrupted': 'The run finished without an error',
    'runner-killed': 'The ledger holds the reservation',
    'release-refused': 'The service was released, and nothing it started survived',
    # Not a row: the run fails before it has any. What must hold is that the
    # tree it made is gone.
    'setup-fails': 'A setup that fails leaves no tree behind',
    # Not a row either: the runner refuses to start (owner decision,
    # 2026-09-25), before anything is reserved.
    'helper-elsewhere': "The runner refuses a helper model outside the plan's provider",
    # Not a row either: the runner refuses to start when Codex could ask for
    # permissions PIO is never shown (review of L3, round 2).
    'experimental-feature-on': 'The runner refuses a Codex that can ask for unseen permissions',
    'experimental-alias-on': 'The runner refuses a Codex that can ask for unseen permissions',
    # L1b and L3.
    'no-wait': 'A read_run waited for its run: until it exited, or to the wait limit',
    # L3 only.
    'wrong-model': "Each run was on the plan's model before its turn started",
    'reviewer-elsewhere': 'Every run asserted that approvals go to the user',
    'no-pre-allow': "The lead's own two tools were pre-allowed, and nothing else",
    'child-overspends': 'Every run stayed within its ceilings',
    # The lead starts its children under other names (review of L3, round 3,
    # SPEND-1): the tool refuses them, so no run starts; or, with the tool's
    # check let through, the runner must still meter, stop and charge them:
    # the renamed alpha passes its ceiling.
    'child-renamed': 'The lead started its two runs through the tool',
    # The lead's tool sent without `omit_tools_from`, as in L3's first live
    # run: on a code-mode-only model its tools are deferred behind `exec`
    # and never named, so the lead cannot call them (2026-09-26).
    'lead-tool-deferred': 'The lead started its two runs through the tool',
    'child-renamed-unchecked': 'Every run stayed within its ceilings',
    # Shapes PIO declines by itself (review of L3, CH-2/F1): the lead's
    # approval asked in a mode PIO does not recognise, and a child's
    # permission grant.
    'lead-asked-in-openai-form': "The lead's own two tools were pre-allowed, and nothing else",
    'lead-asked-by-user-input': "The lead's own two tools were pre-allowed, and nothing else",
    'child-asks-permissions': 'Every approval was decided at the desk',
    # The service refuses to start: nothing was submitted, so no reservation
    # may stand and the sequence may not be blocked (review of L3, F4).
    'service-never-ready': 'The run finished without an error',
    # child-overspends' play, with the runner changed: its meter decides the
    # stop and never sends it (F3), or its charge for a stopped run is only
    # what the run reported (F2).
    'ceiling-cancel-never-sent': 'Every stop the runner made reached Codex',
    'stop-charged-reported': "Every run's charge covers what it could have spent",
    # A child that ignores turn/interrupt, stopped past its share, and the
    # runner interrupted: the run is still going when its usage is read.
    # Charged what it reported plus a step, past its share, so the share
    # row fails; charged only its share, the floor row fails (review of L3,
    # round 2, SB-2).
    'stop-ignored': "Every run's charge stayed within its reserved share",
    'not-exited-charged-share': "Every run's charge covers what it could have spent",
    # Codex reports no usage at all: nothing can stop a run at its ceiling,
    # so the meter must stop it for its silence (review of L3, F9).
    'usage-suppressed': 'Every run stayed within its ceilings',
    # The lead's steps are a whole step in flight each, and it reads once
    # more: its tool must withhold that result and the runner stop it, so
    # it ends within its share (review of L3, A7).
    'lead-heavy': 'Every run stayed within its ceilings',
    # usage-suppressed, with both children asking a command approval that
    # the desk answers: alpha must still be stopped for its silence after
    # its answer (review of L3, round 2, SB-3).
    'asked-silent': 'Every run stayed within its ceilings',
    # The store holds a qualification whose native binary is not the one
    # the committed record qualified (review of L3, REPIN-2).
    'qualified-elsewhere': 'Codex qualified at the pinned identity',
    # A lead step past the hold that ends in a lead-tool error, or in
    # Codex's own shell: the error must be held like any result, and the
    # shell's result met by a stop (review of L3, round 2, SB-1). With the
    # gate skipped for errors, or the meter blind to such items, the row that
    # checks every result against the hold fails.
    'lead-tool-error-past-hold': 'Every run stayed within its ceilings',
    'lead-shell-past-hold': 'Every run stayed within its ceilings',
    'tool-error-ungated': 'Every result the lead got was checked against its hold',
    'meter-ignores-items': 'Every result the lead got was checked against its hold',
    # The runner reads every message a child said, as it did, not its
    # answer: a preamble that quotes the command is taken for the count.
    'first-number-of-all': 'Each child reported the true count',
    # A child that spawns a sub-agent, as Codex 0.157.0 does (review of L3,
    # round 3, SPEND-2): the runner must stop it when the agent appears and
    # charge it a step in flight on each of its threads. With the stop
    # skipped, or the charge blind to the agent's thread, the rows that
    # check each fail.
    'subagent-spawned': 'No run spawned a sub-agent',
    'subagent-not-stopped': 'Every run that spawned a sub-agent was stopped when it appeared',
    'subagent-uncounted': "Every run's charge covers what it could have spent",
    # The live check on Codex's unmetered features, applied to the
    # rehearsal's own Codex home, with every other feature off by the
    # owner's own keys (review of L3, round 4, U1): sub-agents by Codex's
    # default, and with `multi_agent = false` alone; memories on, and on by
    # its alias `memory_tool` after `memories = false`; goals, standalone
    # web search and image generation by Codex's defaults. And, under the
    # fake's token, an override that forgot memories' alias beside the
    # owner's `memory_tool = true`. Each is refused before anything is
    # started or reserved, naming its feature and nothing else.
    **{m: UNMETERED_REFUSAL for m in (
        'subagents-unguarded', 'subagents-multi-agent-only', 'memory-unguarded', 'memory-alias',
        'goals-unguarded', 'web-search-unguarded', 'image-generation-unguarded',
        'override-misses-alias', 'decision-absent')},
    # A child's first step is 40,000, past the 30,000 the bound assumes in
    # flight: the runner must stop it, and the row fails (SPEND-4).
    'step-past-in-flight': 'Every step stayed within the in-flight bound',
    # A child stopped at its ceiling that ignores the stop, answers and
    # exits: charged what it reported plus a step, past its share, so the
    # share row fails; with the round-1 cap put back, the floor row fails
    # (SPEND-7).
    'stopped-past-share': "Every run's charge stayed within its reserved share",
    'stopped-charge-capped': "Every run's charge covers what it could have spent",
    # A child the host's own execution deadline interrupts: charged as a
    # stopped run; with the charge reading the runner's stops alone, the
    # floor row fails (SPEND-6).
    'deadline-interrupted': 'Each child reported the true count',
    'interrupted-charged-reported': "Every run's charge covers what it could have spent",
    # Codex's memory pipeline writes its files and database during the run,
    # as 0.157.0's does with [features] memories on (review of L3, round 3,
    # S6): the row that lists them fails, and no name an owner's session
    # could have given is in the receipt.
    'memory-pipeline-ran': "Codex's memory pipeline wrote nothing during the run",
    # Codex starts a turn on beta's own thread by itself once beta's has
    # ended, as a goal's continuation does (review of L3, round 4, SPEND-9):
    # the host interrupts it and the row that looks for one fails; beta is
    # charged as cut short. With that charge put back to "ended by itself",
    # the floor row fails too.
    'goal-continued': 'No run took a turn of its own after its turn ended',
    # The runner is interrupted while alpha's 40,000-token step is in
    # flight, so the host reports it after the meter has stopped (review of
    # L3, round 4, SPEND-11): the charge must read the host's files, and
    # the floor row hold; with the meter's fold alone, the floor row fails.
    'late-step-after-halt': 'The run finished without an error',
    # The runner's third start, admitted, spends past its share (review of
    # L3, round 4, SPEND-12): charged through the same branches as any run,
    # past its share, so the share row fails and the floor holds; charged
    # its flat share, as before, the floor row fails.
    'probe-over-share': "Every run's charge stayed within its reserved share",
    'probe-charged-flat': "Every run's charge covers what it could have spent",
    'stale-meter-charged': "Every run's charge covers what it could have spent",
    'continuation-uncharged': "Every run's charge covers what it could have spent",
    # The meter thread fails as the third start begins (review of L3, round
    # 3, SPEND-5): it must stop every run itself, at once, and the run ends
    # with the error.
    'meter-dies': 'The run finished without an error',
}
# L1b only: a mutant that must leave its row **inconclusive**, not failed. A
# desk that was never asked has decided nothing, and must not say it has
# (review 47).
INCONCLUSIVE_MUTANTS = {'no-ask': 'Every approval was decided at the desk',
                        # L3: the lead's exit carried no list of what PIO
                        # declined, and it was not seen to exit: nothing is
                        # decided about the pre-allowance (review of L3, round
                        # 3, C3-3).
                        'lead-exit-uncarried': "The lead's own two tools were pre-allowed, "
                                               'and nothing else',
                        # No OpenCode service of the owner's before or after:
                        # nothing observed (C3-4).
                        'owner-service-absent': "The owner's OpenCode service was untouched"}
# L1b only: a mutant whose row must still **hold**. `alpha` outlasts the lead
# tool's wait, so its first read comes back still running at the limit, and
# its second comes back `exited` too soon to count. Only the limit can hold
# the row.
HOLDING_MUTANTS = {'alpha-outlasts': 'A read_run waited for its run: until it exited, or to the wait limit',
                   # L3: the committed record itself, planted: the row must
                   # hold, so it is not a row that can only fail.
                   'qualified-as-committed': 'Codex qualified at the pinned identity',
                   # L3: Codex's unmetered features off per launch, under the
                   # labeled fake's own token, over an owner's configuration
                   # that turns every one on: the child that would spawn a
                   # sub-agent does not, and no memory pipeline runs.
                   'overrides-on': 'No run spawned a sub-agent'}
# The plans a mutant belongs to; any other mutant belongs to every plan.
PLAN_MUTANTS = {**{m: {'L3'} for m in (
                    'subagent-spawned', 'subagent-not-stopped', 'subagent-uncounted',
                    'subagents-unguarded', 'subagents-multi-agent-only', 'overrides-on',
                    'memory-unguarded', 'memory-alias', 'goals-unguarded', 'web-search-unguarded',
                    'image-generation-unguarded', 'override-misses-alias', 'decision-absent',
                    'step-past-in-flight', 'stopped-past-share', 'stopped-charge-capped',
                    'deadline-interrupted', 'interrupted-charged-reported',
                    'memory-pipeline-ran', 'meter-dies', 'lead-exit-uncarried',
                    'goal-continued', 'continuation-uncharged', 'late-step-after-halt',
                    'stale-meter-charged', 'probe-over-share', 'probe-charged-flat')},
                'no-wait': {'L1b', 'L3'}, 'no-ask': {'L1b'}, 'alpha-outlasts': {'L1b'},
                'helper-elsewhere': {'L1', 'L1b'}, 'wrong-model': {'L3'},
                'reviewer-elsewhere': {'L3'}, 'no-pre-allow': {'L3'},
                'child-overspends': {'L3'}, 'child-renamed': {'L3'},
                'child-renamed-unchecked': {'L3'}, 'lead-asked-in-openai-form': {'L3'},
                'lead-tool-deferred': {'L3'},
                'child-asks-permissions': {'L3'}, 'first-number-of-all': {'L3'},
                'lead-asked-by-user-input': {'L3'},
                'ceiling-cancel-never-sent': {'L3'}, 'stop-charged-reported': {'L3'},
                'usage-suppressed': {'L3'}, 'lead-heavy': {'L3'},
                'qualified-elsewhere': {'L3'}, 'qualified-as-committed': {'L3'},
                'lead-tool-error-past-hold': {'L3'}, 'lead-shell-past-hold': {'L3'},
                'tool-error-ungated': {'L3'}, 'meter-ignores-items': {'L3'},
                'stop-ignored': {'L3'}, 'not-exited-charged-share': {'L3'},
                'experimental-feature-on': {'L3'}, 'experimental-alias-on': {'L3'}, 'asked-silent': {'L3'}}


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def private(path):
    """A directory only this user can enter; the service refuses anything else."""
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o700)
    return path


def live_tree(name):
    """A private tree under `~/pio-m4-live`, registered with the cleanup.

    The M3b runner registers its own tree; L1's first live run did not, and
    the release refused it at the end (review 46). Every rehearsal makes a
    probe here through this same function, checks the cleanup would release
    it, and releases it, so the live side of this is rehearsed too.
    """
    base = private(live.HOME / 'pio-m4-live')
    case_cleanup.permit_prefix(base)
    return private(base / f'{name}-{uuid.uuid4().hex[:8]}')


def fixture(root):
    """The workspace the lead and its children share. Counts are the runner's."""
    repo = private(root / 'fixtures') / 'lead'
    repo.mkdir()
    for name, lines in FILES.items():
        # A placeholder `.env` says so on every line and holds nothing that
        # looks like a setting or a key. It exists only in this fixture.
        text = 'placeholder line {n}, not a setting' if name.endswith('.env') \
            else 'line {n}'
        (repo / name).write_text(''.join(text.format(n=n) + '\n'
                                         for n in range(1, lines + 1)))
    (repo / 'README.md').write_text(f'{LEAD} fixture. A throwaway repository.\n')
    git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True,
                                    capture_output=True, text=True).stdout.strip()
    git('init', '-q')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
        'commit', '-q', '-m', 'fixture')
    return repo, git('rev-parse', 'HEAD')


def wc_l(repo):
    """What the files say, counted by the runner and nobody else."""
    return {name: len((repo / name).read_text().splitlines()) for name in FILES}


def helper_models(config_dir):
    """Every model the OpenCode configuration names outside a session's own.

    Review 52, a static reading of OpenCode 2.0.11, not measured: its title
    and compaction helpers use the session's own provider and model, unless
    `small_model` or a model override on a helper says otherwise. So this
    reads `small_model` and the `model` of every `agent` and `mode` entry,
    from the non-secret configuration files only, and nothing else in them.
    """
    models, read = {}, []
    for name in ('opencode.json', 'opencode.jsonc'):
        path = Path(config_dir) / name
        if not path.exists():
            continue
        body = ''.join(line for line in path.read_text().splitlines(keepends=True)
                       if not line.strip().startswith('//'))
        try:
            config = json.loads(body)
        except ValueError as error:
            raise SystemExit(f'refusing to start: {name} is unreadable, so its helper '
                             f'models cannot be checked: {error}')
        read.append(name)
        if config.get('small_model'):
            models[f'{name}: small_model'] = config['small_model']
        for section in ('agent', 'mode'):
            for entry, settings in (config.get(section) or {}).items():
                if isinstance(settings, dict) and settings.get('model'):
                    models[f'{name}: {section}.{entry}.model'] = settings['model']
    return dict(read=read, models=models)


def helpers_elsewhere(helpers):
    """The helper models whose provider is not the plan's (owner decision,
    2026-09-25: the runner refuses to start on any)."""
    provider = MODEL.split('/', 1)[0]
    return {k: m for k, m in helpers['models'].items()
            if str(m).split('/', 1)[0] != provider}


# Codex features that let a command approval ask for more permissions than
# its command: a field Codex strips for a client that did not opt into its
# experimental API, as PIO does not. With either on, the desk (and the L3
# relay's allow) would grant a widening nobody saw (review of L3, round 2).
WIDENING_FEATURES = ('exec_permission_approvals', 'request_permissions_tool')
# Every key Codex 0.157.0 reads as each feature: its own, and every legacy
# alias `codex-rs/features/src/legacy.rs` lists for it at rust-v0.157.0
# (`request_permissions` is `exec_permission_approvals`; the other has
# none). `[features]` is a sorted map, and Codex applies its keys in order,
# each setting its feature (`Features::apply_map`), so the last of a
# feature's keys in that order decides it (review of L3, round 3, SPEND-3).
FEATURE_KEYS = {'exec_permission_approvals': ('exec_permission_approvals', 'request_permissions'),
                'request_permissions_tool': ('request_permissions_tool',)}
# A labeled fake's own token for turning Codex's unmetered features off per
# launch (sub-agents among them); a real Codex takes only a recorded owner
# decision (pio-protocol stream.rs, FEATURES_OFF_DECISIONS: the owner's of
# 2026-09-26, which the L3 plan sends).
FEATURES_OFF_REHEARSAL = 'rehearsal-only-features-off'
# What the host puts in each thread's config under that decision or token,
# key by key (pio_codex::features_off, rust-v0.157.0; review of L3, round 4).
FEATURES_OFF = {'agents.enabled': False, 'features.multi_agent': False,
                'features.multi_agent_v2': False, 'features.memories': False,
                'features.memory_tool': False, 'features.goals': False,
                'web_search': 'disabled', 'features.image_generation': False}


def feature_setting(features, feature):
    """What Codex would make of one feature from a `[features]` table, and
    the key that decided it: `None` where no key names it (Codex's default)
    or the deciding key holds something Codex would not read as a switch."""
    keys = sorted(k for k in FEATURE_KEYS[feature] if k in features)
    if not keys:
        return None, None
    value = features[keys[-1]]
    return (value if isinstance(value, bool) else None), keys[-1]


def codex_features(codex_home):
    """The feature switches the runner checks, and nothing else, from the
    Codex configuration: `[features]` in `config.toml`, each by its own key
    or a legacy alias, with the key that decided it. Whatever else the file
    holds is parsed and not kept."""
    path = Path(codex_home) / 'config.toml'
    if not path.exists():
        return dict(read=str(path.name), exists=False,
                    **{k: None for k in FEATURE_KEYS}, **{f'{k}_key': None for k in FEATURE_KEYS})
    try:
        features = tomllib.loads(path.read_text()).get('features') or {}
    except tomllib.TOMLDecodeError as error:
        raise SystemExit(f'refusing to start: {path.name} is unreadable, so its features '
                         f'cannot be checked: {error}')
    if not isinstance(features, dict):
        features = {}
    record = dict(read=str(path.name), exists=True)
    for feature in FEATURE_KEYS:
        record[feature], record[f'{feature}_key'] = feature_setting(features, feature)
    return record


# Codex 0.157.0's unmetered, default-on features (review of L3, round 4,
# U1): each spends outside every report PIO reads. Sub-agents run threads of
# their own; the memory pipeline sends the owner's idle sessions to a model
# and runs a consolidation agent at a root thread's first turn (on the order
# of 900,000 input tokens an attempt); a goal starts continuation turns by
# itself after turn/completed; standalone web search (`web.run`) makes its
# own model call and records no usage; image generation calls a separate
# endpoint and records none. The keys and their sources at rust-v0.157.0
# are in `crates/pio-codex/src/lib.rs` (`features_off`).
UNMETERED = ('sub-agents', 'memories', 'goals', 'standalone web search', 'image generation')
# The keys the preflight reads, and nothing else: each feature's own and
# its legacy aliases (`features/src/legacy.rs`), `[agents] enabled`, and the
# top-level `web_search`.
UNMETERED_KEYS = ('features.multi_agent', 'features.collab', 'features.multi_agent_v2',
                  'agents.enabled', 'features.memories', 'features.memory_tool',
                  'features.goals', 'web_search', 'features.web_search',
                  'features.web_search_cached', 'features.web_search_request',
                  'features.image_generation', 'features.imagegenext')
WEB_SEARCH_MODES = ('disabled', 'cached', 'indexed', 'live')


def unmetered_keys(codex_home):
    """The keys that decide Codex's unmetered features, and nothing else,
    from `config.toml`: whatever else the file holds is parsed and not kept.
    A `[features] multi_agent_v2` table is kept by its `enabled` alone. An
    unreadable file refuses: nothing it says can be known."""
    path = Path(codex_home) / 'config.toml'
    if not path.exists():
        return path, {}
    try:
        config = tomllib.loads(path.read_text())
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        raise SystemExit(f"refusing to start: {path.name} is unreadable, so whether Codex's "
                         f'unmetered features are off cannot be checked: {error}')
    found = {}
    for dotted in UNMETERED_KEYS:
        table, _, name = dotted.rpartition('.')
        holder = config.get(table) if table else config
        if isinstance(holder, dict) and name in holder:
            value = holder[name]
            if dotted == 'features.multi_agent_v2' and isinstance(value, dict):
                value = dict(table=True, enabled=value.get('enabled'))
            found[dotted] = value
    return path, found


def unmetered_features(codex_home, override=None, decision=None):
    """Whether each of Codex's unmetered features is off on L3's threads, and
    by which route: the owner's own `config.toml`, the per-launch override
    under a recorded decision, or Codex's default.

    The override's keys are laid over the owner's, as Codex merges a request
    override over the file (`app-server/src/config_manager.rs:446-452`), and
    each feature is then resolved as rust-v0.157.0 resolves it: a feature's
    keys, its own and its legacy aliases, applied in sorted order, the last
    deciding (`Features::apply_map`); `imagegenext` ignored beside
    `image_generation` (`features/src/lib.rs:645`); `web_search` deciding
    the mode before any feature (`core/src/config/mod.rs:2659-2670`); and
    sub-agents off only with `agents.enabled` false and `multi_agent_v2`
    not on (`core/src/config/mod.rs:1562-1570`), since `gpt-5.6-terra`'s
    catalog entry names multi-agent v2. A key Codex would not read as a
    switch, or a mode it does not have, is not off."""
    path, owner = unmetered_keys(codex_home)
    merged = {k: (v, 'owner') for k, v in owner.items()}
    merged.update({k: (v, 'override') for k, v in (override or {}).items()})

    def said(key):
        value, source = merged[key]
        where = f'per launch, under {decision!r}' if source == 'override' \
            else "the owner's config.toml"
        shown = json.dumps(value) if not isinstance(value, dict) else \
            f"a table with enabled = {json.dumps(value.get('enabled'))}"
        return dict(key=key, value=value, route=where, why=f'{key} = {shown}, {where}')

    def switch(*keys, default):
        present = sorted(k for k in keys if k in merged)
        if not present:
            return dict(off=not default, key=None, value=None, route="Codex's default",
                        why=f"Codex's default: {'on' if default else 'off'} "
                            f"({', '.join(keys)} unset)")
        found = said(present[-1])
        on = found['value'] if isinstance(found['value'], bool) else None
        return dict(found, off=None if on is None else not on)

    features = {}
    # Sub-agents: V2 if multi_agent_v2 is on; off if agents.enabled is false;
    # else the model's catalog, which names v2 for gpt-5.6-terra.
    v2 = merged.get('features.multi_agent_v2', (None, None))[0]
    v2_on = v2 is True or (isinstance(v2, dict) and v2.get('enabled') is True)
    v2_unread = 'features.multi_agent_v2' in merged and not isinstance(v2, (bool, dict))
    agents = switch('agents.enabled', default=True)
    if v2_unread or v2_on:
        features['sub-agents'] = dict(said('features.multi_agent_v2'),
                                      off=None if v2_unread else False)
    elif agents['off'] is True:
        features['sub-agents'] = agents
    else:
        features['sub-agents'] = dict(agents, why=agents['why'] + f"; {MODEL}'s catalog entry "
                                      'names multi-agent v2, so sub-agents are on')
    features['sub-agents']['multi_agent'] = switch('features.multi_agent', 'features.collab',
                                                   default=True)['off'] is False
    features['memories'] = switch('features.memories', 'features.memory_tool', default=False)
    features['goals'] = switch('features.goals', default=True)
    if 'web_search' in merged:
        found = said('web_search')
        mode = found['value'] if found['value'] in WEB_SEARCH_MODES else None
        features['standalone web search'] = dict(found, off=None if mode is None
                                                 else mode == 'disabled')
    else:
        cached = switch('features.web_search_cached', default=False)
        live = switch('features.web_search', 'features.web_search_request', default=False)
        chosen = cached if cached['key'] and cached['off'] is False else \
            live if live['key'] and live['off'] is False else None
        features['standalone web search'] = dict(
            chosen, off=False, why=chosen['why'] + '; the top-level web_search is unset') \
            if chosen else dict(off=False, key=None, value=None, route="Codex's default",
                                why="Codex's default: cached (web_search unset)")
    if 'features.image_generation' in merged:
        features['image generation'] = switch('features.image_generation', default=True)
    else:
        features['image generation'] = switch('features.imagegenext', default=True)
        if 'features.imagegenext' not in merged:
            features['image generation']['why'] = \
                "Codex's default: on (image_generation, imagegenext unset)"
    return dict(read=path.name, exists=path.exists(), decision=decision,
                owner_keys=owner, features=features)


def features_not_off(unmetered):
    """Each feature that is not off, and why."""
    return {f: u['why'] for f, u in unmetered['features'].items() if u['off'] is not True}


# Codex's memory state (review of L3, round 3, S6). With `[features]
# memories = true`, as the owner has it, the app-server starts a background
# memory pipeline at a root thread's first turn with input (0.157.0,
# app-server turn_processor.rs, codex_memories_write::
# start_memories_startup_task): Phase 1 sends up to two of the owner's
# recent idle sessions to a model and stores what it extracts in
# `memories_1.sqlite`; Phase 2 syncs files under `memories/` (a git
# baseline, `raw_memories.md`, `rollout_summaries/`,
# `phase2_workspace_diff.md`) and runs a consolidation agent that edits
# `MEMORY.md`, `memory_summary.md` and `skills/`. None of it is in
# `config.toml`, none of it reports on PIO's connection (the agent is
# started with `start_thread`, which attaches no connection), and its model
# calls are outside every meter. Its paths are listed before and after, by
# a name Codex chose or a digest of any other name, size and time: never
# content, never a name the owner's sessions could have given.
MEMORY_DIRECTORIES = ('memories', 'memories_v2')
MEMORY_DATABASES = ('memories_1.sqlite', 'memories_v2_1.sqlite')
MEMORY_NAMES = {'raw_memories.md', 'MEMORY.md', 'memory_summary.md', 'phase2_workspace_diff.md',
                'rollout_summaries', 'skills', 'extensions', 'ad_hoc', 'notes',
                'instructions.md', 'SKILL.md'}


def memory_state(codex_home):
    """Codex's memory files and databases under `codex_home`, by name (a
    digest for any name Codex did not choose), size and modification time.
    The git baseline is counted, not listed."""
    home = Path(codex_home)
    state = {}
    for name in MEMORY_DIRECTORIES:
        root = home / name
        if not root.is_dir():
            state[name] = None
            continue
        entries, git = {}, dict(files=0, bytes=0, newest=None)
        for path in sorted(root.rglob('*')):
            relative = path.relative_to(root)
            stat = path.lstat()
            if relative.parts[0] == '.git':
                if path.is_file():
                    git.update(files=git['files'] + 1, bytes=git['bytes'] + stat.st_size,
                               newest=max(git['newest'] or 0, int(stat.st_mtime)))
                continue
            key = '/'.join(part if part in MEMORY_NAMES else f'sha256:{sha(part)[:12]}'
                           for part in relative.parts)
            entries[key] = dict(kind='dir' if path.is_dir() else 'file',
                                size=None if path.is_dir() else stat.st_size,
                                mtime=int(stat.st_mtime))
        state[name] = dict(entries=entries, git=git)
    for name in MEMORY_DATABASES:
        for suffix in ('', '-wal', '-shm'):
            path = home / f'{name}{suffix}'
            state[f'{name}{suffix}'] = dict(size=path.stat().st_size,
                                            mtime=int(path.stat().st_mtime)) \
                if path.is_file() else None
    return state


def memory_changes(before, after):
    """What changed between two listings: each path or database added,
    removed or changed in size or time."""
    changes = []
    empty = dict(entries={}, git=dict(files=0, bytes=0, newest=None))
    for name in sorted(set(before) | set(after)):
        old, new = before.get(name), after.get(name)
        if name in MEMORY_DIRECTORIES and (old is not None or new is not None):
            # A directory made or removed during the run: each entry apart.
            if (old is None) != (new is None):
                changes.append(dict(path=name, before=None if old is None else 'present',
                                    after=None if new is None else 'present'))
            old, new = old or empty, new or empty
            for key in sorted(set(old['entries']) | set(new['entries'])):
                a, b = old['entries'].get(key), new['entries'].get(key)
                if a != b:
                    changes.append(dict(path=f'{name}/{key}', before=a, after=b))
            if old['git'] != new['git']:
                changes.append(dict(path=f'{name}/.git', before=old['git'], after=new['git']))
        elif old != new:
            changes.append(dict(path=name, before=old, after=new))
    return changes


def fresh_credential(principal):
    return f'ccred1.{principal}.' + base64.urlsafe_b64encode(
        os.urandom(32)).decode().rstrip('=')


def now():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def scrub(value):
    """What may leave this machine: home paths redacted, and the repository
    and any other volume path named rather than spelled out. `redact`
    rewrites only home paths, and the repository lives on a volume."""
    text = json.dumps(redact(value)).replace(str(ROOT), '<repo>')
    return json.loads(re.sub(r'/Volumes/[^/"\\]+', '<volume>', text))


class Rows:
    """Every observation, its expected value, and whether they agree."""

    def __init__(self, rehearse):
        self.rehearse = rehearse
        self.rows = []

    def add(self, name, observed, expected, holds=None, live_only=False, note=''):
        """`holds` may answer None: the observation could not decide the row.
        That is recorded as inconclusive, neither held nor failed."""
        agrees = holds(observed) if holds else observed == expected
        provable = not (live_only and self.rehearse)
        verdict = bool(agrees) if provable and agrees is not None else None
        if provable and agrees is None:
            note = f'{note}; inconclusive' if note else 'inconclusive'
        self.rows.append(dict(row=name, kind='row', observed=observed, expected=expected,
                              holds=verdict, proven=verdict is True, note=note))

    def record(self, name, observed, note=''):
        """What was seen, with nothing to compare it against. Never proven."""
        self.rows.append(dict(row=name, kind='record', observed=observed, expected=None,
                              holds=None, proven=False, note=note))

    def failed(self):
        return [r['row'] for r in self.rows if r['holds'] is False]


class Service:
    """One service for the lead and its children, rehearsed or live:
    `serve-opencode` for L1 and L1b, `serve-codex` for L3."""

    def __init__(self, root, rehearse, scenario, mutant=None, features_off=None):
        self.root = root
        self.rehearse = rehearse
        self.store = root / 'store'
        self.socket = private(root / 'socket') / 'public.sock'
        if len(str(self.socket).encode()) > 100:
            raise SystemExit(f'socket path too long for the platform: {self.socket}')
        self.owner_credential = fresh_credential('owner')
        self.lead_credential = fresh_credential('lead')
        protocol = dict(format='combraton-conformance-config/1', principal='owner',
                        provider_id=PROVIDER,
                        credentials=[dict(credential=self.owner_credential),
                                     dict(credential=self.lead_credential)],
                        executor=dict(host_id=f'{HARNESS}-host'))
        if HARNESS == 'codex':
            self.config = dict(format='pio-codex-service/1', protocol=protocol,
                               codex=self.codex(root, rehearse, scenario))
            if features_off:
                # Codex's unmetered features off per launch (review of L3,
                # round 3, SPEND-2; round 4).
                self.config['codex']['features_off_decision'] = features_off
        else:
            self.config = self.opencode(root, rehearse, scenario, protocol)
        if mutant == 'service-never-ready':
            # A setting the service refuses at start, as serve-codex refuses
            # a Codex it cannot qualify: it exits before it is ever ready.
            self.config[HARNESS]['refused_at_start_by_the_mutant'] = True
        self.config_path = root / 'service.json'
        self.config_path.write_text(json.dumps(self.config))
        os.chmod(self.config_path, 0o600)
        self.daemon = None

    @staticmethod
    def codex(root, rehearse, scenario):
        """The owner's Codex, as installed and configured, or the labeled
        fake. The model is asked for under the dated exception, and the
        provider is only ever checked, never sent (owner, 2026-09-25)."""
        if rehearse:
            executable = root / 'fake-codex'
            executable.write_text(f"#!/bin/sh\nexec '{BINARY}' codex fake-app-server \"$@\"\n")
            executable.chmod(0o755)
            home = root
            env = {'PATH': '/usr/bin:/bin', 'HOME': str(home),
                   'PIO_CODEX_FAKE_SCENARIO': json.dumps(scenario)}
        else:
            # Exactly M2's live environment. PIO passes no key: Codex
            # authenticates itself from its own home.
            home = live.HOME
            executable = home / '.local/bin/codex'
            env = {'PATH': f'{home}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                   'HOME': str(home), 'USER': os.environ.get('USER', ''),
                   'LANG': 'en_US.UTF-8'}
        return dict(executable=str(executable), env=env, codex_home=str(home / '.codex'),
                    fixture_root=str(root / 'fixtures'),
                    thread=dict(sandbox='workspace-write', approvalPolicy='on-request',
                                model=MODEL),
                    labeled_fake=rehearse, test_only_model_exception=live.MODEL_EXCEPTION,
                    expected_model_provider=CODEX['model_provider'])

    def opencode(self, root, rehearse, scenario, protocol):
        if rehearse:
            home = root
            executable = root / 'fake-opencode'
            executable.write_text(f"#!/bin/sh\nexec '{BINARY}' opencode fake-acp \"$@\"\n")
            executable.chmod(0o755)
            config_dir = private(root / 'opencode-config')
            env = {'PATH': '/usr/bin:/bin', 'HOME': str(home),
                   'USER': os.environ.get('USER', 'pio'),
                   'PIO_OPENCODE_FAKE_SCENARIO': json.dumps(scenario)}
        else:
            # The owner's OpenCode, exactly as they configured it. PIO passes
            # no key: OpenCode authenticates itself.
            home = live.HOME
            executable = Path(shutil.which('opencode2')
                              or str(home / '.local/bin/opencode2'))
            config_dir = home / '.config/opencode'
            env = {'PATH': f'{home}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                   'HOME': str(home), 'USER': os.environ.get('USER', '')}
        return dict(
            format='pio-opencode-service/1', protocol=protocol,
            opencode=dict(executable=str(executable), env=env,
                          config_dir=str(config_dir), home=str(home),
                          fixture_root=str(root / 'fixtures'),
                          labeled_fake=rehearse, model=MODEL,
                          test_only_model_exception=live.MODEL_EXCEPTION))

    def start(self):
        out = (self.root / 'daemon.stdout').open('w')
        err = (self.root / 'daemon.stderr').open('w')
        self.daemon = subprocess.Popen(
            [str(BINARY), f'serve-{HARNESS}', '--data-dir', str(self.store),
             '--config', str(self.config_path), '--socket', str(self.socket)],
            stdout=out, stderr=err)
        # In its own session, so nothing that kills the runner kills it.
        self.watchdog = subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve()), '--watch-runner',
             str(os.getpid()), '--daemon', str(self.daemon.pid), '--root', str(self.root),
             '--plan', LEAD],
            stdout=(self.root / 'watchdog.log').open('w'), stderr=subprocess.STDOUT,
            start_new_session=True)
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            # A service that refused to start (an unqualified Codex, a
            # setting it does not accept) has exited: say so now, not after
            # the whole wait (review of L3, F4).
            code = self.daemon.poll()
            if code is not None:
                tail = (self.root / 'daemon.stderr').read_text(errors='replace')[-400:]
                raise SystemExit(f'the service exited with code {code} before it became '
                                 f'ready; daemon.stderr ends: {redact(tail)}')
            try:
                owner = self.owner()
                owner.close()
                return
            except (OSError, ValueError, KeyError, AssertionError):
                time.sleep(0.25)
        raise SystemExit('the service did not become ready; see daemon.stderr')

    def owner(self):
        return Caller(self.socket, self.owner_credential, features=FEATURES,
                      execution_features=EXECUTION_FEATURES)

    def lead(self, grant_id):
        return Caller(self.socket, self.lead_credential, grant=grant_id,
                      features=FEATURES, execution_features=EXECUTION_FEATURES)

    def host_events(self, views, briefs):
        """Each execution's own host events.

        The host names its files by invocation, and the journal's invocation
        records carry a host command hash rather than the execution id. The
        public view links the two where usage was reported
        (`usage.observations[].invocation_id`); otherwise the invocation is
        the one whose launch spec carries that run's brief digest. A run
        neither finds is left out, and the rows that need it fail.
        """
        journal = self.store / 'journal.sqlite3'
        if not journal.exists():
            return {}
        with sqlite3.connect(f'file:{journal}?mode=ro', uri=True) as db:
            states = [json.loads(r[0]) for r in db.execute('select state from invocations')]
        by_run = {}
        for identity, current in views.items():
            observations = ((current or {}).get('usage') or {}).get('observations') or []
            invocation = observations[0]['invocation_id'] if observations else None
            if invocation is None and identity in briefs:
                digest = 'sha256:' + sha(briefs[identity])
                invocation = next((s['invocation_id'] for s in states
                                   if (s.get('payload') or {}).get('brief', {}).get('digest')
                                   == digest), None)
            path = self.store / f'{HARNESS}-{invocation}.events.jsonl'
            if invocation and path.exists():
                by_run[identity] = [json.loads(l) for l in path.read_text().splitlines()
                                    if l.strip()]
        return by_run

    def release(self, remove):
        if self.daemon and self.daemon.poll() is None:
            self.daemon.kill()
            self.daemon.wait(timeout=10)
        case_cleanup.release(self.root if remove else self.store, remove=remove)


def view(caller, identity):
    answer = caller.query('execution.inspect', {'execution': identity})
    return answer.get('result')


def missing(caller, identity):
    """The service, asked, says it holds no such execution."""
    answer = caller.query('execution.inspect', {'execution': identity})
    return ((answer.get('error') or {}).get('data') or {}).get('code') == 'not_found'


def events(caller):
    """Every execution event, following `next_cursor` to the end: a page
    shorter than the limit is the last. (It read `cursor` and `has_more`,
    which the result does not carry, and so read one page only.)"""
    items, payload = [], {'limit': 1000, 'from': 'start', 'kinds': ['execution.execution']}
    while True:
        result = caller.query('core.events.read', payload).get('result')
        assert result is not None, 'core.events.read refused'
        items += [i['event'] for i in result['items'] if 'event' in i]
        if len(result['items']) < payload['limit']:
            return items
        payload = {'limit': 1000, 'cursor': result['next_cursor'],
                   'kinds': ['execution.execution']}


def in_family(identity):
    """An execution under this lead: the lead, or any id beneath it,
    whatever its name (review of L3, round 3, SPEND-1)."""
    return isinstance(identity, str) and (identity == LEAD or identity.startswith(f'{LEAD}.'))


def adopt(meters, identity, record):
    """Meter a run under this lead that the plan did not name, as a child:
    from here it is metered, stopped, cancelled, relayed at the desk and
    charged like one. The lead's tool starts only the plan's children, so
    this is the second line (review of L3, round 3, SPEND-1)."""
    if identity in meters or not in_family(identity):
        return
    gauge = CodexMeter if HARNESS == 'codex' else Meter
    meters[identity] = gauge(identity, CHILD_CEILING)
    if identity not in PROBES:
        record.setdefault('unplanned_runs', []).append(dict(run=identity, seen_at=now()))
        print(f'UNPLANNED {identity}: metered, stopped and charged as a child', flush=True)


def runs(meters):
    """Every run the watch, the rows and the charge cover: the plan's, then
    any other under this lead the stream has shown, in the order seen. The
    runner's own probes are charged apart."""
    return (*RUNS, *[i for i in list(meters) if i not in RUNS and i not in PROBES])


class Discovery:
    """The executions under this lead that the stream has shown, one read a
    pass from where the last left off, each adopted as it appears."""

    def __init__(self):
        self.cursor = None

    def adopt(self, owner, meters, record):
        while True:
            payload = {'limit': 1000, 'kinds': ['execution.execution']}
            payload.update({'cursor': self.cursor} if self.cursor else {'from': 'start'})
            result = owner.query('core.events.read', payload).get('result')
            if result is None:
                return
            for item in result['items']:
                if 'event' in item:
                    adopt(meters, item['event']['subject']['id'], record)
            self.cursor = result.get('next_cursor') or self.cursor
            if len(result['items']) < payload['limit']:
                return


def spool(caller, identity, offset=0):
    """A run's spooled session updates from `offset`, and where they end."""
    raw = b''
    while True:
        result = caller.query('execution.output.read', {
            'execution': identity, 'offset': offset, 'max_bytes': 65536}).get('result')
        if not result:
            return raw, offset
        raw += base64.b64decode(result['data_base64'])
        if result['next_offset'] == offset:
            return raw, offset
        offset = result['next_offset']


def spoken(caller, identity):
    """What a run said, decoded from its spool the way the lead tool does:
    every message, one to a line, and the last one it completed."""
    raw = spool(caller, identity)[0].decode('utf-8', 'replace')
    return message_text(raw), final_answer(raw)


def first_number(text):
    found = re.search(r'\d+', text or '')
    return int(found.group()) if found else None


def relayed(text):
    """The counts a lead relayed, `<file>: <number>`, for this plan's files."""
    names = '|'.join(re.escape(name) for name in FILES)
    return {m.group(1): int(m.group(2))
            for m in re.finditer(rf'(?<!\w)({names})\s*:\s*(\d+)', text or '')}


def submit(caller, identity, brief, repo, base, origin, extensions):
    body = brief.encode()
    payload = dict(brief=dict(digest='sha256:' + sha(body), media_type='text/plain'),
                   workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                   timeouts=dict(delivery=DELIVERY_TIMEOUT,
                                 execution_deadline=EXECUTION_DEADLINE))
    if origin is not None:
        payload['origin'] = origin
    envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                       payload, command_id=identity)
    envelope['extensions'] = extensions
    return caller.call(envelope)


def steer(caller, identity):
    """One steer under the lead's grant: what came back, and the run's state
    just before and just after it, so the steer is bracketed by the turn."""
    current = view(caller, identity) or {}
    note = b'keep to the fixture'
    envelope = command('execution.steer', dict(kind='execution.execution', id=identity),
                       dict(message=dict(digest='sha256:' + sha(note),
                                         media_type='text/plain')),
                       command_id=f'{identity}.steer-{uuid.uuid4().hex[:6]}',
                       revision=current.get('revision', 0))
    envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=note.decode())}
    answer = caller.call(envelope)
    after = (view(caller, identity) or {}).get('runtime')
    at = dict(runtime_at_steer=current.get('runtime'),
              delivery_at_steer=current.get('delivery'), runtime_after_steer=after)
    if 'error' in answer:
        return dict(at, refused=answer['error']['data'].get('code'))
    outcome = answer['result'].get('outcome', {})
    return dict(at, request=outcome.get('request'), alternative=outcome.get('alternative'),
                delivery_id=outcome.get('delivery_id'))


# What the desk's two decisions are on the wire: OpenCode and Claude take
# `allow` and `deny`; Codex takes `accept` and `decline`, and PIO sends each
# as a single use.
WIRE = {'codex': {'allow': 'accept', 'deny': 'decline'}}


def respond(caller, identity, action_id, decision, revision):
    decision = WIRE.get(HARNESS, {}).get(decision, decision)
    body = json.dumps({'decision': decision}).encode()
    envelope = command('execution.respond_action',
                       dict(kind='execution.execution', id=identity),
                       dict(action_id=action_id,
                            response=dict(digest='sha256:' + sha(body),
                                          media_type='application/json')),
                       command_id=f'{identity}.answer-{action_id}', revision=revision)
    envelope['extensions'] = {CONTENT: dict(media_type='application/json',
                                            text=body.decode())}
    return caller.call(envelope)


def cancel(caller, identity, why):
    current = view(caller, identity) or {}
    envelope = command('execution.cancel', dict(kind='execution.execution', id=identity),
                       {}, command_id=f'{identity}.cancel-{why}',
                       revision=current.get('revision', 0))
    answer = caller.call(envelope)
    if 'error' in answer:
        return dict(refused=answer['error']['data'].get('code'))
    return dict(outcome=answer['result'].get('outcome'))


class Meter:
    """An upper bound on what one run has spent so far, from its own session.

    OpenCode reports usage once, at the end of a turn, and then only for the
    last model step, so nothing the harness says during a turn can stop it.
    What PIO does see is every session update, spooled as it arrives. Each
    tool call is followed by at most one more model step, and no step's
    context can hold more than the harness's fixed base plus everything the
    session has sent so far (a token is at least one byte). So the run has
    spent at most `(tool calls + 1) x (STEP_BASE + bytes)`.
    """

    def __init__(self, identity, ceiling):
        self.identity = identity
        self.ceiling = ceiling
        self.offset = 0
        self.bytes = 0
        self.partial = b''
        self.calls = set()
        self.stopped = None

    def read(self, caller):
        raw, self.offset = spool(caller, self.identity, self.offset)
        self.bytes += len(raw)
        lines = (self.partial + raw).split(b'\n')
        self.partial = lines.pop()
        for line in lines:
            try:
                update = json.loads(line).get('update', {})
            except ValueError:
                continue
            if update.get('sessionUpdate') in ('tool_call', 'tool_call_update') \
                    and update.get('toolCallId'):
                self.calls.add(update['toolCallId'])

    def estimate(self):
        return (len(self.calls) + 1) * (STEP_BASE + self.bytes)

    def summary(self, tool_calls=None):
        return dict(calls=len(self.calls), tool_log_calls=tool_calls, bytes=self.bytes,
                    estimate=self.estimate(), ceiling=self.ceiling, stopped=self.stopped,
                    silenced=getattr(self, 'silenced', False))


class CodexMeter(Meter):
    """What one Codex run has spent so far, from Codex itself.

    Codex reports the thread's running total after every model step
    (`thread/tokenUsage/updated`), and that total covers every step (review
    48), so the view's usage is the spend, not a bound on it. A run stopped
    at its ceiling can finish the one step in flight, which the worst case
    allows for. Tool calls are counted from the run's own spooled items.
    """

    def __init__(self, identity, ceiling):
        super().__init__(identity, ceiling)
        self.total = 0
        # What the event stream said last: the run's runtime and how many
        # usage reports it has made; how long it has been active since its
        # total last rose, and when the meter last looked.
        self.runtime = None
        self.reports = 0
        self.silent = 0.0
        self.looked = None
        self.silenced = False
        # The lead only: results its tool never saw, with its total then.
        self.untooled = []
        # The actions it is waiting on.
        self.asking = set()
        # From the run's own host events, followed in the order the host
        # wrote them (review of L3, round 3, SPEND-2 and SPEND-4): each
        # thread's last total, the run's total as the host summed it, the
        # largest model step any report showed, every thread the run did not
        # start, and how its own turn ended.
        self.invocation = None
        self.host_path = None
        self.host_offset = 0
        self.threads = {}
        self.own_total = 0
        self.host_total = 0
        self.largest_step = 0
        self.subagents = []
        self.turn_status = None
        # Turns Codex started by itself on the run's own thread after its
        # turn had ended, which the host interrupted (round 4, SPEND-9).
        self.continued = []

    def read(self, caller):
        raw, self.offset = spool(caller, self.identity, self.offset)
        self.bytes += len(raw)
        lines = (self.partial + raw).split(b'\n')
        self.partial = lines.pop()
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            item = (record.get('params') or {}).get('item') or {}
            if record.get('method') == 'item/completed' and item.get('type') == 'mcpToolCall':
                self.calls.add(item.get('id'))
        observations = ((view(caller, self.identity) or {}).get('usage') or {}) \
            .get('observations') or []
        if observations and isinstance(observations[0].get('amount'), int):
            self.total = max(self.total, observations[0]['amount'])

    def estimate(self):
        return self.total

    def host_event(self, event):
        """One event from the run's own host events file."""
        kind = event.get('kind')
        if kind == 'usage':
            thread = event.get('thread_id') or 'own'
            total = (event.get('total') or {}).get('totalTokens')
            last = (event.get('last') or {}).get('totalTokens')
            if isinstance(total, int):
                previous = self.threads.get(thread, 0)
                self.largest_step = max(self.largest_step, total - previous)
                self.threads[thread] = max(previous, total)
                if event.get('own_thread', True):
                    self.own_total = max(self.own_total, total)
            if isinstance(last, int):
                self.largest_step = max(self.largest_step, last)
            summed = event.get('run_total', total)
            if isinstance(summed, int):
                self.host_total = max(self.host_total, summed)
        elif kind == 'other_thread':
            self.subagents.append(dict(thread_id=event.get('thread_id'), how=event.get('how'),
                                       at=now()))
            print(f"SUB-AGENT {self.identity}: {event.get('thread_id')} "
                  f"({(event.get('how') or {}).get('by')})", flush=True)
        elif kind == 'turn_completed' and not event.get('continuation'):
            self.turn_status = event.get('status')
        elif kind == 'continuation_started':
            self.continued.append(dict(turn_id=event.get('turn_id'), at=now()))
            print(f"CONTINUED {self.identity}: {event.get('turn_id')} after its own turn; "
                  'interrupted by the host', flush=True)
        elif self.identity == LEAD and untooled(event):
            self.untooled.append(dict(item_type=event.get('item_type'), status=event.get('status'),
                                      server=event.get('server'), total=self.host_total))

    def allowance(self):
        """A step in flight on each of the run's threads when it is stopped:
        the step the bound assumes, or the largest the run was seen to take,
        if larger (review of L3, round 3, SPEND-4 and SPEND-2)."""
        return max(CODEX['in_flight'], self.largest_step) * (1 + len(self.subagents))

    def summary(self, tool_calls=None):
        return dict(super().summary(tool_calls), threads=dict(self.threads),
                    host_total=self.host_total, largest_step=self.largest_step,
                    subagents=list(self.subagents), turn_status=self.turn_status,
                    continued=list(self.continued))

    def observe(self, event):
        """One event from the run's stream."""
        payload = event.get('payload') or {}
        if event['type'] == 'execution.usage.observed' and payload.get('invocation_id'):
            self.invocation = payload['invocation_id']
        if event['type'] == 'execution.usage.observed' and isinstance(payload.get('amount'), int):
            if payload['amount'] > self.total:
                self.silent = 0.0
            self.total = max(self.total, payload['amount'])
            self.reports += 1
        elif event['type'] == 'execution.runtime.changed' and payload.get('runtime'):
            self.runtime = payload['runtime']
            if payload['runtime'] == 'requires_action' and payload.get('action_id'):
                self.asking.add(payload['action_id'])
        elif event['type'] == 'execution.action.answered':
            # The fold sets the view back to `active` when the last pending
            # action settles, and emits no runtime change for it: without
            # this, a run that had asked once never counted as active again,
            # and the silence stop could not fire for it (review of L3, round
            # 2, SB-3).
            self.asking.discard(payload.get('action_id'))
            if self.runtime == 'requires_action' and not self.asking:
                self.runtime = 'active'
        elif event['type'] == 'execution.exit.observed':
            self.runtime = 'exited'


class CodexMetering(threading.Thread):
    """Every Codex run's spend, read as it is reported, on a connection of
    its own and apart from the watch loop (review of L3, F3 and A7).

    Measured in the L3 rehearsal: one query to the service took a median of
    167 ms (p90 371 ms), a pass of the watch loop (which also relays the
    desk, steers and makes the third start) took up to 8.2 s, and a usage
    report reached the loop's meter 1.07 s after the host recorded it, too
    late for the interrupt to reach a child before its turn ended. So this
    follows the event stream, one `core.events.read` a pass: each
    `execution.usage.observed` is a run's reported total, and each runtime
    change says whether it still runs. A stop decided here is sent from
    here.
    """

    def __init__(self, service, meters, root, record):
        super().__init__(name='codex-meter', daemon=True)
        self.service, self.meters, self.root, self.record = service, meters, root, record
        self.halt = threading.Event()
        self.error = None
        self.passes = 0
        self.seconds = 0.0
        # Every run's own host events, followed in the order the host wrote
        # them: for results the lead's tool never sees (review of L3, round
        # 2, SB-1), each step's size (round 3, SPEND-4) and every thread a
        # run did not start (round 3, SPEND-2).

    def briefs(self):
        """Each run's brief, by which the journal names its invocation: the
        lead's, the plan's children's, and any other the lead's tool
        started."""
        briefs = {LEAD: BRIEF, **{f'{LEAD}.{c}': child_brief(f) for c, f in CHILDREN.items()}}
        briefs.update({f"{LEAD}.{e['arguments'].get('name')}": e['arguments'].get('brief', '')
                       for e in tool_log(self.root) if e.get('event') == 'tool_call'
                       and e.get('tool') == 'start_run'})
        return briefs

    def watch_host_events(self):
        briefs = None
        for identity, gauge in list(self.meters.items()):
            if gauge.host_path is None:
                briefs = briefs if briefs is not None else self.briefs()
                gauge.host_path = host_events_file(self.service, gauge.invocation,
                                                   briefs.get(identity))
            if gauge.host_path is None or not gauge.host_path.exists():
                continue
            with open(gauge.host_path, 'rb') as handle:
                handle.seek(gauge.host_offset)
                data = handle.read()
            whole = data[:data.rfind(b'\n') + 1]
            gauge.host_offset += len(whole)
            for line in whole.splitlines():
                gauge.host_event(json.loads(line))

    def run(self):
        began = time.monotonic()
        try:
            owner = self.service.owner()
        except Exception as caught:
            self.error = redact(repr(caught))[:500]
            return
        cursor = None
        try:
            while not self.halt.is_set():
                started = time.time()
                while True:
                    payload = {'limit': 1000, 'kinds': ['execution.execution']}
                    payload.update({'cursor': cursor} if cursor else {'from': 'start'})
                    result = owner.query('core.events.read', payload).get('result')
                    if result is None:
                        raise RuntimeError('core.events.read refused')
                    gap = False
                    for item in result['items']:
                        if 'gap' in item:
                            gap = True
                            continue
                        subject = item['event']['subject']['id']
                        adopt(self.meters, subject, self.record)
                        gauge = self.meters.get(subject)
                        if gauge is not None:
                            gauge.observe(item['event'])
                    cursor = result.get('next_cursor') or cursor
                    if gap:
                        # Events the stream no longer holds: read each run
                        # itself rather than trust a total with a hole in it.
                        for identity, gauge in list(self.meters.items()):
                            current = view(owner, identity) or {}
                            seen = (current.get('usage') or {}).get('observations') or []
                            if seen and isinstance(seen[0].get('amount'), int):
                                gauge.total = max(gauge.total, seen[0]['amount'])
                            gauge.runtime = current.get('runtime') or gauge.runtime
                    if len(result['items']) < payload['limit']:
                        break
                self.passes += 1
                if self.record.get('mutant') == 'meter-dies' and self.record.get('third_started'):
                    # The meter itself fails, as the third start begins
                    # (review of L3, round 3, SPEND-5).
                    raise RuntimeError('meter-dies: the meter failed mid-run')
                self.watch_host_events()
                codex_meter(owner, self.meters, self.root, self.record)
                # What the lead's tool reads before it hands back a result.
                lead = self.meters[LEAD]
                meter_file = self.root / 'lead-meter.json'
                temporary = meter_file.with_name(meter_file.name + '.tmp')
                temporary.write_text(json.dumps(dict(
                    total=lead.total, reports=lead.reports, hold_above=HOLD_ABOVE,
                    pass_started=started)))
                os.replace(temporary, meter_file)
                self.halt.wait(0.1)
        except Exception as caught:
            self.error = redact(repr(caught))[:500]
            # Never run unmetered, whatever else is going on (review of L3,
            # round 3, SPEND-5): the lead's tool holds from now, and every
            # run still going is cancelled here, on a connection of its own,
            # not when the watch loop next looks.
            self.stop_everything()
        finally:
            self.seconds = round(time.monotonic() - began, 1)
            owner.close()

    def stop_everything(self):
        died = dict(at=now(), error=self.error, cancelled={})
        with contextlib.suppress(OSError):
            (self.root / 'lead-stop').touch()
        try:
            owner = self.service.owner()
            try:
                for identity in list(self.meters):
                    current = view(owner, identity) or {}
                    if current.get('admission') == 'admitted' \
                            and current.get('runtime') != 'exited':
                        died['cancelled'][identity] = cancel(owner, identity, 'meter-died')
            finally:
                owner.close()
        except Exception as caught:
            died['cancel_error'] = redact(repr(caught))[:500]
        died['done_at'] = now()
        self.record['meter_died'] = died
        print(f"METER DIED: {self.error}; cancelled {sorted(died['cancelled'])}", flush=True)


def tool_log(root):
    path = root / 'lead-tool.jsonl'
    if not path.exists():
        return []
    return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]


def started_runs(root):
    """The runs the lead's tool reported started, from its own log."""
    return {f"{LEAD}.{e['arguments'].get('name')}" for e in tool_log(root)
            if e.get('event') == 'tool_call' and e.get('tool') == 'start_run'
            and (e.get('result') or {}).get('started')}


class ToolInstance:
    """The lead tool, run by the runner in the model's place for one call.

    The third start goes through the same tool the lead holds, so a tool that
    reports a refused admission as a start fails here as it would for the
    lead. It is made while the lead is still running: an initiator that has
    exited starts nothing, whatever its budget.
    """

    def __init__(self, spec, log):
        env = dict({v['name']: v['value'] for v in spec['env']}, PIO_LEAD_LOG=str(log))
        self.child = subprocess.Popen([spec['command'], *spec['args']], env=env,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      text=True, bufsize=1, start_new_session=True)
        self.next = 0
        self.call('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                 'clientInfo': {'name': 'lead_run', 'version': '1'}})

    def call(self, method, params):
        self.next += 1
        self.child.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': self.next,
                                           'method': method, 'params': params}) + '\n')
        self.child.stdin.flush()
        while True:
            line = self.child.stdout.readline()
            if not line:
                raise EOFError('the lead tool closed its output')
            answer = json.loads(line)
            if answer.get('id') == self.next:
                return answer

    def tool(self, tool_name, **arguments):
        result = self.call('tools/call', {'name': tool_name, 'arguments': arguments})['result']
        text = result['content'][0]['text']
        return {'error': text} if result.get('isError') else json.loads(text)

    def close(self):
        self.child.stdin.close()
        self.child.wait(timeout=10)


class Desk:
    """Every pending approval, put in front of the person who decides.

    Live, a request is written to `desk/pending-<action>.json`, and the answer
    is `desk/answer-<action>.json` — the owner's decision, relayed by the
    builder with their words. A rehearsal answers for itself and says so.
    Only `allow` and `deny` are encodable: the host takes the single-use
    option by kind, and an `*_always` option is never selected.

    **The desk never blocks the run and never gives up on it.** It is asked
    on every pass of the watch. A request nobody has answered is left alone:
    the host's single-use reject lands when the delivery timeout runs out,
    counted from the moment the request reached it, and the desk records the
    request as lapsed. The run goes on, and the receipt is written.
    """

    def __init__(self, directory, rehearse, silent=False):
        self.directory = private(directory)
        self.rehearse = rehearse
        self.silent = silent
        self.items = {}

    def poll(self, owner, views):
        stream = None
        for identity, current in views.items():
            for action in (current or {}).get('actions', []):
                action_id = action['action_id']
                item = self.items.get(action_id)
                if item is None and action['state'] == 'pending':
                    stream = stream if stream is not None else events(owner)
                    approval = next((e['payload'].get(APPROVAL) for e in stream
                                     if e['subject']['id'] == identity
                                     and e['type'] == 'execution.runtime.changed'
                                     and e['payload'].get('action_id') == action_id), None)
                    item = dict(run=identity, action_id=action_id,
                                requested_at=action['requested_at'], relayed_at=now(),
                                approval=approval, state='waiting')
                    self.items[action_id] = item
                    pending = self.directory / f'pending-{action_id}.json'
                    # Whole or not at all: a relay reading the directory
                    # never finds it half written (review of L3, F8). The
                    # temporary name does not end in `.json`.
                    temporary = pending.with_name(pending.name + '.tmp')
                    temporary.write_text(json.dumps(scrub(item), indent=2) + '\n')
                    os.replace(temporary, pending)
                    print(f'DESK pending {identity} {action_id} {pending}', flush=True)
                if item is None or item['state'] != 'waiting':
                    continue
                if action['state'] != 'pending':
                    # Settled without an answer from here: the host's default.
                    item.update(state='lapsed', settled_as=action['state'],
                                settled_at=action.get('answered_at'))
                    print(f'DESK lapsed {identity} {action_id}', flush=True)
                    continue
                answer = self.answer(action_id)
                if answer is None:
                    continue
                if answer.get('decision') not in ('allow', 'deny'):
                    # Never sent, and never the end of the run: set aside so
                    # a corrected answer can be written in its place.
                    item.setdefault('refused_answers', []).append(answer)
                    source = self.directory / f'answer-{action_id}.json'
                    if source.exists():
                        source.rename(self.directory / f'answer-{action_id}.refused-'
                                      f"{len(item['refused_answers'])}.json")
                    print(f"DESK refused answer {action_id} {answer.get('decision')!r}: "
                          'only allow and deny are single-use', flush=True)
                    continue
                sent = respond(owner, identity, action_id, answer['decision'],
                               (view(owner, identity) or {}).get('revision', 0))
                if 'error' in sent and sent['error']['data'].get('code') == 'conflict':
                    continue
                item.update(answer=answer, state='answered', answered_by_desk_at=now(),
                            sent=sent.get('result', {}).get('outcome', {}).get('state')
                            or sent.get('error', {}).get('data'))
                print(f"DESK answered {action_id} {answer['decision']} "
                      f"by {answer.get('decided_by')}", flush=True)

    def answer(self, action_id):
        if self.rehearse:
            if self.silent:
                return None
            return dict(decision='allow', decided_by='rehearsal',
                        words='rehearsal: the runner answers; no owner is asked')
        path = self.directory / f'answer-{action_id}.json'
        if not path.exists():
            return None
        try:
            return json.loads(path.read_text())
        except ValueError as error:
            return dict(decision=None, unreadable=str(error))


class NoSleep:
    """A no-sleep assertion held for the whole run.

    macOS stops its monotonic clock while asleep, so every deadline here would
    stop with it, and a reviewer's gate run slept on battery (review 44).
    `-i` holds against idle sleep on battery too; `-s` only on AC power.
    """

    def __init__(self, required):
        self.process = None
        tool = shutil.which('caffeinate')
        if tool is None:
            if required:
                raise SystemExit('refusing to run live: nothing here can hold the '
                                 'machine awake (no caffeinate)')
            self.record = dict(held=False, reason='no caffeinate on this platform')
            return
        self.process = subprocess.Popen([tool, '-i', '-s', '-w', str(os.getpid())])
        power = subprocess.run(['pmset', '-g', 'ps'], capture_output=True,
                               text=True).stdout.splitlines()
        self.record = dict(held=True, command=f'caffeinate -i -s -w {os.getpid()}',
                           power=power[0] if power else None)

    def close(self):
        if self.process is None:
            return
        self.record['held_to_the_end'] = self.process.poll() is None
        self.process.terminate()
        self.process.wait(timeout=10)


def mutated_tool(root, mutant='reports-refused-as-started'):
    """The lead tool with a bug put back: the one the first rehearsal caught
    (a refused admission reported as a start), or, for `tool-error-ungated`,
    an error handed back without passing the gate (review of L3, round 2,
    SB-1)."""
    text = TOOL.read_text()
    if mutant == 'tool-error-ungated':
        fix = "        held, seen = withheld(arrived)\n"
        bug = "        if response.get('isError'):\n            return response\n" + fix
    else:
        fix = "    if outcome.get('admission') == 'refused':\n"
        bug = "    if False:\n"
    assert text.count(fix) == 1, 'the mutant no longer matches the tool'
    path = root / 'lead_tool_mutant.py'
    path.write_text(text.replace(fix, bug))
    return path


def scenario(mutant):
    """What the labeled fake plays: the calls a model would make, and the
    answers a model would give, neither of them a model."""
    if mutant == 'setup-fails':
        raise RuntimeError('setup-fails: the scenario could not be built')
    # `child-renamed`: the lead starts and reads its children under names
    # the plan does not have (review of L3, round 3, SPEND-1).
    renamed = mutant in ('child-renamed', 'child-renamed-unchecked')
    called = {short: f'{short}_run' if renamed else short for short in CHILDREN}
    reads = [dict(tool='read_run', arguments=dict(name=called[short]), until='exited',
                  report_as=name) for short, name in CHILDREN.items()]
    starts = [dict(tool='start_run', arguments=dict(name=called[short], brief=child_brief(name)))
              for short, name in CHILDREN.items()]
    loops = ([dict(tool='read_run', arguments=dict(name='alpha'), repeat=CALL_CEILING * 2)]
             if mutant == 'lead-loops' else [])
    if HARNESS == 'codex':
        # The lead's plain calls (two starts, two reads) each nearly a whole
        # step in flight in these plays (LEAD_PAST_HOLD_STEP): enough more
        # reads that the one after them comes back with the lead's reported
        # total past its hold, and still under its ceiling, whatever the
        # sizing (one more at the hold of 120,000, none at 95,000).
        past = [dict(tool='read_run', arguments=dict(name='alpha'))] * (
            HOLD_ABOVE // LEAD_PAST_HOLD_STEP - len(starts + reads) + 1)
        extra = {
            'lead-heavy': past + [dict(tool='read_run', arguments=dict(name='alpha'))],
            # An argument read_run does not take: the tool raises TypeError.
            'lead-tool-error-past-hold': past + [dict(tool='read_run',
                                                      arguments=dict(name='alpha', wait=True))],
            'tool-error-ungated': past + [dict(tool='read_run',
                                               arguments=dict(name='alpha', wait=True))],
            'lead-shell-past-hold': past + [dict(tool='!shell', command='wc -l alpha.md')],
            'meter-ignores-items': past + [dict(tool='!shell', command='wc -l alpha.md')],
        }.get(mutant, [])
        return codex_scenario(mutant, starts + reads + loops + extra)
    return {
        **dict(
            lead=dict(calls=starts + reads + loops,
                      relay_offset=1 if mutant == 'wrong-relay' else 0),
            answer_line_counts=True, led_offset=1 if mutant == 'wrong-child' else 0,
            # Long enough to be steered while it runs, and asked about one
            # thing, so the desk has something to relay.
            led_delay_ms=3000, permission_request=ASKS, ask_in='led',
            usage_total=4096),
        # A plan's own play replaces those it names.
        **plan_scenario(mutant)}


def codex_scenario(mutant, calls):
    """What the labeled Codex fake plays for L3. The lead's calls go through
    its MCP server; each child runs the one command its brief quotes, then
    counts. `alpha` sleeps thirty seconds, so the lead's read waits and the
    steer lands on a running turn; `beta` asks a command approval, so the
    desk has something to relay (live, `on-request` in a writable sandbox
    need not ask at all). The fake answers the model and provider it was
    asked for on `openai`, unless a mutant says otherwise."""
    alpha, beta = child_brief(CHILDREN['alpha']), child_brief(CHILDREN['beta'])
    play = dict(
        lead=dict(calls=calls, relay_offset=1 if mutant == 'wrong-relay' else 0),
        answer_line_counts=True, led_offset=1 if mutant == 'wrong-child' else 0,
        led_delay_ms=1000 if mutant == 'no-wait' else 30_000, led_delay_if=alpha,
        command_approval_if=beta, model_provider=CODEX['model_provider'], usage_step=4096)
    if mutant == 'wrong-model':
        play['model_reported'] = 'another-model'
    if mutant == 'reviewer-elsewhere':
        play['approvals_reviewer'] = 'auto_review'
    if mutant in ('child-overspends', 'ceiling-cancel-never-sent', 'stop-charged-reported',
                  'child-renamed', 'child-renamed-unchecked', 'stopped-past-share',
                  'stopped-charge-capped', 'meter-dies'):
        # alpha runs its command once more than its ceiling holds in steps,
        # each a 30,000-token step, the most the bound allows in flight
        # (review of L3, round 3, SPEND-4): three times at a ceiling of
        # 60,000. Codex reports each once its command has finished, so the
        # report that reaches the ceiling (60,000) arrives with the next step
        # in flight, and the runner stops alpha then; a child's model step
        # here takes six seconds, longer than the stop takes to reach it
        # (about 1.5 s against this service), as a real step does.
        play.update(led_step_if=alpha, led_step=CODEX['in_flight'], led_commands_if=alpha,
                    led_commands=CHILD_CEILING // CODEX['in_flight'] + 1, led_step_ms=6000)
    if mutant in ('stopped-past-share', 'stopped-charge-capped'):
        # And ignores the stop: it runs its last command and answers, both
        # reported (120,000 in all), and exits by itself (SPEND-7).
        play['led_ignores_interrupt_if'] = alpha
    if mutant == 'step-past-in-flight':
        # One step of 40,000: under the ceiling, past the in-flight bound.
        play.update(led_heavy_if=alpha, led_heavy_step=40_000, led_step_ms=6000)
    if mutant in ('probe-over-share', 'probe-charged-flat'):
        # The third run the runner's probe starts, admitted, takes one step
        # of 150,000, past its share (review of L3, round 4, SPEND-12).
        play.update(led_heavy_if=THIRD_BRIEF, led_heavy_step=150_000)
    if mutant in ('late-step-after-halt', 'stale-meter-charged'):
        # The same step, reported only once the runner is on its way out.
        play.update(led_heavy_if=alpha, led_heavy_step=40_000)
    if mutant in ('memory-pipeline-ran', 'overrides-on'):
        play['memory_pipeline'] = True
    if mutant == 'overrides-on':
        # And two stream retries on beta, which the receipt counts.
        play.update(stream_retries_if=beta, stream_retries=2)
    if mutant in ('goal-continued', 'continuation-uncharged', 'overrides-on'):
        # beta goes on by itself once its turn has ended, as a goal's
        # continuation does (review of L3, round 4, SPEND-9).
        play['continue_after_turn_if'] = beta
    if mutant in ('subagent-spawned', 'subagent-not-stopped', 'subagent-uncounted',
                  'overrides-on'):
        # alpha's first step also spawns a sub-agent, as 0.157.0 does, which
        # takes three steps of its own unless stopped.
        play.update(spawn_agent_if=alpha, subagent_steps=3, subagent_step_ms=1500)
    if mutant in ('stop-ignored', 'not-exited-charged-share'):
        # alpha's first step alone is past its ceiling, and by more than its
        # share less a step (100,000 against 60,000 and 120,000); it
        # acknowledges the interrupt and goes on, answering two and a half
        # minutes later, so it is still running when its usage is read.
        play.update(led_heavy_if=alpha, led_heavy_step=CHILD_SHARE - CODEX['in_flight'] + 10_000,
                    led_ignores_interrupt_if=alpha, led_answer_ms=150_000)
    if mutant == 'lead-asked-in-openai-form':
        # The lead's tool approval in a mode the 0.157.0 schema allows and
        # PIO does not recognise, asked despite the pre-allowance, and only
        # before `read_run`: both children are started by then, so nothing
        # but the ask itself can fail the pre-allowance row.
        play.update(lead_asks_in_mode='openai/form', lead_asks_for='read_run')
    if mutant == 'lead-asked-by-user-input':
        # The same ask by Codex's other route (review of L3, round 2, HR-2).
        play.update(lead_asks_by='requestUserInput', lead_asks_for='read_run')
    if mutant == 'child-asks-permissions':
        play['led_permissions_if'] = beta
    if mutant in ('lead-heavy', 'lead-tool-error-past-hold', 'tool-error-ungated',
                  'lead-shell-past-hold', 'meter-ignores-items'):
        play['lead_usage_step'] = LEAD_PAST_HOLD_STEP
    if mutant in ('usage-suppressed', 'asked-silent'):
        # No run reports anything, and alpha works past the silence bound,
        # so the lead waiting on it and alpha itself must be stopped.
        play.update(usage_suppressed=True, led_delay_ms=(USAGE_SILENCE + 30) * 1000)
    if mutant == 'asked-silent':
        # Both children ask first: alpha's silence counts only once its
        # answer has put it back to work.
        play['command_approval_if'] = 'Run the shell command'
    return play


def plan_scenario(mutant):
    """L1b: the waiting child works for thirty seconds and the asking child
    asks to read its `.env` file; each only where its brief says so. The
    shape of the fake's read request is OpenCode's read tool's own parameter,
    not a measured permission request."""
    if not PLAN['waits']:
        return {}
    waits, asks = CHILDREN[PLAN['waits']], CHILDREN[PLAN['asks']]
    return dict(
        # `alpha-outlasts`: past the tool's 55 s wait, and not so far past
        # that a second read waits another 20 s.
        led_delay_ms={'no-wait': 1000, 'alpha-outlasts': 62_000}.get(mutant, 30_000),
        led_delay_if=child_brief(waits),
        # `no-ask`: text no prompt contains, so nobody asks.
        ask_if='no prompt names this' if mutant == 'no-ask' else asks,
        permission_request=dict(title=f'read {asks}', kind='read',
                                input=dict(filePath=asks)))


def on_sigterm(signum, frame):
    raise SystemExit('SIGTERM')


def run(args):
    rehearse = args.rehearse
    started_at = now()
    record = dict(format='pio-lead-run/2', mode='rehearsal' if rehearse else 'live',
                  harness=dict(name=HARNESS, **(CODEX['record'] if HARNESS == 'codex' else {})),
                  lead=LEAD, model=MODEL, budget=BUDGET, sequence=SEQUENCE,
                  sequence_cap=SEQUENCE_CAP, sequence_stop=SEQUENCE_STOP,
                  approvals=APPROVALS, bound=BOUND, mutant=args.mutant,
                  started_at=started_at, desk=args.desk,
                  desk_answered_by='answer files, as live (--relay)' if args.relay
                  else 'the rehearsal itself' if args.rehearse else 'answer files')
    head = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'],
                          capture_output=True, text=True).stdout.strip()
    dirty = bool(subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'],
                                capture_output=True, text=True).stdout.strip())
    record.update(commit=head, dirty=dirty)
    names = {LEAD: f'{args.attempt}/{LEAD}' if args.attempt else LEAD}
    names.update({f'{LEAD}.{c}': f'{names[LEAD]}.{c}' for c in CHILDREN})
    if rehearse:
        record['preflight'] = {'checked': False, 'reason': 'rehearsal'}
        root = Path(tempfile.mkdtemp(prefix='pio-l1-', dir='/tmp')).resolve()
        os.chmod(root, 0o700)
        book_path = args.receipt.with_name(args.receipt.stem + '-ledger.json')
        book_path.unlink(missing_ok=True)
    else:
        # A clean tree, a binary built from it, and its digest. The build is
        # the runner's own, so "built from HEAD" is not an inference.
        subprocess.run(['cargo', 'build', '--locked', '--workspace'], cwd=ROOT, check=True)
        record['preflight'] = opencode_live_run.preflight(False, root=ROOT, binary=BINARY)
        book_path = live.ledger_path()
        book = read_ledger(book_path)
        for key in names.values():
            if key in book['runs']:
                raise SystemExit(f'the ledger already holds {key}; those tokens were '
                                 'spent. Run again under --attempt instead.')
        spent = sequence_charged(book)
        # The stop is checked against what this attempt could spend at worst,
        # not only against what has been spent: nothing checks it mid-run.
        if spent + WORST_CASE > SEQUENCE_STOP:
            raise SystemExit(f'stop: the lead sequence has charged {spent}; one more '
                             f'attempt could spend {WORST_CASE}, past the '
                             f'{SEQUENCE_STOP} stop')
        if live.cumulative(book) + WORST_CASE >= live.STOP_AT:
            raise SystemExit(f"stop: the {PLAN['harness']} cap would reach its stop")
        root = live_tree(LEAD)
    args.root = root
    try:
        return run_in(args, record, rehearse, root, names, book_path, started_at)
    except BaseException:
        if 'exit_path' in record:
            # The exit path wrote the receipt and charged the ledger; the
            # tree holds what explains it (daemon.stderr among it).
            raise
        # Nothing was reserved and nothing started, so the tree holds nothing
        # worth keeping. A rehearsal that failed here left its tree in /tmp,
        # and the leak gate found it (L1b's first rehearsal).
        with contextlib.suppress(Exception):
            case_cleanup.release(root)
        raise


def run_in(args, record, rehearse, root, names, book_path, started_at):
    """The run, in a tree that exists: everything from the probe onward."""
    # The cleanup must be able to release the live tree, and that is checked
    # before anything is reserved or started, not found out at the end. A
    # rehearsal checks a probe made the same way, then releases it.
    probe = live_tree('probe') if rehearse else root
    releasable, why = case_cleanup.releasable(probe)
    record['live_tree'] = dict(releasable=releasable, why=why or None,
                               probe=rehearse)
    if rehearse:
        try:
            case_cleanup.release(probe)
        except AssertionError as refused:
            record['live_tree']['release_error'] = redact(str(refused))[:500]
            with contextlib.suppress(OSError):
                probe.rmdir()  # empty, and made above; never anything else
        record['live_tree']['removed'] = not probe.exists()
    elif not releasable:
        raise SystemExit(f'refusing to start: the cleanup could not release {root}: {why}')
    record['root'] = str(root)
    print(f'ROOT {root}', flush=True)
    awake = NoSleep(required=not rehearse)
    record['no_sleep'] = awake.record
    rows = Rows(rehearse)
    # Sub-agents off per launch: the owner's recorded decision, live, or the
    # labeled fake's own token (review of L3, round 3, SPEND-2).
    # Codex's unmetered features off per launch (review of L3, round 3,
    # SPEND-2; round 4, U1): the plan's own recorded owner decision, L3's
    # alone (owner decision, 2026-09-26), live and rehearsed alike. The
    # labeled fake's own token for the mutants that model the override
    # itself; none for those that play a Codex whose features are on, or
    # that apply the live check to a home without the decision.
    features_off = FEATURES_OFF_REHEARSAL if args.mutant in OVERRIDE_MUTANTS \
        else None if args.mutant in (*UNMETERED_REFUSED, *FEATURES_ON_MUTANTS) \
        else PLAN.get('features_off_decision')
    service = Service(root, rehearse, scenario(args.mutant), args.mutant,
                      features_off=features_off)
    if HARNESS == 'opencode':
        config_dir = Path(service.config['opencode']['config_dir'])
        if args.mutant == 'helper-elsewhere':
            # The case the check exists for, in the rehearsal's own config.
            (config_dir / 'opencode.jsonc').write_text(
                json.dumps({'small_model': 'another-provider/helper-model'}))
        # Before anything is reserved or started: a helper on another
        # provider would spend there, and nothing in the receipt would show it.
        record['helpers'] = helper_models(config_dir)
        elsewhere = helpers_elsewhere(record['helpers'])
        if elsewhere:
            raise SystemExit('refusing to start: the OpenCode configuration points a helper '
                             f"at a provider other than the plan's ({MODEL.split('/', 1)[0]}): "
                             f'{sorted(elsewhere)}')
    if HARNESS == 'codex':
        codex_home = Path(service.config['codex']['codex_home'])
        planted = {
            # The case the check exists for, in the rehearsal's own Codex home.
            'experimental-feature-on': '[features]\nexec_permission_approvals = true\n',
            # The same feature by Codex's legacy alias, which, sorted after the
            # canonical key, overrides it (review of L3, round 3, SPEND-3).
            'experimental-alias-on': '[features]\nexec_permission_approvals = false\n'
                                     'request_permissions = true\n',
            **UNMETERED_PLANTED}.get(args.mutant)
        if planted:
            codex_home.mkdir(parents=True, exist_ok=True)
            (codex_home / 'config.toml').write_text(planted)
        # Before anything is reserved or started. Only these keys are read.
        record['codex_features'] = codex_features(codex_home)
        features = record['codex_features']
        # A key Codex would not read as a switch is not "off": Codex would
        # refuse the file, and the runner cannot tell what it meant.
        widening = [f"{k} (by {features[f'{k}_key']})" for k in WIDENING_FEATURES
                    if features[k] is True
                    or (features[k] is None and features[f'{k}_key'] is not None)]
        if widening:
            raise SystemExit('refusing to start: the Codex configuration turns on '
                             f'{widening}: a command approval could then ask for permissions '
                             'PIO is never shown, and an allow would grant them')
        # Codex's unmetered, default-on features (review of L3, round 3,
        # SPEND-2; round 4, U1): no plan has any of them, and their spend is
        # outside every share. Live, each must resolve off, from the owner's
        # own configuration or from the per-launch override under a recorded
        # owner decision, laid over it as Codex lays it; otherwise the run is
        # refused here, naming each feature and the key that decided it. A
        # rehearsal records what it read, and its mutants apply the live check.
        override = dict(FEATURES_OFF) if features_off else None
        if override and args.mutant == 'override-misses-alias':
            # A runner whose override forgot memories' legacy alias: the
            # owner's `memory_tool = true` still decides (sorted last).
            override.pop('features.memory_tool')
        record['unmetered'] = unmetered_features(codex_home, override, features_off)
        not_off = features_not_off(record['unmetered'])
        live_check = not rehearse or args.mutant in UNMETERED_REFUSED
        if live_check and not_off:
            raise SystemExit(
                "refusing to start: Codex's unmetered, default-on features are not all off for "
                "L3's threads: " + '; '.join(f'{f} ({why})' for f, why in not_off.items())
                + ". Each must be off, by the owner's own config.toml or per launch under a "
                "recorded owner decision the plan sends (pio-protocol FEATURES_OFF_DECISIONS); "
                'nothing is started or reserved.')
    # `--relay`: a rehearsal whose desk waits for answer files, as a live
    # one does, so the relay that will run beside the live run is rehearsed
    # against the requests this code actually writes.
    desk = Desk(root / 'desk', rehearse and not args.relay,
                silent=args.mutant == 'desk-silent')
    gauge = CodexMeter if HARNESS == 'codex' else Meter
    meters = {LEAD: gauge(LEAD, LEAD_CEILING),
              **{f'{LEAD}.{c}': gauge(f'{LEAD}.{c}', CHILD_CEILING) for c in CHILDREN}}
    # Each run's worst case is reserved in the ledger immediately before the
    # lead's submit, the first thing that can spend, and not before the
    # service starts: starting it qualifies Codex and spends nothing, and a
    # service that never became ready must not leave every share reserved
    # and the sequence blocked with nothing run (review of L3, F4).
    state = dict(submitted=set(), grant_id=None, spec=None, repo=None,
                 reserve=lambda: reserve(book_path, names, started_at))
    previous = signal.signal(signal.SIGTERM, on_sigterm)
    error = None
    # From here every exit writes the receipt and charges the ledger.
    record['exit_path'] = True
    try:
        watch(args, record, service, desk, meters, state)
    except BaseException as caught:  # SystemExit and KeyboardInterrupt too
        error = caught
        record['error'] = dict(type=type(caught).__name__,
                               message=redact(str(caught))[:2000], at=now())
    finally:
        # Nothing below may be cut short: a second interrupt here would leave
        # tokens spent with no ledger line.
        signal.signal(signal.SIGINT, signal.SIG_IGN)
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        try:
            observed = settle(record, service, meters, state, root)
        except Exception as caught:
            observed = None
            record['settle_error'] = redact(repr(caught))[:2000]
        if 'usage' not in record:
            # The service could not be asked: every run that was submitted
            # or reported started is charged its bound, never nothing.
            record['usage'] = usage_of({}, meters, state['submitted'] | started_runs(root))
        record['desk_items'] = list(desk.items.values())
        try:
            if observed is not None:
                judge(rows, record, observed, desk, meters, state, rehearse)
        except Exception as caught:
            record['judge_error'] = redact(repr(caught))[:2000]
        if error is not None:
            rows.add('The run finished without an error', record['error'], None)
        rows.add('The live tree can be released', record['live_tree'],
                 'releasable, and a probe removed',
                 holds=lambda t: t['releasable'] and t.get('removed', True)
                 and 'release_error' not in t)
        # A step of the runner's own that raised left its rows unwritten, and
        # a row that was never written never fails (L1 live, review 46).
        rows.add("The runner's own steps raised no error",
                 {k: record[k] for k in ('settle_error', 'judge_error', 'meter_error')
                  if k in record}, {})
        if HARNESS == 'codex' and 'memory_before' in record:
            # Every run is over and its app-server gone, so Codex's memory
            # pipeline, which runs inside it, has stopped too (review of L3,
            # round 3, S6).
            record['memory_after'] = memory_state(service.config['codex']['codex_home'])
            rows.add("Codex's memory pipeline wrote nothing during the run",
                     dict(changes=memory_changes(record['memory_before'], record['memory_after']),
                          memories=((record.get('unmetered') or {}).get('features') or {})
                          .get('memories')),
                     "no memory file or database added, removed or changed",
                     holds=lambda o: not o['changes'],
                     note="Codex's own background pipeline, which [features] memories starts at "
                          "a root thread's first turn: its files and databases by name (a digest "
                          'for any name Codex did not choose), size and time, never content. A '
                          'change means it ran, and its model calls report nowhere PIO reads and '
                          'are outside every meter')
        try:
            if args.mutant == 'release-refused':
                # The guard refuses every tree, as it refused L1's live one.
                with case_cleanup.no_permitted_prefixes():
                    service.release(remove=rehearse)
            else:
                service.release(remove=rehearse)
        except Exception as caught:
            record['release_error'] = redact(repr(caught))[:2000]
        rows.add('The service was released, and nothing it started survived',
                 record.get('release_error') or 'released', 'released')
        if args.mutant == 'release-refused':
            try:
                service.release(remove=rehearse)
            except Exception as caught:
                record['mutant_release_error'] = redact(repr(caught))[:2000]
        awake.close()
        record['rows'] = rows.rows
        record['failed'] = rows.failed()
        record['charge'] = charge(book_path, names, record.get('usage', {}), started_at,
                                  authoritative=observed is not None,
                                  lead_submitted=LEAD in state['submitted'],
                                  probes=record.get('probes_admitted'))
        record['finished_at'] = now()
        args.receipt.write_text(json.dumps(scrub(record), indent=2, sort_keys=True) + '\n')
        if root.exists():
            (root / 'runner-done').write_text(now() + '\n')
        signal.signal(signal.SIGINT, signal.default_int_handler)
        signal.signal(signal.SIGTERM, previous)
    if error is not None:
        raise error
    return record


def watch(args, record, service, desk, meters, state):
    """Everything the runner asks of the service, in order."""
    rehearse = args.rehearse
    root = service.root
    repo, base = fixture(root)
    state['repo'] = repo
    record['wc_l'] = wc_l(repo)
    # The owner's OpenCode service is asserted untouched on every run, on
    # either harness (the owner's standing rule).
    record['owner_service_before'] = opencode_live_run.owner_service()
    if HARNESS == 'opencode':
        record['sessions_before'] = live.session_listing(repo, rehearse)
        record['configured'] = live.configured_model(rehearse)
    if HARNESS == 'codex':
        record['memory_before'] = memory_state(service.config['codex']['codex_home'])
    service.start()
    if args.mutant in ('qualified-elsewhere', 'qualified-as-committed'):
        # What serve-codex writes for the owner's Codex: the committed record
        # for the pin the binary carries, as it stands, or with its native
        # binary changed.
        pin = re.search(r'PINNED_VERSION: &str = "([^"]+)"',
                        (ROOT / 'crates/pio-codex/src/lib.rs').read_text()).group(1)
        planted = json.loads((QUALIFICATIONS / f'qualification-{pin}.json').read_text())['record']
        if args.mutant == 'qualified-elsewhere':
            planted['resolution']['native']['sha256'] = '0' * 64
        planted['service_node_resolution'] = dict(applicable=True, matches_qualification=True)
        (service.store / 'qualification.json').write_text(json.dumps(planted))
    if HARNESS == 'codex':
        # Before anything is reserved: serve-codex starts on any Codex its
        # qualification passes, and that never compares the wrapper, native
        # binary or Node with the committed record, so a Hermes Node update
        # would pass it and burn the one attempt (review of L3, round 2, V-1).
        record['qualification_check'] = qualification_precheck(service, rehearse)
    owner = service.owner()
    if HARNESS == 'codex':
        state['metering'] = CodexMetering(service, meters, root, record)
        state['metering'].start()

    grant_id = str(uuid.uuid4())
    rights = ['execution.submit', 'execution.steer', 'execution.read',
              'core.events.read']
    if args.mutant == 'grant-may-answer':
        rights.append('execution.respond_action')
    if args.mutant == 'grant-no-steer':
        rights.remove('execution.steer')
    terms = dict(holder='lead', audience=PROVIDER, rights=rights,
                 resources=[dict(kind='execution.execution', id_prefix=f'{LEAD}.')],
                 delegation=dict(allowed=False, max_depth=0))
    issued = owner.call(command('core.grant.issue', dict(kind='core.grant', id=grant_id),
                                terms, command_id=f'grant-{grant_id}'))
    assert 'result' in issued, issued
    record['grant'] = dict(id=grant_id, terms=terms)
    state['grant_id'] = grant_id

    credential_file = root / 'lead.credential'
    credential_file.write_text(service.lead_credential + '\n')
    os.chmod(credential_file, 0o600)
    tool = mutated_tool(root, args.mutant) \
        if args.mutant in ('reports-refused-as-started', 'tool-error-ungated') else TOOL
    spec = dict(name=LEAD_SERVER, command=sys.executable,
                args=[str(tool), '--credential-file', str(credential_file)],
                env=[dict(name='PIO_LEAD_SOCKET', value=str(service.socket)),
                     dict(name='PIO_LEAD_GRANT', value=grant_id),
                     dict(name='PIO_LEAD_ID', value=LEAD),
                     dict(name='PIO_LEAD_WORKSPACE', value=str(repo)),
                     dict(name='PIO_LEAD_BASE', value=base),
                     dict(name='PIO_LEAD_LOG', value=str(root / 'lead-tool.jsonl')),
                     dict(name='PIO_LEAD_CALL_CEILING', value=str(CALL_CEILING)),
                     dict(name='PIO_LEAD_STOP', value=str(root / 'lead-stop')),
                     # The host's own deadline for each child, shortened for
                     # the mutants it interrupts (review of L3, round 3,
                     # SPEND-6).
                     dict(name='PIO_LEAD_DEADLINE', value='20' if args.mutant in (
                         'deadline-interrupted', 'interrupted-charged-reported') else '900'),
                     # Only the plan's children, by name (review of L3,
                     # round 3, SPEND-1). `child-renamed-unchecked` stands
                     # for a tool whose check let other names through: the
                     # runner must meter, stop and charge them regardless.
                     dict(name='PIO_LEAD_CHILDREN', value=','.join(
                         [f'{c}_run' for c in CHILDREN]
                         if args.mutant == 'child-renamed-unchecked' else CHILDREN))])
    if HARNESS == 'codex':
        # What the meter reads, for the tool to read before it hands back a
        # result (review of L3, A7).
        spec['env'].append(dict(name='PIO_LEAD_METER', value=str(root / 'lead-meter.json')))
    if HARNESS == 'codex' and args.mutant != 'no-pre-allow':
        # Owner decision, 2026-09-25: the lead's own two tools are
        # pre-allowed, per launch, in the lead's thread config alone.
        spec['pre_allowed_tools'] = list(PRE_ALLOWED)
    if HARNESS == 'codex':
        # PIO's own server, in the model's own tool list, required, and
        # given thirty seconds to start (LEAD_TOOL_SETTINGS). The mutant
        # `lead-tool-deferred` sends it as L3's first live run did.
        spec.update(LEAD_TOOL_SETTINGS)
        if args.mutant == 'lead-tool-deferred':
            spec.pop('omit_tools_from')
    state['spec'] = spec
    extensions = {CONTENT: dict(media_type='text/plain', text=BRIEF)}
    if args.mutant == 'lead-without-brief':
        extensions = {}
    if args.mutant == 'lead-submit-invalid':
        # Bytes that do not match the brief's digest: the service refuses the
        # envelope, and no execution is made.
        extensions[CONTENT] = dict(media_type='text/plain', text=BRIEF + ' tampered')
    if args.mutant != 'no-tool':
        extensions[LEAD_TOOL] = spec
    # Before anything can spend: each run's worst case, held in the ledger
    # until the exit path replaces it with what the run is charged.
    record['reserved'] = state['reserve']()
    state['submitted'].add(LEAD)
    record['lead_submit'] = submit(owner, LEAD, BRIEF, repo, base,
                                   dict(initiator=dict(kind='execution.execution', id=LEAD),
                                        depth=0, call_budget=BUDGET + (
                                            1 if args.mutant in THIRD_ADMITTED else 0)),
                                   extensions).get('result', {}).get('outcome')

    # Watch until every run is over: relay approvals, meter every run, steer a
    # child while its turn is running, and make the third start while the
    # lead is.
    lead_grant = service.lead(grant_id)
    record['steer_running'], record['third'] = None, None
    metered = 0.0
    deadline = time.monotonic() + EXECUTION_DEADLINE + 120
    discovery = Discovery()
    while time.monotonic() < deadline:
        # Every run under this lead, the plan's and any other the stream has
        # shown (review of L3, round 3, SPEND-1).
        views = {i: view(owner, i) for i in runs(meters)}
        desk.poll(owner, views)
        metering = state.get('metering')
        if metering is not None and not metering.is_alive():
            # Never run unmetered: the exit path stops everything.
            raise SystemExit(f'the meter stopped: {metering.error}')
        if metering is None and time.monotonic() - metered >= 0.5:
            metered = time.monotonic()
            discovery.adopt(owner, meters, record)
            meter(owner, views, meters, root, record)
        first = views[f'{LEAD}.alpha']
        # **Only a turn that is running and has been delivered.** Anything
        # earlier is `not_supported` on every harness, so it would say nothing
        # about OpenCode (review 44).
        if record['steer_running'] is None and first and first['admission'] == 'admitted' \
                and first['runtime'] == 'active' and first.get('delivery') == 'acknowledged':
            record['steer_running'] = steer(lead_grant, f'{LEAD}.alpha')
            if args.mutant in ('late-step-after-halt', 'stale-meter-charged'):
                # alpha's heavy first step is in flight, its command running.
                # The runner is interrupted now: the step is reported only
                # when settle's cancel reaches alpha, after the meter has
                # stopped folding the host's events (review of L3, round 4,
                # SPEND-11).
                time.sleep(5)
                raise KeyboardInterrupt("the runner interrupted with alpha's heavy step in "
                                        'flight')
        both = all(views[f'{LEAD}.{c}'] and views[f'{LEAD}.{c}']['admission'] == 'admitted'
                   for c in CHILDREN)
        lead_view = views[LEAD] or {}
        if args.mutant in ('stop-ignored', 'not-exited-charged-share') and any(
                s['run'] == f'{LEAD}.alpha' for s in record.get('ceiling_stops', [])):
            raise KeyboardInterrupt('the runner interrupted after a stop Codex ignored')
        if record['third'] is None and both and lead_view.get('runtime') != 'exited':
            if args.mutant == 'interrupted':
                raise KeyboardInterrupt('the interrupted mutant, with every run admitted')
            # The runner's own probe of PIO's budget: its instance of the
            # tool may name `third`, so the start reaches the service.
            probe_spec = dict(spec, env=[dict(v, value=f"{v['value']},third")
                                         if v['name'] == 'PIO_LEAD_CHILDREN' else v
                                         for v in spec['env']])
            # On a thread of its own (review of L3, round 3, SPEND-5): the
            # tool may wait up to a minute for the meter and a hold, and the
            # watch loop must not stop looking meanwhile.
            record['third'] = dict(pending=True)
            record['third_started'] = True

            def third_start(spec=probe_spec, service=service):
                instance = ToolInstance(spec, root / 'runner-tool.jsonl')
                caller = service.owner()
                try:
                    result = instance.tool('start_run', name='third', brief=THIRD_BRIEF)
                    record['third'] = dict(result=result,
                                           lead_runtime=(view(caller, LEAD) or {}).get('runtime'))
                except Exception as caught:
                    record['third'] = dict(error=redact(repr(caught))[:500])
                finally:
                    caller.close()
                    with contextlib.suppress(Exception):
                        instance.close()
            state['third_thread'] = threading.Thread(target=third_start, name='third-start',
                                                     daemon=True)
            state['third_thread'].start()
        over = lambda v: not v or v.get('admission') == 'refused' or v.get('runtime') == 'exited'
        if all(over(views[i]) for i in views):
            break
        time.sleep(0.25)

    if state.get('third_thread') is not None:
        # The probe's answer, before its row is judged: at most the tool's
        # meter wait and hold.
        state['third_thread'].join(timeout=METER_WAIT_AND_HOLD + 10)
    record['steer_exited'] = steer(lead_grant, f'{LEAD}.alpha')
    record['lead_answer'] = respond(lead_grant, f'{LEAD}.alpha', f'{LEAD}.alpha.action-1',
                                    'allow',
                                    (view(owner, f'{LEAD}.alpha') or {}).get('revision', 0))
    record['lead_reads_itself'] = lead_grant.query('execution.inspect', {'execution': LEAD})
    # Attaching a tool is the owner's act: under the lead's grant it is
    # refused before anything else about the submit is looked at.
    record['grant_attach'] = submit(
        lead_grant, f'{LEAD}.attached', ATTACHED_BRIEF, repo, base,
        dict(initiator=dict(kind='execution.execution', id=LEAD), depth=1, call_budget=0),
        {CONTENT: dict(media_type='text/plain', text=ATTACHED_BRIEF), LEAD_TOOL: spec})
    lead_grant.close()
    # And a spec that carries a credential value is refused at admission,
    # because the spec is journaled. A credential-shaped dummy, never the
    # lead's own: a refused submit is journaled too.
    leaky = dict(spec, env=[*spec['env'], dict(name='PIO_LEAD_NOTE',
                                               value='ccred1.lead.' + 'x' * 43)])
    brief_bytes = CREDENTIAL_BRIEF
    record['credential_check'] = submit(
        owner, 'credential-check', brief_bytes, repo, base, None,
        {CONTENT: dict(media_type='text/plain', text=brief_bytes), LEAD_TOOL: leaky})
    owner.close()


# Items that are the model's own output, not a tool's result.
MODEL_ITEMS = ('agentMessage', 'reasoning', 'userMessage', 'plan')


def untooled(event):
    """A completed item on the lead's thread whose result the lead's tool
    did not hand back: anything but the model's own output and a completed
    call to the lead's own server."""
    if event.get('kind') != 'item_completed' or event.get('item_type') in MODEL_ITEMS:
        return False
    return not (event.get('item_type') == 'mcpToolCall' and event.get('server') == LEAD_SERVER
                and event.get('status') == 'completed')


def host_events_file(service, invocation, brief):
    """A run's own host events file: by its invocation, where the stream has
    named it, else the one whose launch carries the run's brief digest, as
    host_events finds it. None until there is one."""
    if invocation is None and brief is not None:
        journal = service.store / 'journal.sqlite3'
        if not journal.exists():
            return None
        digest = 'sha256:' + sha(brief)
        with contextlib.closing(sqlite3.connect(f'file:{journal}?mode=ro', uri=True)) as db:
            states = [json.loads(r[0]) for r in db.execute('select state from invocations')]
        invocation = next((s['invocation_id'] for s in states
                           if (s.get('payload') or {}).get('brief', {}).get('digest') == digest),
                          None)
    return service.store / f'{HARNESS}-{invocation}.events.jsonl' if invocation else None


def host_steps(events):
    """What a run's own host events say of its spend, worked out afresh from
    the file: the sum of each thread's last total, the largest model step
    (a report's last step, or the rise between two reports on one thread),
    how many reports, every thread it did not start, and whether its own
    turn ended interrupted."""
    threads, largest, steps, others, interrupted, continued = {}, 0, 0, [], False, []
    for event in events:
        if event['kind'] == 'usage':
            thread = event.get('thread_id') or 'own'
            total = (event.get('total') or {}).get('totalTokens')
            last = (event.get('last') or {}).get('totalTokens')
            if isinstance(total, int):
                largest = max(largest, total - threads.get(thread, 0))
                threads[thread] = max(threads.get(thread, 0), total)
                steps += 1
            if isinstance(last, int):
                largest = max(largest, last)
        elif event['kind'] == 'other_thread' and event.get('thread_id') not in others:
            others.append(event.get('thread_id'))
        elif event['kind'] == 'turn_completed' and not event.get('continuation'):
            interrupted = event.get('status') == 'interrupted'
        elif event['kind'] == 'continuation_started':
            continued.append(event.get('turn_id'))
    return dict(sum=sum(threads.values()), largest=largest, steps=steps, others=others,
                interrupted=interrupted, continued=continued)


OTHER_THREADS = 'pio.combraton.dev/other-threads'
CONTINUATIONS = 'pio.combraton.dev/continuations'


def other_threads_carried(stream):
    """Each run's list of threads it did not start, as its exit carried it;
    None for a run whose exit carried none."""
    return {e['subject']['id']: (e.get('payload') or {}).get(OTHER_THREADS)
            for e in stream if e['type'] == 'execution.exit.observed'}


def u_ran(entry):
    return entry['basis'] not in ('refused before any model call', NO_SUCH_EXECUTION)


def lead_calls(root):
    return len([e for e in tool_log(root) if e.get('event') == 'request'
                and e.get('method') == 'tools/call'])


def lead_withheld(root):
    """Whether the lead's tool has withheld a result it had ready."""
    return any(e.get('event') == 'held' and e.get('result_withheld') for e in tool_log(root))


def meter(owner, views, meters, root, record):
    """OpenCode: read every live run's meter from its session, and stop a run
    that passes its ceiling."""
    calls = lead_calls(root)
    for identity, gauge in list(meters.items()):
        current = views.get(identity)
        if not current or current.get('admission') == 'refused':
            continue
        gauge.read(owner)
        if gauge.stopped or current.get('runtime') == 'exited':
            continue
        over = []
        if identity == LEAD and max(len(gauge.calls), calls) > CALL_CEILING:
            over.append(f'{max(len(gauge.calls), calls)} tool calls, past {CALL_CEILING}')
        if gauge.estimate() >= gauge.ceiling:
            over.append(f'at most {gauge.estimate()} tokens, past {gauge.ceiling}')
        if over:
            stop_run(owner, identity, gauge, over, root, record)


def codex_meter(owner, meters, root, record):
    """Codex: stop a run whose reported total has reached its ceiling, or a
    lead past its call ceiling. The totals come from the event stream
    (`CodexMetering`)."""
    calls = lead_calls(root)
    at = time.monotonic()
    for identity, gauge in list(meters.items()):
        if gauge.looked is not None and gauge.runtime == 'active':
            gauge.silent += at - gauge.looked
        gauge.looked = at
        if gauge.stopped or gauge.runtime == 'exited':
            continue
        over = []
        if identity == LEAD and calls > CALL_CEILING:
            over.append(f'{calls} tool calls, past {CALL_CEILING}')
        if identity == LEAD and lead_withheld(root):
            # Its tool will not hand it another result: stop the turn now,
            # rather than let the held call run out and start a step.
            over.append(f'its tool withheld a result: {gauge.total} tokens reported, past '
                        f'{HOLD_ABOVE}')
        past = [u for u in getattr(gauge, 'untooled', []) if u['total'] > HOLD_ABOVE]
        if identity == LEAD and past and record.get('mutant') != 'meter-ignores-items':
            # A result the tool never saw came back past the hold: its step has
            # begun, and nothing but a stop ends the next one.
            over.append(f"a result its tool never saw ({past[0]['item_type']}) came back "
                        f"with {past[0]['total']} tokens reported, past {HOLD_ABOVE}")
        if gauge.total >= gauge.ceiling:
            over.append(f'{gauge.total} tokens reported, past {gauge.ceiling}')
        if gauge.subagents and record.get('mutant') != 'subagent-not-stopped':
            # No plan has a sub-agent: its spend is outside every share
            # (review of L3, round 3, SPEND-2).
            first = gauge.subagents[0]
            over.append(f"a sub-agent thread appeared ({first['thread_id']}, "
                        f"{(first['how'] or {}).get('by')}); no plan has one")
        if gauge.largest_step > CODEX['in_flight']:
            # The bound's one premise is broken: a step larger than the one
            # it allows in flight (review of L3, round 3, SPEND-4).
            over.append(f'a step of {gauge.largest_step} tokens, past the '
                        f"{CODEX['in_flight']} the bound assumes in flight")
        if gauge.silent >= USAGE_SILENCE:
            # Nothing reported, so nothing can stop it at its ceiling: stop
            # it now, loudly, and charge it its whole share.
            gauge.silenced = True
            over.append(f'no usage reported in {USAGE_SILENCE} s of activity; stopped '
                        'rather than run unmetered')
        if over:
            stop_run(owner, identity, gauge, over, root, record)


def stop_run(owner, identity, gauge, over, root, record):
    """Stop one run: for the lead, first the hold, so no result of its tool
    lets it take another step; then the cancel. What is recorded is PIO's
    receipt for the cancel; whether it reached the harness is the host's to
    say, and a row reads that (review of L3, F3)."""
    if identity == LEAD:
        (root / 'lead-stop').touch()
    if record.get('mutant') == 'ceiling-cancel-never-sent':
        sent = dict(outcome='MUTANT: the cancel was never sent')
    else:
        sent = cancel(owner, identity, 'ceiling')
    gauge.stopped = dict(why=over, at=now(), cancel=sent)
    record.setdefault('ceiling_stops', []).append(dict(run=identity, **gauge.stopped))
    print(f'CEILING {identity}: {"; ".join(over)}; cancel requested', flush=True)


def settle(record, service, meters, state, root):
    """Stop whatever still runs, and read everything the rows and the charge
    need while the service is still there to ask."""
    # The lead's tool holds from here on, whatever else happens.
    (root / 'lead-stop').touch()
    metering = state.get('metering')
    if metering is not None:
        metering.halt.set()
        metering.join(timeout=15)
        record['metering'] = dict(source='core.events.read, one query a pass',
                                  passes=metering.passes, seconds=metering.seconds,
                                  error=metering.error)
        if metering.error:
            record['meter_error'] = metering.error
    owner = service.owner()
    try:
        # Every execution under this lead the stream holds, whatever its
        # name, adopted before anything is cancelled (review of L3, round 3,
        # SPEND-1).
        for event in events(owner):
            adopt(meters, event['subject']['id'], record)
        stopped = {}
        for identity in runs(meters):
            current = view(owner, identity)
            if current and current.get('admission') == 'admitted' \
                    and current.get('runtime') != 'exited':
                stopped[identity] = cancel(owner, identity, 'exit')
        # The runner's own probes, each expected to be refused. One the
        # service admitted is a real run with no reservation: cancelled here
        # with the rest, and charged below (review of L3, round 2, SB-5).
        admitted = {}
        for probe in PROBE_SHARES:
            current = view(owner, probe) or {}
            if current.get('admission') == 'admitted':
                admitted[probe] = dict(
                    runtime=current.get('runtime'), share=PROBE_SHARES[probe],
                    cancel=cancel(owner, probe, 'probe')
                    if current.get('runtime') != 'exited' else None)
        if stopped or admitted:
            # OpenCode's host escalates an unanswered cancel to a kill after
            # ten seconds. **The Codex host has no such escalation**: a
            # turn/interrupt Codex did not honour leaves the run going. So
            # this waits at most a minute, for either, and a run still not
            # exited when its usage is read is charged its whole share
            # (review of L3, F2).
            if stopped:
                record['stopped_on_exit'] = stopped
            end = time.monotonic() + 60
            while time.monotonic() < end and any(
                    (view(owner, i) or {}).get('runtime') not in (None, 'exited')
                    for i in (*stopped, *admitted)):
                time.sleep(0.5)
        views = {i: view(owner, i) or {} for i in (*runs(meters), *PROBE_SHARES)}
        # A submit the service answered with an error made nothing: that run
        # is charged nothing, not its share (review of L3, round 2, SB-6).
        record['no_such_execution'] = sorted(i for i in RUNS if not views[i]
                                             and missing(owner, i))
        for identity, gauge in list(meters.items()):
            if views.get(identity):
                gauge.read(owner)
        stream = events(owner)
        log = tool_log(root)
        briefs = {LEAD: BRIEF, **PROBE_BRIEFS}
        briefs.update({f"{LEAD}.{e['arguments'].get('name')}": e['arguments'].get('brief', '')
                       for e in log if e.get('event') == 'tool_call'
                       and e.get('tool') == 'start_run'})
        host = service.host_events(views, briefs)
        # An admitted probe is charged as any run is, through the same
        # branches, and never below its share: at least its share, and at
        # least what it was seen to spend plus a step in flight (review of
        # L3, round 4, SPEND-12). OpenCode's is its share.
        for probe, entry in admitted.items():
            if HARNESS != 'codex':
                entry.update(charged=entry['share'], basis=PROBE_BASIS)
                continue
            gauge = meters.get(probe) or CodexMeter(probe, CHILD_CEILING)
            if views.get(probe):
                gauge.read(owner)
            spent = codex_usage(views, {probe: gauge}, {probe}, {probe}, record.get('mutant'),
                                host, identities=[probe],
                                shares={probe: entry['share']})[probe]
            charged = max(entry['share'], spent['charged'], spent['seen'] + spent['allowance'])
            if record.get('mutant') == 'probe-charged-flat':
                # The flat share, as before round 4.
                charged = entry['share']
            entry.update(spent, charged=charged, basis=f"{PROBE_BASIS}; {spent['basis']}")
        record['probes_admitted'] = admitted
        # What each run's host sent to turn Codex's unmetered features off,
        # where it did.
        record['features_off_sent'] = {i: next((e.get('sent') for e in host.get(i, [])
                                                if e['kind'] == 'features_off_sent'), None)
                                       for i in host}
        if HARNESS == 'opencode':
            sessions = {i: next((e.get('session_id') for e in host.get(i, [])
                                 if e['kind'] == 'session_created'), None)
                        for i in runs(meters)}
            steps, record['store_read'] = store_steps(service.config['opencode']['home'],
                                                      sessions)
        else:
            steps = {}
            record['store_read'] = dict(
                read=False, reason="Codex's own total covers every step (review 48); "
                                   'no store is read')
            # The qualification serve-codex made before any native work: what
            # ran, from its own record, never a literal of ours (review of L3,
            # REPIN-2). A labeled fake has none.
            qualified = service.store / 'qualification.json'
            if qualified.exists():
                record['harness']['qualification'] = qualification_block(
                    json.loads(qualified.read_text()))
                record['harness']['pinned'] = record['harness']['qualification']['pinned']
            else:
                record['harness']['qualification'] = dict(
                    qualified=None, reason='labeled fake: no qualification' if service.rehearse
                    else 'serve-codex wrote no qualification record')
        heard = {c: spoken(owner, f'{LEAD}.{c}') for c in CHILDREN}
        said = {c: text for c, (text, _) in heard.items()}
        answered = {c: last for c, (_, last) in heard.items()}
        relay = spoken(owner, LEAD)[0]
    finally:
        owner.close()
    calls = len([e for e in log if e.get('event') == 'request'
                 and e.get('method') == 'tools/call'])
    record['meters'] = {i: g.summary(calls if i == LEAD else None) for i, g in meters.items()}
    stops = {s['run'] for s in record.get('ceiling_stops', [])} \
        | set(record.get('stopped_on_exit') or {})
    record['usage'] = usage_of(views, meters, state['submitted'] | started_runs(root), steps,
                               stopped=stops, mutant=record.get('mutant'),
                               absent=set(record.get('no_such_execution') or []), host=host)
    record.update(views=views, tool_log=log, spoken=dict(said, lead=relay),
                  answered=answered)
    return dict(views=views, stream=stream, log=log, host=host, said=said, relay=relay,
                answered=answered, steps=steps)


def store_steps(home, sessions):
    """Each run's model steps, from the owner's OpenCode store.

    Owner decision, 2026-09-24: read-only, and only the `session_message`
    rows of the sessions PIO started. Nothing else in the store is opened,
    and no other table is named. A rehearsal's home has no store, so there
    the answer is that nothing could be read.
    """
    path = Path(home) / '.local/share/opencode/opencode.db'
    ids = sorted({s for s in sessions.values() if s})
    if not ids or not path.exists():
        return {i: None for i in sessions}, dict(
            read=False, reason='no store at the harness home' if ids else 'no session id')
    marks = ','.join('?' * len(ids))
    with contextlib.closing(sqlite3.connect(f'file:{path}?mode=ro', uri=True)) as db:
        found = db.execute(
            'select session_id, data from session_message '
            f"where type = 'assistant' and session_id in ({marks}) "
            'order by session_id, seq', ids).fetchall()
    by_session = {}
    for session, data in found:
        tokens = json.loads(data).get('tokens') or {}
        cache = tokens.get('cache') or {}
        by_session.setdefault(session, []).append(
            sum(tokens.get(k) or 0 for k in ('input', 'output', 'reasoning'))
            + (cache.get('read') or 0) + (cache.get('write') or 0))
    steps = {i: (dict(steps=by_session.get(s, []), total=sum(by_session.get(s, [])))
                 if s else None) for i, s in sessions.items()}
    return steps, dict(read=True, table='session_message', mode='ro',
                       sessions=len(ids), rows=len(found))


NO_SUCH_EXECUTION = 'no such execution: the service, asked, holds none, so nothing ran'
# Why a retry is counted and not charged (review of L3, round 4, SPEND-10).
RETRIES_NOT_CHARGED = (
    "each an `error` notification with willRetry true on the run's own thread, counted from "
    "the host's events; the first WebSocket retry of a step is not surfaced in a release build "
    '(core/src/responses_retry.rs:119-121, rust-v0.157.0), so this can undercount by one a '
    'step. Not charged: Codex records usage only on a completed response, so a dropped '
    'attempt reports nothing, and nothing PIO observes says what, if anything, it was billed; '
    'the bound lists it as outside (not measured)')
PROBE_BASIS = ('admitted though it should have been refused: a run with no reservation, '
               'charged at least its whole share, and at least what it was seen to spend plus '
               'a step in flight')


def usage_of(views, meters, submitted, steps=None, stopped=(), mutant=None, absent=(),
             host=None):
    """What each run reported, and what it is charged, on what basis.

    OpenCode's turn usage is its **last model step's**, so a turn that made
    tool calls reported less than it spent. Every earlier step's context is
    contained in the last one's, so the turn spent at most the reported total
    times the number of steps it could have taken: one per tool call, plus
    one. Where the owner's store gives the steps themselves and its last step
    is the one reported, the run is charged their sum. A run that reported
    nothing is charged its meter's bound, never less than the M3b allowance,
    and never less than the steps the store holds.
    """
    usage = (codex_usage(views, meters, submitted, stopped, mutant, host) if HARNESS == 'codex'
             else opencode_usage(views, meters, submitted, steps))
    for identity in absent:
        if identity in usage:
            usage[identity] = dict(charged=0, basis=NO_SUCH_EXECUTION)
    return usage


def opencode_usage(views, meters, submitted, steps=None):
    """OpenCode's charges; see usage_of."""
    steps = steps or {}
    usage = {}
    for identity in runs(meters):
        current = views.get(identity) or {}
        if current.get('admission') == 'refused':
            usage[identity] = dict(charged=0, basis='refused before any model call')
            continue
        if not current and identity not in submitted:
            continue
        observations = (current.get('usage') or {}).get('observations') or []
        reported = observations[0]['amount'] if observations else None
        gauge = meters[identity]
        most = len(gauge.calls) + 1
        measured = steps.get(identity) or {}
        recorded = measured.get('steps') or []
        if isinstance(reported, int) and reported > 0:
            entry = dict(reported_last_step=reported, steps_at_most=most,
                         bound=reported * most, measured_steps=recorded or None,
                         meter_estimate=gauge.estimate())
            if recorded and recorded[-1] == reported:
                entry.update(charged=sum(recorded), basis='measured_from_store_steps')
            elif recorded:
                entry.update(charged=max(sum(recorded), reported * most),
                             basis='steps_bound',
                             why="the store's last step is not the one reported")
            else:
                entry.update(charged=reported * most, basis='steps_bound')
            usage[identity] = entry
        else:
            usage[identity] = dict(reported_last_step=None, usage='unknown',
                                   measured_steps=recorded or None,
                                   charged=max(live.CANCEL_ALLOWANCE, gauge.estimate(),
                                               sum(recorded)),
                                   basis='allowance', meter_estimate=gauge.estimate())
    return usage


STOPPED_BASIS = ('stopped by the runner, or its turn interrupted: what it reported, plus a '
                 'step in flight on each of its threads')
NOT_EXITED_BASIS = ('not seen exited when its usage was read: its whole share, or what it '
                    'reported or its meter saw plus a step in flight on each of its threads, '
                    'if more')
SILENT_BASIS = (f'stopped by the runner after {USAGE_SILENCE} s of activity with no usage '
                'reported: its whole share, or what it reported plus a step in flight on each '
                'of its threads, if more')


def codex_usage(views, meters, submitted, stopped=(), mutant=None, host=None, identities=None,
                shares=None):
    """Codex's reported total for each run, which covers every step of its
    turn (review 48) and, from round 3, every thread it had: so it is what a
    run that ended by itself is charged.

    A run whose turn was cut short, by the runner's stop or by an interrupt
    the host sent (its own deadline, a cancel), may have had a step in
    flight on each of its threads that Codex bills and that no report holds,
    so it is charged what it reported plus that allowance, **not capped at
    its share**: a run that spent past its share is charged past it, and the
    row that checks the share fails (review of L3, round 2, SB-2). The
    allowance is a step per thread: the step the bound assumes, or the
    largest the run was seen to take, if larger (round 3, SPEND-4), times
    its own thread and every sub-agent's (SPEND-2). A run still not exited
    when its usage was read is charged its whole share, or what it reported
    or its own meter saw plus the allowance, if that is more; one stopped
    for silence the same (round 3, SPEND-6); one that reported nothing, its
    whole share or what was seen, if more. Never below what was reported or
    observed, and never nothing (F2).

    What was seen, the largest step, every thread and whether the turn was
    cut short come from the meter and, where the service could still be
    asked, from each run's own host events read afresh from the files: the
    meter stops folding them when the runner halts it, and a step or a
    thread the host writes after that, during the cancels on the way out,
    is the floor's and must be the charge's too (review of L3, round 4,
    SPEND-11)."""
    usage = {}
    for identity in (identities or runs(meters)):
        current = views.get(identity) or {}
        if current.get('admission') == 'refused':
            usage[identity] = dict(charged=0, basis='refused before any model call')
            continue
        if not current and identity not in submitted:
            continue
        observations = (current.get('usage') or {}).get('observations') or []
        reported = observations[0]['amount'] if observations else None
        gauge = meters[identity]
        share = (shares or {}).get(identity) or (LEAD_SHARE if identity == LEAD else CHILD_SHARE)
        filed = host_steps((host or {}).get(identity, []))
        if mutant == 'stale-meter-charged':
            # A charge that read the meter's fold alone, as before round 4.
            filed = host_steps([])
        allowance = max(CODEX['in_flight'], gauge.largest_step, filed['largest']) * (
            1 + max(len(gauge.subagents), len(filed['others'])))
        # Cut short: by the runner, or by an interrupt the host sent and
        # the turn ended on (round 3, SPEND-6).
        # Or by the host's interrupt of a turn Codex started by itself after
        # the run's own (round 4, SPEND-9).
        cut = identity in stopped or gauge.turn_status == 'interrupted' \
            or (current.get('cancellation') or {}).get('outcome') == 'cancelled' \
            or bool(gauge.continued) or filed['interrupted'] or bool(filed['continued'])
        if mutant == 'interrupted-charged-reported':
            cut = identity in stopped
        if mutant == 'continuation-uncharged':
            cut = cut and not (gauge.continued or filed['continued'])
        # What PIO saw it spend: its report (the sum over its threads), its
        # own meter's total, or the sum of its threads' host reports, if
        # higher (the meter is all there is when the service cannot be asked).
        seen = max(reported if isinstance(reported, int) else 0, gauge.total,
                   sum(gauge.threads.values()), filed['sum'])
        if mutant == 'subagent-uncounted':
            # A PIO that counted the run's own thread alone.
            reported = gauge.own_total or reported
            seen = reported if isinstance(reported, int) else 0
            allowance = max(CODEX['in_flight'], gauge.largest_step)
        entry = dict(reported_total=reported if isinstance(reported, int) else None,
                     meter_estimate=gauge.estimate(), share=share, seen=seen,
                     stopped_by_the_runner=identity in stopped, cut_short=cut,
                     allowance=allowance, threads=dict(gauge.threads) or None,
                     host_files=dict(sum=filed['sum'], largest=filed['largest'],
                                     others=filed['others']),
                     subagents=sorted({*(s['thread_id'] for s in gauge.subagents),
                                       *filed['others']}))
        if current.get('runtime') != 'exited':
            entry.update(charged=share if mutant == 'not-exited-charged-share'
                         else max(share, seen + allowance), basis=NOT_EXITED_BASIS)
        elif gauge.silenced:
            entry.update(charged=max(share, seen + allowance), basis=SILENT_BASIS)
        elif isinstance(reported, int) and reported > 0:
            if cut and mutant != 'stop-charged-reported':
                charged = seen + allowance
                if mutant == 'stopped-charge-capped':
                    # The round-1 cap, put back (review of L3, round 3, SPEND-7).
                    charged = min(charged, share)
                entry.update(charged=charged, basis=STOPPED_BASIS, in_flight=allowance)
            else:
                entry.update(charged=seen, basis='reported_total')
        else:
            entry.update(usage='unknown', charged=max(share, seen), basis='allowance')
        usage[identity] = entry
    return usage

def running_steer(s):
    """The running steer holds only against a turn that was running and
    delivered when it was sent, and still running just after. A turn that
    ended inside the steer's round trip decides nothing either way."""
    if not (s and s.get('request') == 'not_supported'
            and s.get('runtime_at_steer') == 'active'
            and s.get('delivery_at_steer') == 'acknowledged'):
        return False
    if s.get('runtime_after_steer') == 'exited':
        return None
    return s.get('runtime_after_steer') is not None


def host_model(records):
    """The model a run's session was on, from the host's own events."""
    kinds = [r['kind'] for r in records]
    selected = next((r for r in records if r['kind'] == 'model_selected'), None)
    created = next((r for r in records if r['kind'] == 'session_created'), None)
    turn = kinds.index('turn_start_sent') if 'turn_start_sent' in kinds else None
    return dict(
        requested=selected and selected.get('requested_model'),
        selection_error=selected and selected.get('error'),
        reported=created and created.get('reported_model'),
        matches=created and created.get('model_matches_requested'),
        before_turn=turn is not None and selected is not None and created is not None
        and kinds.index('model_selected') < turn and kinds.index('session_created') < turn)


def codex_model(records):
    """The model and provider a Codex thread was on, from the host's record
    of `thread/start`'s own answer, and whether that came before the turn."""
    kinds = [r['kind'] for r in records]
    checked = next((r for r in records if r['kind'] == 'model_checked'), None) or {}
    turn = kinds.index('turn_start_sent') if 'turn_start_sent' in kinds else None
    return dict(requested=checked.get('requested_model'), reported=checked.get('model'),
                provider=checked.get('model_provider'), matches=checked.get('matches'),
                before_turn=turn is not None and 'model_checked' in kinds
                and kinds.index('model_checked') < turn)


def qualification_block(q):
    """The Codex that ran, from serve-codex's own qualification record: the
    npm wrapper, the native binary and the Node that ran the wrapper, each by
    label and sha256, and the pin, version, schema and drift it was
    qualified against."""
    resolution = q.get('resolution') or {}

    def part(name, *extra):
        found = resolution.get(name) or {}
        return dict(label=found.get('path'), sha256=found.get('sha256'),
                    **{field: found.get(field) for field in extra})
    return dict(qualified=q.get('qualified'), pinned=q.get('pinned'),
                version=(q.get('version') or {}).get('native'),
                wrapper=part('wrapper', 'package_version'), native=part('native'),
                node=part('node', 'version'),
                schema_listing=(q.get('schema') or {}).get('canonical_listing_sha256'),
                drift=(q.get('schema') or {}).get('drift_count'),
                service_node_matches=(q.get('service_node_resolution') or {})
                .get('matches_qualification'))


def qualification_precheck(service, rehearse):
    """The pin row's judgment, before anything is reserved: live, a missing
    record or one that is not the committed record for its pin refuses the
    run. A labeled fake has no record, so a rehearsal checks only a record
    that is there (the mutants plant one)."""
    path = service.store / 'qualification.json'
    if not path.exists():
        if rehearse:
            return dict(checked=False, reason='labeled fake: serve-codex qualifies none')
        raise SystemExit('refusing to start: serve-codex wrote no qualification record')
    ran = qualification_block(json.loads(path.read_text()))
    committed = committed_qualification(ran.get('pinned'))
    held = qualified_at_pin(dict(ran=ran, committed=committed, rehearsal=False))
    if not held:
        raise SystemExit('refusing to start: the Codex serve-codex qualified is not the '
                         f"committed identity for its pin {(ran.get('pinned') or {}).get('version')}: "
                         'a wrapper, native binary, Node, version or schema differs, or its '
                         'record says it is not qualified')
    return dict(checked=True, holds=True, pinned=ran.get('pinned'))


def committed_qualification(pinned):
    """The committed record for the pin a service qualified against, or None
    if there is none."""
    version = (pinned or {}).get('version')
    path = QUALIFICATIONS / f'qualification-{version}.json'
    if not version or not path.exists():
        return None
    return qualification_block(json.loads(path.read_text())['record'])


# What must equal the committed record: every identity digest, the pin, the
# version and the schema. Labels are paths, and are not compared.
PINNED_FIELDS = (('pinned',), ('version',), ('wrapper', 'sha256'),
                 ('wrapper', 'package_version'), ('native', 'sha256'), ('node', 'sha256'),
                 ('node', 'version'), ('schema_listing',))


def qualified_at_pin(o):
    """Qualified, with no drift, the service's Node the qualified one, and
    every digest the committed record's for that pin. A rehearsal's labeled
    fake has no record: that decides nothing."""
    ran, committed = o['ran'], o['committed']
    if ran.get('qualified') is None and o['rehearsal']:
        return None
    if committed is None or ran.get('qualified') is not True or ran.get('drift') != 0:
        return False
    if ran['node']['sha256'] is not None and ran.get('service_node_matches') is not True:
        return False
    pick = lambda block, path: block.get(path[0]) if len(path) == 1 \
        else (block.get(path[0]) or {}).get(path[1])
    return all(pick(ran, f) is not None and pick(ran, f) == pick(committed, f)
               for f in PINNED_FIELDS)


def native_declines(stream):
    """What each run's host declined by itself, read from the run's own
    `execution.exit.observed` on the public stream, or, for a run whose start
    failed and that has no exit, from the runtime change that marks it
    refused before delivery (review of L3, round 4, R4-HC-2): a list, empty
    for none, or None where neither carried one."""
    found = {}
    for e in stream:
        if e['type'] == 'execution.exit.observed':
            found[e['subject']['id']] = e['payload'].get(NATIVE)
        elif e['type'] == 'execution.runtime.changed' \
                and e['payload'].get('reason') == 'refused_before_delivery':
            found.setdefault(e['subject']['id'], e['payload'].get(NATIVE))
    return found


def codex_running_steer(o):
    """L3's claim: a steer under the lead's grant, recorded while the turn
    was running and delivered, and then acknowledged by Codex with the id it
    returned. What the model did with it is not observed, and says so. A
    turn that ended inside the steer's round trip decides nothing."""
    s, entry = o['steer'] or {}, o['delivery'] or {}
    if not (s.get('request') == 'recorded' and s.get('runtime_at_steer') == 'active'
            and s.get('delivery_at_steer') == 'acknowledged'):
        return False
    if s.get('runtime_after_steer') == 'exited' and entry.get('delivery') != 'acknowledged':
        return None
    return entry.get('delivery') == 'acknowledged' \
        and entry.get('proof_class') == 'provider_ack_id' \
        and entry.get('behavior') == 'not_observed'


def judge(rows, record, observed, desk, meters, state, rehearse):
    views, stream, log, host = (observed['views'], observed['stream'], observed['log'],
                                observed['host'])
    said, relay, answered = observed['said'], observed['relay'], observed['answered']
    grant_id = state['grant_id']
    truth = record['wc_l']
    # Every run under this lead that ran, the plan's and any other (review
    # of L3, round 3, SPEND-1); the runner's own probes are judged apart.
    everyone = runs(meters)

    # --- The lead's own run.
    lead_view = views[LEAD]
    rows.add('The lead was admitted', lead_view.get('admission'), 'admitted')
    rows.add("The lead's delivery was acknowledged", lead_view.get('delivery'),
             'acknowledged')
    # An app-server exits 0 after a turn that was interrupted too, so the
    # exit code alone would call a stopped run normal: a cancel on the view
    # fails it (review of L3, F11).
    rows.add('The lead exited normally',
             dict(runtime=lead_view.get('runtime'), exit=lead_view.get('exit'),
                  cancellation=lead_view.get('cancellation')),
             dict(runtime='exited', exit={'code': 0}, cancellation=None),
             note='no cancel: a turn that was interrupted did not exit normally')

    # --- The model every session was on, before its turn began. From the
    # host's own record of the harness's answer, not from what PIO asked.
    if HARNESS == 'codex':
        rows.add("Each run was on the plan's model before its turn started",
                 {i: codex_model(host.get(i, [])) for i in RUNS},
                 {i: dict(requested=MODEL, reported=MODEL, provider=CODEX['model_provider'],
                          matches=True, before_turn=True) for i in RUNS},
                 note="model_checked: thread/start's own answer, before turn_start_sent; the "
                      'host refuses a mismatch before the first turn'
                      + ('; the fake answers what its scenario says' if rehearse else ''))
        ran = record['harness'].get('qualification') or {}
        rows.add('Codex qualified at the pinned identity',
                 dict(ran=ran, committed=committed_qualification(ran.get('pinned')),
                      rehearsal=rehearse),
                 "qualified, no schema drift, the service's Node the qualified one, and "
                 'the wrapper, native binary and Node digests, version, pin and schema '
                 "listing equal to the committed record for serve-codex's pin",
                 holds=qualified_at_pin,
                 note="from serve-codex's own qualification record in the store; a labeled "
                      'fake has none, so a rehearsal decides nothing here unless a record '
                      'is there')
        rows.add('Every run asserted that approvals go to the user',
                 {i: next((e.get('approvals_reviewer') for e in host.get(i, [])
                           if e['kind'] == 'thread_started'), None) for i in RUNS},
                 {i: 'user' for i in RUNS},
                 note='approvalsReviewer is never set; any other answer ends the run '
                      'before its first turn')
    else:
        rows.add("Each run was on the plan's model before its turn started",
                 {i: host_model(host.get(i, [])) for i in RUNS},
                 {i: dict(requested=MODEL, selection_error=None, reported=MODEL, matches=True,
                          before_turn=True) for i in RUNS},
                 note='model_selected, then session_created (reported_model, '
                      'model_matches_requested), both before turn_start_sent'
                      + ('; the fake reports what it was asked for' if rehearse else ''))

    # --- The tool, the lead's session and nobody else's.
    sent = {run: [e.get('names') for e in host.get(run, [])
                  if e['kind'] == 'mcp_servers_sent'] for run in RUNS}
    rows.add('Only the lead got the tool', sent,
             {LEAD: [[LEAD_SERVER]], **{f'{LEAD}.{c}': [[]] for c in CHILDREN}})
    declined = native_declines(stream)
    if HARNESS == 'codex':
        # Owner decision, 2026-09-25: the lead's own two tools, per launch,
        # and nothing else; so Codex asked nothing about them. **By any
        # path**: an ask PIO surfaced, or one in a shape PIO did not
        # recognise and declined by itself (review of L3, CH-2/F1). An
        # elicitation from the lead's own server, or from a server Codex did
        # not name, counts.
        lead_declined = declined.get(LEAD)
        if record.get('mutant') == 'lead-exit-uncarried':
            # A lead whose exit carried no list, and which was not seen to
            # exit: the host lost, say (review of L3, round 3, C3-3).
            lead_declined = None
        asked = dict(
            surfaced=[e.get('message') for e in host.get(LEAD, [])
                      if e['kind'] == 'action_requested'
                      and e.get('approval_kind') == 'mcp_tool_call'],
            # By either route Codex has: an elicitation from the lead's own
            # server (or one it did not name), or a request for the user's
            # input whose question is Codex's MCP tool-call approval (review
            # of L3, round 2, HR-2). An exit that carried no list says
            # nothing, and is not taken for 'nothing asked'.
            declined_by_pio=[{k: d.get(k) for k in ('method', 'mode', 'approval_kind', 'server',
                                                    'tool', 'mcp_tool_call_approval')}
                             for d in lead_declined
                             if (d.get('method') == ELICITATION
                                 and d.get('server') in (LEAD_SERVER, None))
                             or (d.get('method') == 'item/tool/requestUserInput'
                                 and d.get('mcp_tool_call_approval') is True)]
            # Whatever the lead's runtime: a list that was never carried is
            # not "nothing asked" (review of L3, round 3, R3-HC-9 / C3-3).
            if lead_declined is not None
            else 'not carried: no exit of the lead carried what PIO declined')
        held = not asked['surfaced'] and not asked['declined_by_pio']
        expected_asked = dict(surfaced=[], declined_by_pio=[])
        # What each thread's config carried on the wire, as the host read it
        # back from the request it sent: exactly the lead's two tools at
        # `approve`, and no server-wide default (review of L3, CH-1).
        expected_pre = dict(pre_allowed={LEAD: [{LEAD_SERVER: dict(
            tools={tool: dict(approval_mode='approve') for tool in PRE_ALLOWED},
            default_tools_approval_mode=None)}],
            **{f'{LEAD}.{c}': [{}] for c in CHILDREN}},
            asked_about_the_tool=expected_asked)
        rows.add("The lead's own two tools were pre-allowed, and nothing else",
                 dict(pre_allowed={run: [e.get('servers') for e in host.get(run, [])
                                         if e['kind'] == 'mcp_servers_sent'] for run in RUNS},
                      asked_about_the_tool=asked),
                 expected_pre,
                 # An ask PIO surfaced, or a wrong wire, fails it; with neither,
                 # a list that was never carried decides nothing.
                 holds=lambda o: False if o['pre_allowed'] != expected_pre['pre_allowed']
                 or o['asked_about_the_tool']['surfaced']
                 else None if isinstance(o['asked_about_the_tool']['declined_by_pio'], str)
                 else o == expected_pre,
                 note="in the lead's thread config only; never written to the owner's"
                      + ('' if held else
                         f"; THE PRE-ALLOWANCE DID NOT HOLD: Codex asked about the lead's "
                         f"tool {len(asked['surfaced'])} time(s) PIO surfaced and "
                         f"{len(asked['declined_by_pio'])} time(s) PIO declined by itself"))
    # The tool's own witness, not the host's account of itself: each
    # launch writes `started`. One is the lead's session; none means the
    # lead never had it, and three means the children did too.
    rows.add('The tool was launched exactly once',
             len([e for e in log if e.get('event') == 'started']), 1)
    methods = [e['method'] for e in log if e.get('event') == 'request']
    rows.add('The tool reached the lead', methods[:3],
             ['initialize', 'notifications/initialized', 'tools/list'])
    rows.add("Every line of the lead tool's log carries the time it was written",
             dict(lines=len(log), stamped=len([e for e in log if e.get('at')])),
             'every line, and at least one',
             holds=lambda o: o['lines'] > 0 and o['stamped'] == o['lines'])
    if HARNESS == 'codex':
        # PIO's own server settings as the host sent them, read back from
        # its own request, and the tool's server ready on the lead's thread,
        # by Codex's own startup status, before its turn (L3's first live
        # run, 2026-09-26).
        def visibility(run):
            events_of_run = host.get(run, [])
            kinds = [e['kind'] for e in events_of_run]
            sent = next((e.get('settings') for e in events_of_run
                         if e['kind'] == 'mcp_servers_sent'), None) or {}
            ready = next((e for e in events_of_run if e['kind'] == 'lead_tool_ready'), None)
            return dict(
                sent=sent.get(LEAD_SERVER),
                ready=ready and ready.get('ready'),
                before_turn=None if ready is None else 'turn_start_sent' in kinds
                and kinds.index('lead_tool_ready') < kinds.index('turn_start_sent'))
        rows.add("The lead's tool was in the model's own list, and ready before its turn",
                 {run: visibility(run) for run in RUNS},
                 {run: dict(sent=LEAD_TOOL_SETTINGS if run == LEAD else None,
                            ready=True if run == LEAD else None,
                            before_turn=True if run == LEAD else None) for run in RUNS},
                 note="the lead tool's own server table as the host sent it: omit_tools_from "
                      "code_mode and deferred keep its tools in gpt-5.6-terra's own tool list "
                      '(code-mode-only; core/src/tools/spec_plan.rs:234-266 at rust-v0.157.0), '
                      "and ready is Codex's mcpServer/startupStatus/updated on the lead's thread "
                      'before turn/start' + ('; the fake computes the exposure from that source'
                                            if rehearse else ''))
    calls = [e for e in log if e.get('event') == 'tool_call']
    rows.add('The lead started its two runs through the tool',
             sorted((e['arguments'].get('name'), e['result'].get('started'))
                    for e in calls if e['tool'] == 'start_run'),
             sorted((c, True) for c in CHILDREN))
    rows.add('Every tool request carried the grant',
             sorted({e.get('grant') for e in log}), [grant_id])

    # --- The runs it started.
    children = {f'{LEAD}.{c}': views[f'{LEAD}.{c}'] for c in CHILDREN}
    rows.add('The submits landed', {i: v.get('admission') for i, v in children.items()},
             {i: 'admitted' for i in children})
    rows.add('origin.initiator is bound', {i: v.get('origin') for i, v in children.items()},
             {i: dict(initiator=dict(kind='execution.execution', id=LEAD), depth=1,
                      call_budget=0) for i in children})
    rows.add('The children ran',
             {i: dict(delivery=v.get('delivery'), runtime=v.get('runtime'),
                      exit=v.get('exit'), cancellation=v.get('cancellation'))
              for i, v in children.items()},
             {i: dict(delivery='acknowledged', runtime='exited', exit={'code': 0},
                      cancellation=None) for i in children},
             note='to the end of their turns: a child that was cancelled did not')
    third = record.get('third')
    rows.add('A third start is refused by PIO',
             # Still pending, or failed, is what it is: not a refusal.
             dict(started=third['result'].get('started'),
                  code=(third['result'].get('refused') or {}).get('code'),
                  lead_running=third['lead_runtime'] != 'exited')
             if third and 'result' in third else third,
             dict(started=False, code='call_budget_spent', lead_running=True))
    led = sorted(e['subject']['id'] for e in stream
                 if e['type'] == 'execution.exit.observed'
                 and e['subject']['id'].startswith(f'{LEAD}.'))
    rows.add(f'{LEAD} has exactly two children that ran', led, sorted(children))

    # --- What the grant carries, and the one thing it does not.
    under = [e['payload'].get(UNDER_GRANT) for e in stream
             if e['type'] == 'execution.steer.requested'
             and e['subject']['id'] == f'{LEAD}.alpha']
    if HARNESS == 'codex':
        steered = record.get('steer_running') or {}
        entry = next((d for d in (views[f'{LEAD}.alpha'].get('steering') or [])
                      if steered.get('delivery_id')
                      and d.get('delivery_id') == steered.get('delivery_id')), None)
        rows.add('A steer while the child runs', dict(steer=steered or None, delivery=entry),
                 'recorded under the grant while the turn was active and delivered, then '
                 'acknowledged by Codex (provider_ack_id); behavior not observed',
                 holds=codex_running_steer,
                 note="L3's claim: delivery under the lead's authority, not obedience")
    else:
        rows.add('A steer while the child runs', record.get('steer_running'),
                 'not_supported, sent while the turn was active and delivered, and '
                 'before it exited',
                 holds=running_steer,
                 note="OpenCode's own negative: its turn was running and delivered, and "
                      'the host never sets a turn id')
    rows.add('A steer on an exited run', record.get('steer_exited'),
             'not_supported on any harness, once the turn is over',
             holds=lambda s: bool(s) and s.get('request') == 'not_supported'
             and s.get('runtime_at_steer') == 'exited')
    rows.add('Each steer names the grant that sent it',
             [dict(grant=u and u.get('grant'), holder=u and u.get('holder'),
                   recorded_by=u and u.get('recorded_by')) for u in under],
             [dict(grant=grant_id, holder='lead', recorded_by='pio')] * 2)
    data = (record.get('lead_answer') or {}).get('error', {}).get('data', {})
    rows.add('The lead may not answer an approval',
             dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
             dict(code='permission_denied', reason='right_missing'),
             note='aimed at a child it may read, so only the missing right can refuse it')
    data = (record.get('grant_attach') or {}).get('error', {}).get('data', {})
    rows.add('A grant cannot attach the tool',
             dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
             dict(code='permission_denied', reason='owner_authority_required'))
    outcome = (record.get('credential_check') or {}).get('result', {}).get('outcome', {})
    rows.add('A tool spec carrying a credential is refused',
             dict(admission=outcome.get('admission'), reason=outcome.get('reason'),
                  named='lead_tool_carries_a_credential_value'
                  in str(outcome.get('alternative'))),
             dict(admission='refused', reason='capability_unavailable', named=True))
    data = (record.get('lead_reads_itself') or {}).get('error', {}).get('data', {})
    rows.add('The lead cannot read its own run',
             dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
             dict(code='permission_denied', reason='out_of_scope'),
             note=f'the grant covers {LEAD}. and not {LEAD}')

    # --- The results, against the runner's own count.
    # In a rehearsal the fake counted and the relay is scripted, so these
    # two show the runner compares — which is what kills `wrong-child`
    # and `wrong-relay` — and not that a model got anything right.
    compares = ('rehearsal: the fake counted, so this shows the runner compares'
                if rehearse else '')
    # Each child's **answer**: the last message it completed. On this model
    # a run says what it is about to do first, and a preamble that quotes
    # `sleep 30 && wc -l alpha.md` holds a number that is not the count (M2
    # R5, R6; review of L3, F5).
    final = said if record.get('mutant') == 'first-number-of-all' else answered
    rows.add('Each child reported the true count',
             {CHILDREN[c]: first_number(final[c]) for c in CHILDREN}, truth,
             note=(compares + '; ' if compares else '') + 'the last message each child '
                  'completed after its last command, not its preamble; none if it never '
                  'answered')
    rows.add('The lead relayed the true counts', relayed(relay), truth, note=compares)

    # --- Approvals: the desk, never PIO by default, never "always".
    # Who decided is on the stream; the option actually sent is the host's
    # own record of what it applied, because the stream's decision for a
    # caller's answer carries only the decision.
    decisions = []
    for e in stream:
        if e['type'] != 'execution.action.answered':
            continue
        run_id, action_id = e['subject']['id'], e['payload'].get('action_id', '')
        seq = int(action_id.rsplit('-', 1)[-1]) if action_id[-1:].isdigit() else None
        applied = next((a for a in host.get(run_id, []) if a['kind'] == 'control_applied'
                        and a.get('action_seq') == seq), {})
        item = desk.items.get(action_id, {})
        decisions.append(dict(
            run=run_id, action_id=action_id, desk=item.get('state'),
            desk_decision=(item.get('answer') or {}).get('decision'),
            decided_by=e['payload'].get(DECISION, {}).get('decided_by'),
            applied=applied.get('applied'), option_kind=applied.get('option_kind'),
            always_option_taken=applied.get('always_option_taken'),
            approval_kind=next((r.get('approval_kind') for r in host.get(run_id, [])
                                if r['kind'] == 'action_requested'
                                and r.get('action_seq') == seq), None),
            sent=applied.get('sent')))
    lapsed = [dict(run=run_id, action_seq=r.get('action_seq'),
                   after_seconds=r.get('after_seconds'), option_kind=r.get('option_kind'))
              for run_id, records in host.items() for r in records
              if r['kind'] == 'request_denied_by_default']
    single_use = {'allow': 'allow_once', 'deny': 'reject_once'}
    # On Codex the host records the response it wrote: a decision, or an
    # elicitation action with no `persist`. Nothing else is single-use.
    sent_once = {'allow': [{'decision': 'accept'}, {'action': 'accept', 'content': {}}],
                 'deny': [{'decision': 'decline'}, {'action': 'decline'}]}
    once = (lambda d: d['sent'] in sent_once.get(d['desk_decision'], [])) \
        if HARNESS == 'codex' else \
        (lambda d: d['applied'] is True and d['option_kind'] == single_use.get(d['desk_decision']))
    # A request the host declined by itself was decided by PIO, by default,
    # and never reached the desk. Each one fails the row, and a run that
    # exited without carrying the list at all fails it too: PIO's own
    # decisions could not be read (review of L3, F1).
    exited = [i for i in everyone if (views.get(i) or {}).get('runtime') == 'exited']
    by_pio = {i: declined.get(i) for i in exited}
    rows.add('Every approval was decided at the desk',
             dict(decisions=decisions, lapsed=lapsed,
                  desk={a: i['state'] for a, i in desk.items.items()},
                  declined_by_pio=by_pio),
             'each relayed and answered at the desk, decided by the caller, sent as '
             'the single-use kind, never always, none lapsed, and nothing declined by '
             'PIO by itself',
             # Nothing asked, nothing decided: that cannot fail, so it is
             # not a pass either (review 47).
             holds=lambda o: False if any(v is None or v for v in o['declined_by_pio'].values())
             else None if not o['desk'] and not o['lapsed'] and not o['decisions']
             else not o['lapsed']
             and all(s == 'answered' for s in o['desk'].values())
             and len(o['desk']) == len(o['decisions'])
             and all(d['decided_by'] == 'caller' and once(d)
                     and d['always_option_taken'] is False for d in o['decisions']),
             note=f'{len(decisions)} approval(s) asked; none asked is inconclusive; a lapse '
                  f"is the host's single-use reject after the delivery timeout, and it "
                  f'fails {LEAD}; so does any request PIO declined by itself, and a run '
                  'whose exit did not carry that list')
    rows.record('What PIO declined by itself',
                {i: declined.get(i, 'not carried: the run has no exit event') for i in everyone},
                note="each request the host answered with an error, never put to a caller: "
                     "what was asked (method, server, mode, Codex's approval kind, tool name) "
                     'and why; never an argument, a form, a URL or a message')
    if PLAN['waits']:
        waiting = f"{LEAD}.{PLAN['waits']}"
        reads = [dict(run=e['result'].get('run'), runtime=e['result'].get('runtime'),
                      seconds=e.get('seconds'))
                 for e in record['tool_log'] if e.get('event') == 'tool_call'
                 and e.get('tool') == 'read_run' and isinstance(e.get('result'), dict)]
        # Two ways a read shows it waited: it took at least WAITED seconds
        # and came back `exited`, or it came back still running at the tool's
        # own limit. L1b's first attempt read `alpha` in 54.6 s of 55, so a
        # slightly slower `alpha` would have failed a row about waiting by
        # waiting the longest it can (owner decision, 2026-09-25).
        rows.add('A read_run waited for its run: until it exited, or to the wait limit',
                 [r for r in reads if r['run'] == waiting],
                 f'a read of {waiting} that took at least {WAITED} s and returned exited, '
                 f'or one that returned still running after the tool\'s {READ_WAIT} s limit',
                 holds=lambda o: any(
                     (r['runtime'] == 'exited' and (r['seconds'] or 0) >= WAITED)
                     or (r['runtime'] not in ('exited', None)
                         and (r['seconds'] or 0) >= READ_WAIT) for r in o),
                 note='seconds as the lead tool measured each call')
    if HARNESS == 'codex':
        # Each stream retry Codex surfaced, per run (review of L3, round 4,
        # SPEND-10), and why none is charged.
        rows.record('Stream retries Codex reported',
                    {i: len([e for e in host.get(i, []) if e['kind'] == 'native_error'
                             and e.get('will_retry') is True]) for i in everyone},
                    note=RETRIES_NOT_CHARGED)
        rows.record('What Codex asked, and what was sent',
                    [dict(run=d['run'], approval_kind=d['approval_kind'], desk=d['desk'],
                          sent=d['sent']) for d in decisions],
                    note="the fake in a rehearsal; the owner's Codex live")
    else:
        rows.record('What OpenCode does with a permission prompt',
                    [dict(run=d['run'], option_kind=d['option_kind'], desk=d['desk'])
                     for d in decisions],
                    note='the fake in a rehearsal; the owner\'s OpenCode live')

    # --- Spend: bounded while it ran, and reported at the end.
    summary = record['meters']
    rows.add('Every run stayed within its ceilings', summary,
             f'no stop; the lead made at most {CALL_CEILING} tool calls; every '
             'bound stayed under its ceiling',
             holds=lambda m: all(g['stopped'] is None and g['estimate'] < g['ceiling']
                                 for i, g in m.items() if views.get(i))
             and max(m[LEAD]['calls'], m[LEAD]['tool_log_calls'] or 0) <= CALL_CEILING,
             note=f"estimate {BOUND['estimate']}")
    usage = record['usage']
    if HARNESS == 'codex':
        # A stop is PIO's request; whether it reached Codex is the host's to
        # say: its own control_sent for turn/interrupt, Codex's answer to
        # it, and the turn ending interrupted (review of L3, F3).
        stops = {s['run'] for s in record.get('ceiling_stops', [])} \
            | set(record.get('stopped_on_exit') or {})
        reached = {}
        for run in sorted(stops):
            events_of_run = host.get(run, [])
            sent = {e.get('control_id') for e in events_of_run
                    if e['kind'] == 'control_sent' and e.get('method') == 'turn/interrupt'
                    and str(e.get('control_id', '')).startswith(f'{run}.cancel-')}
            reached[run] = dict(
                interrupt_sent=bool(sent),
                acknowledged=any(e['kind'] == 'control_response' and e.get('control_id') in sent
                                 and e.get('error') is None for e in events_of_run),
                turn=next((e.get('status') for e in events_of_run
                           if e['kind'] == 'turn_completed'), None),
                cancellation=((views.get(run) or {}).get('cancellation') or {}).get('outcome'))
        # Every result the lead got was held to its hold: each response its
        # tool gave passed the tool's gate, and each result its tool never
        # saw that came back past the hold was met by a stop (review of L3,
        # round 2, SB-1).
        responded = [e for e in log if e.get('event') in ('tool_call', 'tool_failed')]
        gates = {e.get('call'): e for e in log if e.get('event') == 'gate'}
        host_total, came_back = 0, []
        for event in host.get(LEAD, []):
            if event['kind'] == 'usage':
                total = (event.get('total') or {}).get('totalTokens')
                host_total = max(host_total, total if isinstance(total, int) else 0)
            elif untooled(event):
                came_back.append(dict(item_type=event.get('item_type'), status=event.get('status'),
                                      server=event.get('server'), total=host_total))
        checked = dict(
            responses=len(responded),
            ungated=[e.get('call') for e in responded if e.get('call') not in gates],
            handed_back_past_hold=[g for g in gates.values() if isinstance(g.get('total'), int)
                                   and g['total'] > g['hold_above'] and not g['held']],
            untooled=came_back,
            stopped_for_one=any('never saw' in w for s in record.get('ceiling_stops', [])
                                if s['run'] == LEAD for w in s['why']))
        rows.add('Every result the lead got was checked against its hold', checked,
                 f'every response of its tool passed the gate, none was handed back past '
                 f'{HOLD_ABOVE}, and a result its tool never saw that came back past '
                 f'{HOLD_ABOVE} was met by a stop',
                 holds=lambda o: None if not o['responses'] and not o['untooled'] else
                 not o['ungated'] and not o['handed_back_past_hold']
                 and (o['stopped_for_one'] or not any(u['total'] > HOLD_ABOVE
                                                       for u in o['untooled'])),
                 note="the lead tool's own gate records, and the lead's host events in the "
                      'order the host wrote them')
        rows.add('Every stop the runner made reached Codex', reached,
                 'for each run the runner stopped: the host sent turn/interrupt, Codex '
                 'acknowledged it, and the turn ended interrupted (cancellation cancelled)',
                 holds=lambda o: None if not o else all(
                     r['interrupt_sent'] and r['acknowledged'] and r['turn'] == 'interrupted'
                     and r['cancellation'] == 'cancelled' for r in o.values()),
                 note="the host's own control_sent and control_response, not PIO's "
                      'cancel_requested; no stop, nothing to judge')
        # What each run is charged, against what it could have spent and the
        # share reserved for it: a run that ended by itself at least what it
        # reported, one the runner stopped at least that plus a step in
        # flight (up to its share), one not seen exited its whole share, and
        # none more than its share (review of L3, F2 and F3).
        # Two rows, so that undercharging and overspending fail apart (review
        # of L3, round 2, SB-2). The floor is worked out here from what was
        # observed (each run's own view, its meter, whether the runner
        # stopped it, whether it was still running), not from the charge's
        # own basis or formula: what it reported or its meter saw, plus a
        # step in flight if the runner stopped it or it still ran, and at
        # least its whole share if it still ran, reported nothing, or was
        # stopped for silence.
        # An admitted probe is judged as any run is, at least at its share
        # (review of L3, round 4, SPEND-12).
        probes = record.get('probes_admitted') or {}
        charged_runs = {**usage, **probes}
        step, floors, within = CODEX['in_flight'], {}, {}
        observed_steps = {run: host_steps(host.get(run, [])) for run in charged_runs}
        for run, u in charged_runs.items():
            if u['basis'] in ('refused before any model call', NO_SUCH_EXECUTION):
                continue
            current = views.get(run) or {}
            share = u.get('share') or (LEAD_SHARE if run == LEAD else CHILD_SHARE)
            reported = ((current.get('usage') or {}).get('observations') or [{}])[0].get('amount')
            gauge = record['meters'].get(run) or {}
            steps_seen = observed_steps[run]
            seen = max(reported if isinstance(reported, int) else 0, gauge.get('estimate') or 0,
                       steps_seen['sum'])
            running = current.get('runtime') != 'exited'
            cut = run in stops or steps_seen['interrupted'] or bool(steps_seen['continued']) \
                or (current.get('cancellation') or {}).get('outcome') == 'cancelled'
            allowance = max(step, steps_seen['largest']) * (1 + len(steps_seen['others']))
            floor = seen + (allowance if cut or running else 0)
            if running or not isinstance(reported, int) or reported <= 0 or gauge.get('silenced') \
                    or run in probes:
                floor = max(floor, share)
            floors[run] = dict(charged=u['charged'], at_least=floor, reported=reported,
                               meter=gauge.get('estimate'), threads_sum=steps_seen['sum'],
                               allowance=allowance, stopped=run in stops, cut_short=cut,
                               running=running, silenced=bool(gauge.get('silenced')))
            within[run] = dict(charged=u['charged'], share=share)
        rows.add("Every run's charge covers what it could have spent", dict(runs=floors),
                 'for every run that ran: at least what it reported, its meter saw or its '
                 'threads reported, plus a step in flight on each of its threads if it was '
                 'stopped, interrupted or still ran, and its whole share if it still ran, '
                 'reported nothing or went silent',
                 holds=lambda o: bool(o['runs']) and all(
                     r['charged'] >= r['at_least'] for r in o['runs'].values()),
                 note=f'a step in flight: {step}, or the largest seen, if larger; worked out '
                      "from the views, the meters and the host's own usage events, not from "
                      'the charge')
        # The bound's one premise, checked: no model step larger than the
        # step it allows in flight (review of L3, round 3, SPEND-4). From the
        # host's own usage events: each report's last step, and the rise
        # between two reports on the same thread.
        rows.add('Every step stayed within the in-flight bound',
                 {run: dict(largest=o['largest'], steps=o['steps'])
                  for run, o in observed_steps.items() if o['steps']},
                 f"no model step, on any thread of any run, past {step} tokens",
                 holds=lambda o: None if not o else all(
                     r['largest'] <= step for r in o.values()),
                 note="each report's last step and the rise between reports, from the "
                      "host's own usage events; a run past it is stopped")
        # No plan has a sub-agent, and one that appears is stopped at once
        # (review of L3, round 3, SPEND-2): from the host's own events and
        # from what each exit carried.
        carried = other_threads_carried(stream)
        exited_runs = [r for r in usage if (views.get(r) or {}).get('runtime') == 'exited'
                       and u_ran(usage[r])]
        spawned = {run: sorted({*observed_steps[run]['others'],
                                *(t['thread_id'] for t in (carried.get(run) or []))})
                   for run in usage if u_ran(usage[run])}
        rows.add('No run spawned a sub-agent',
                 dict(threads=spawned,
                      not_carried=[r for r in exited_runs if carried.get(r) is None]),
                 'no thread but its own on any run: no plan has a sub-agent',
                 holds=lambda o: not any(o['threads'].values()) and not o['not_carried'],
                 note="the host's other_thread events, and the list each exit carries "
                      '(pio.combraton.dev/other-threads); an exit without it is not nothing')
        stopped_for = {run: any('sub-agent' in w for s in record.get('ceiling_stops', [])
                                if s['run'] == run for w in s['why'])
                       for run, threads in spawned.items() if threads}
        rows.add('Every run that spawned a sub-agent was stopped when it appeared',
                 stopped_for, 'each such run stopped by the runner, for its sub-agent',
                 holds=lambda o: None if not o else all(o.values()),
                 note='none spawned, nothing to judge')
        # No turn of Codex's own after a run's turn had ended (review of L3,
        # round 4, SPEND-9): a goal's continuation, which the host, reading
        # the run's thread for its grace period, interrupts; the run is then
        # charged as cut short. From the host's own events and each exit.
        continuations = {e['subject']['id']: (e.get('payload') or {}).get(CONTINUATIONS)
                         for e in stream if e['type'] == 'execution.exit.observed'}
        rows.add('No run took a turn of its own after its turn ended',
                 dict(turns={run: sorted({*observed_steps[run]['continued'],
                                          *(c.get('turn_id') for c in
                                            (continuations.get(run) or []))})
                             for run in usage if u_ran(usage[run])},
                      not_carried=[r for r in exited_runs if continuations.get(r) is None]),
                 'no turn started on any run\'s own thread after its turn had ended',
                 holds=lambda o: not any(o['turns'].values()) and not o['not_carried'],
                 note="the host's continuation_started events and the list each exit carries "
                      '(pio.combraton.dev/continuations); such a turn is interrupted by the host '
                      'and its run charged a step in flight')
        rows.add("Every run's charge stayed within its reserved share",
                 dict(runs=within, total=sum(r['charged'] for r in within.values()),
                      worst_case=WORST_CASE),
                 'every run charged at most the share reserved for it, and all of them at '
                 'most the worst case',
                 holds=lambda o: bool(o['runs']) and o['total'] <= o['worst_case'] and all(
                     r['charged'] <= r['share'] for r in o['runs'].values()),
                 note=f'lead share {LEAD_SHARE}, child share {CHILD_SHARE}, worst case '
                      f'{WORST_CASE}')
    ran = {i: u for i, u in usage.items()
           if u['basis'] not in ('refused before any model call', NO_SUCH_EXECUTION)}
    if HARNESS == 'opencode':
        rows.add("Each run's steps were read from the owner's store",
                 dict(read=record.get('store_read'),
                      runs={i: dict(steps=u.get('measured_steps'),
                                    reported_last_step=u.get('reported_last_step'),
                                    basis=u['basis']) for i, u in ran.items()}),
                 'every run that ran has its steps there, and the last is the one reported',
                 holds=lambda o: bool((o['read'] or {}).get('read')) and bool(o['runs'])
                 and all(r['steps'] and (r['reported_last_step'] is None
                                         or r['steps'][-1] == r['reported_last_step'])
                         for r in o['runs'].values()),
                 live_only=True,
                 note="owner decision 2026-09-24: session_message rows of PIO's sessions, "
                      "read-only; measured from the store's recorded steps, not the bill: "
                      'that the store holds every billed call is not proven')
    rows.add('Every run reported its usage',
             {i: u.get('reported_total', u.get('reported_last_step')) for i, u in usage.items()
              if u['basis'] not in ('refused before any model call', NO_SUCH_EXECUTION)},
             'a positive amount per run that ran',
             holds=lambda u: bool(u) and all(isinstance(a, int) and a > 0
                                             for a in u.values()),
             note="Codex's own total, the sum over the run's threads, which covers every step: "
                  'what a run that ended by itself is charged; a run cut short is charged it '
                  'plus a step in flight on each of its threads, and one not seen exited at '
                  'least its share (review of L3, round 3)'
             if HARNESS == 'codex' else
             "OpenCode's last model step; charged times the steps it could have taken")

    record['owner_service_after'] = opencode_live_run.owner_service()
    if record.get('mutant') == 'owner-service-absent':
        # No OpenCode service of the owner's to see, on any machine.
        record['owner_service_before'] = record['owner_service_after'] = []
    rows.add("The owner's OpenCode service was untouched",
             dict(before=record['owner_service_before'], after=record['owner_service_after']),
             'the same processes before and after',
             # Nothing listed before or after observed nothing (review of L3,
             # round 3, C3-4): [] == [] is not "untouched".
             holds=lambda o: None if not o['before'] and not o['after']
             else o['before'] == o['after'],
             note='the owner\'s own OpenCode service, by process; none running is inconclusive')
    if HARNESS == 'codex':
        # Every run's own before and after of the owner's config.toml. A
        # thread in a writable workspace trusts that project, which every run
        # discloses (M2); anything else would be a change nobody asked for.
        diffs = {i: next((e.get('diff') for e in host.get(i, [])
                          if e['kind'] == 'config_after'), None) for i in everyone
                 if views.get(i)}
        rows.add("The owner's Codex configuration changed only by the fixture's trust entry",
                 diffs,
                 'for every run, nothing removed or changed, nothing outside the project '
                 'tables, and any project added is under the fixture root',
                 holds=lambda d: all(
                     x is not None and not x['projects_removed'] and not x['projects_changed']
                     and x['other_changes'] is False
                     and all(a['location'] == 'fixture' for a in x['projects_added'])
                     for x in d.values()))
        return
    record['sessions_after'] = live.session_listing(state['repo'], rehearse)
    rows.add('PIO deleted no session',
             dict(before=(record['sessions_before'] or {}).get('entry_count'),
                  after=(record['sessions_after'] or {}).get('entry_count'),
                  deleted_nothing=live.deleted_nothing(record['sessions_before'],
                                                       record['sessions_after'])),
             'nothing that was listed before is missing after',
             # With nothing listed before, nothing could be deleted: that
             # cannot fail, so it is not a pass either (review 47).
             holds=lambda o: None if not o['before'] else o['deleted_nothing'] is True,
             live_only=True)


def names_of(record):
    """The ledger name of the lead in a receipt: its attempt's, if any."""
    return f"{record['attempt']}/{LEAD}" if record.get('attempt') else LEAD


def read_ledger(path):
    if path.exists():
        return json.loads(path.read_text())
    return {'cap': live.CAP, 'stop_at': live.STOP_AT,
            'measure': 'every token counter the harness reports, summed', 'runs': {}}


def sequence_charged(book):
    return sum(e.get('charged') or 0 for e in book['runs'].values()
               if e.get('sequence') == SEQUENCE)


def ledger_line(**line):
    """One ledger line. Codex's ledger totals its lines' `tokens`, so each
    of this runner's Codex lines carries what it charges there too, and the
    cap check M2 made counts L3."""
    if HARNESS == 'codex':
        line['tokens'] = line.get('charged') or 0
    return line


def write_ledger(path, book):
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(book, indent=2) + '\n')
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)


def reserve(path, names, at):
    """Each run's worst-case share, in the ledger before anything can spend."""
    book = read_ledger(path)
    lines = {}
    for identity in RUNS:
        line = ledger_line(sequence=SEQUENCE, model=MODEL, at=at, charge_basis='reserved',
                           charged=LEAD_SHARE if identity == LEAD else CHILD_SHARE,
                           why='the worst case this run could spend; the exit path replaces '
                               'it with the charge, and a runner killed from outside leaves it')
        book['runs'][names[identity]] = line
        lines[names[identity]] = line
    write_ledger(path, book)
    return lines


def charge(path, names, usage, at, authoritative, lead_submitted=True, probes=None):
    """Replace every reservation with its charge, and report the sequence
    total and whether a stop fired. Charged, never observed; unknown is never
    zero. A run the service says was refused, or never submitted, is charged
    nothing; where the service could not be asked, a run with no usage keeps
    its reservation — unless the lead was never submitted, so nothing could
    have spent: then a reservation is replaced with nothing (review of L3,
    F4). A name with no reservation gets no line at all, so a run that never
    started leaves its name free."""
    book = read_ledger(path)
    lines = {}
    # A run under this lead the plan did not name is charged on a line of
    # its own, named as a planned child would be (review of L3, round 3,
    # SPEND-1); it has no reservation to replace.
    names = dict(names, **{i: f'{names[LEAD]}{i[len(LEAD):]}' for i in usage
                           if i not in names and in_family(i)})
    for identity in (*RUNS, *[i for i in usage if i not in RUNS]):
        entry = usage.get(identity)
        base = dict(sequence=SEQUENCE, model=MODEL, at=at)
        reserved = (book['runs'].get(names[identity]) or {}).get('charge_basis') == 'reserved'
        if entry is None and not reserved:
            continue
        if entry is None and not authoritative and lead_submitted:
            continue
        if entry is None and not authoritative:
            line = dict(base, charged=0, charge_basis='never submitted: the lead was never '
                                                      'submitted, so nothing could spend')
        elif entry is None:
            line = dict(base, charged=0, charge_basis='never submitted')
        elif entry['basis'] in ('refused before any model call', NO_SUCH_EXECUTION):
            line = dict(base, charged=0, charge_basis=entry['basis'])
        elif HARNESS == 'codex':
            line = dict(base, charged=entry['charged'], charge_basis=entry['basis'],
                        observed_total_tokens=entry.get('reported_total'),
                        observed_is="the thread's total, which covers every step",
                        meter_estimate=entry.get('meter_estimate'))
        else:
            line = dict(base, charged=entry['charged'], charge_basis=entry['basis'],
                        observed_total_tokens=entry.get('reported_last_step'),
                        observed_is='the last model step of the turn',
                        measured_steps=entry.get('measured_steps'),
                        bound=entry.get('bound'), steps_at_most=entry.get('steps_at_most'),
                        meter_estimate=entry.get('meter_estimate'))
            if entry.get('why'):
                line['why'] = entry['why']
            if entry['basis'] == 'allowance':
                line.update(usage='unknown', why=(
                    "no usage reported; charged the run's whole share" if HARNESS == 'codex'
                    else "no usage reported; charged the run's meter bound, and never less "
                         f'than {opencode_live_run.CANCEL_ALLOWANCE}'))
        line = ledger_line(**line)
        book['runs'][names[identity]] = line
        lines[names[identity]] = line
    # A probe the service should have refused and admitted instead ran off
    # the ledger: it gets a line of its own, at its share (review of L3,
    # round 2, SB-5).
    for probe, entry in (probes or {}).items():
        line = ledger_line(sequence=SEQUENCE, model=MODEL, at=at, charged=entry['charged'],
                           charge_basis=entry['basis'])
        book['runs'][f'{names[LEAD]}:{probe}'] = line
        lines[f'{names[LEAD]}:{probe}'] = line
    write_ledger(path, book)
    total = sequence_charged(book)
    ledger = CODEX['ledger'] if HARNESS == 'codex' else 'MiniMax ledger'
    cap = 'codex' if HARNESS == 'codex' else 'minimax'
    return {'ledger': 'rehearsal ledger' if 'rehearsal' in path.name else ledger,
            'lines': lines, 'reservations_left': sorted(
                k for k, v in book['runs'].items()
                if k in names.values() and v.get('charge_basis') == 'reserved'),
            'sequence_charged': total, 'sequence_cap': SEQUENCE_CAP,
            'sequence_stop': SEQUENCE_STOP, 'stop_reached': total >= SEQUENCE_STOP,
            f'{cap}_charged': live.cumulative(book), f'{cap}_cap': live.CAP,
            'measured_against': 'charged'}


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def watchdog(runner, daemon, root):
    """Stop the service if the runner dies without its exit path.

    Started in its own session by `Service.start`, so whatever kills the
    runner does not kill it. It leaves quietly when the service is gone or
    the runner finished (`runner-done`, or a rehearsal's root released).
    Otherwise it stops the lead's tool, cancels every run still going, waits
    out the host's kill, stops the daemon, and writes `watchdog.json`. The
    ledger keeps its reservations: nothing here knows what was spent. It holds
    a no-sleep assertion of its own for as long as it lives, since the
    runner's died with the runner (review 46).
    """
    if shutil.which('caffeinate'):
        subprocess.Popen(['caffeinate', '-i', '-s', '-w', str(os.getpid())],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
    while True:
        if not alive(daemon) or not root.exists():
            return
        if not alive(runner):
            if (root / 'runner-done').exists():
                return
            break
        time.sleep(0.5)
    record = dict(runner_pid=runner, runner_gone_at=now(), daemon_pid=daemon)
    (root / 'lead-stop').touch()
    try:
        config = json.loads((root / 'service.json').read_text())
        owner = Caller(root / 'socket' / 'public.sock',
                       config['protocol']['credentials'][0]['credential'],
                       features=FEATURES, execution_features=EXECUTION_FEATURES)
        try:
            # Every execution under this lead, whatever its name (review of
            # L3, round 3, SPEND-1).
            family = sorted({*RUNS, *(e['subject']['id'] for e in events(owner)
                                      if in_family(e['subject']['id']))})
            record['cancelled'] = {
                i: cancel(owner, i, 'watchdog') for i in family
                if (view(owner, i) or {}).get('admission') == 'admitted'
                and (view(owner, i) or {}).get('runtime') != 'exited'}
            end = time.monotonic() + 60
            while time.monotonic() < end and any(
                    (view(owner, i) or {}).get('runtime') not in (None, 'exited')
                    for i in record['cancelled']):
                time.sleep(0.5)
            record['runtimes'] = {i: (view(owner, i) or {}).get('runtime') for i in family}
        finally:
            owner.close()
    except Exception as caught:
        record['error'] = repr(caught)[:500]
    try:
        os.kill(daemon, signal.SIGKILL)
    except ProcessLookupError:
        pass
    end = time.monotonic() + 10
    while alive(daemon) and time.monotonic() < end:
        time.sleep(0.2)
    record.update(daemon_stopped=not alive(daemon), finished_at=now())
    (root / 'watchdog.json').write_text(json.dumps(record, indent=2) + '\n')


def runner_killed(args):
    """The one exit no exit path survives: SIGKILL, from outside, once the
    desk has answered. What must be left is the reservation in the ledger,
    no receipt, and a service the watchdog has stopped."""
    command_line = [sys.executable, str(Path(__file__).resolve()), '--rehearse',
                    '--mutant', 'runner-killed', '--inner', '--out', str(args.out),
                    '--plan', LEAD]
    child = subprocess.Popen(command_line, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True)
    root, answered = None, False
    for line in child.stdout:
        print(line, end='', flush=True)
        if line.startswith('ROOT '):
            root = Path(line.split(' ', 1)[1].strip())
        if line.startswith('DESK answered'):
            answered = True
            break
    os.kill(child.pid, signal.SIGKILL)
    child.wait()
    assert root is not None and answered, 'the runner never reached the desk'
    try:
        end = time.monotonic() + 150
        while not (root / 'watchdog.json').exists() and time.monotonic() < end:
            time.sleep(0.5)
        stopped = json.loads((root / 'watchdog.json').read_text())
        print(f"watchdog: {json.dumps(stopped)[:400]}")
        assert stopped.get('daemon_stopped') is True, stopped
        assert LEAD in stopped.get('cancelled', {}), stopped
        assert not args.receipt.exists(), 'a receipt was written by a runner that was killed'
        book = read_ledger(args.receipt.with_name(args.receipt.stem + '-ledger.json'))
        held = {i: (book['runs'].get(i) or {}) for i in RUNS}
        assert all(line.get('charge_basis') == 'reserved' for line in held.values()), held
        assert held[LEAD]['charged'] == LEAD_SHARE and all(
            held[f'{LEAD}.{c}']['charged'] == CHILD_SHARE for c in CHILDREN), held
    finally:
        if root is not None:
            case_cleanup.release(root, remove=True)
    wanted = MUTANTS['runner-killed']
    print(f"mutant runner-killed: dies on {wanted!r}: no receipt, every reservation "
          f"stands ({LEAD_SHARE} + 2 x {CHILD_SHARE}), and the watchdog stopped the service")
    raise SystemExit(1)


def charge_selftest():
    """The Codex charge's corners that no play reaches, on made-up runs:
    one stopped for silence past its share (review of L3, round 3,
    SPEND-6), one the host's own deadline interrupted and the runner never
    stopped (SPEND-6), one stopped past its share (uncapped, SPEND-7), one
    with a step past the bound (SPEND-4), and one with a sub-agent
    (SPEND-2)."""
    select_plan('L3')
    step = CODEX['in_flight']

    def run(reported, **fields):
        gauge = CodexMeter(f'{LEAD}.alpha', CHILD_CEILING)
        gauge.total = reported
        for key, value in fields.items():
            setattr(gauge, key, value)
        view = dict(admission='admitted', runtime='exited',
                    usage=dict(observations=[dict(amount=reported)]))
        return view, gauge

    def charged(view, gauge, stopped=()):
        views = {f'{LEAD}.alpha': view}
        meters = {f'{LEAD}.alpha': gauge}
        entry = codex_usage(views, meters, {f'{LEAD}.alpha'}, stopped)[f'{LEAD}.alpha']
        return entry['charged'], entry['basis']
    cases = [
        ('silenced past its share', run(95_000, silenced=True), (), (95_000 + step, SILENT_BASIS)),
        ('interrupted by the host', run(100_000, turn_status='interrupted'), (),
         (100_000 + step, STOPPED_BASIS)),
        ('stopped past its share', run(100_000), (f'{LEAD}.alpha',), (100_000 + step, STOPPED_BASIS)),
        ('a step past the bound', run(60_000, largest_step=40_000), (f'{LEAD}.alpha',),
         (100_000, STOPPED_BASIS)),
        ('with a sub-agent', run(40_000, subagents=[dict(thread_id='t')]), (f'{LEAD}.alpha',),
         (40_000 + 2 * step, STOPPED_BASIS)),
    ]
    for name, (view, gauge), stopped, want in cases:
        got = charged(view, gauge, stopped)
        assert got == want, (name, got, want)
    print(f'charge selftest: {len(cases)} corners charged as the rules say')


def main():
    if '--watch-runner' in sys.argv:
        watch_args = argparse.ArgumentParser()
        watch_args.add_argument('--watch-runner', type=int, required=True)
        watch_args.add_argument('--daemon', type=int, required=True)
        watch_args.add_argument('--root', type=Path, required=True)
        watch_args.add_argument('--plan', choices=sorted(PLANS), default='L1')
        given = watch_args.parse_args()
        select_plan(given.plan)
        return watchdog(given.watch_runner, given.daemon, given.root)
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/lead-run')
    parser.add_argument('--rehearse', action='store_true',
                        help='the labeled fake, no tokens')
    parser.add_argument('--desk', help="the owner's words confirming they are at the "
                                       'desk; required for a live run, quoted in the receipt')
    parser.add_argument('--attempt', help='a new name for a live run the ledger already holds')
    parser.add_argument('--plan', choices=sorted(PLANS), default='L1',
                        help='L1, or L1b: the same shape, with a read that waits '
                             'and a request that comes to the desk; or L3, on Codex')
    parser.add_argument('--mutant', choices=sorted({*MUTANTS, *INCONCLUSIVE_MUTANTS,
                                                    *HOLDING_MUTANTS}))
    parser.add_argument('--relay', action='store_true',
                        help='rehearsal only: the desk waits for answer files, as live, '
                             'so a relay can answer them')
    parser.add_argument('--charge-selftest', action='store_true',
                        help="check the Codex charge's corners on made-up runs, and exit")
    parser.add_argument('--inner', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.charge_selftest:
        return charge_selftest()
    select_plan(args.plan)
    if args.mutant and args.plan not in PLAN_MUTANTS.get(args.mutant, {args.plan}):
        raise SystemExit(f'mutant {args.mutant} belongs to plans '
                         f'{sorted(PLAN_MUTANTS[args.mutant])}')
    if not args.rehearse and not args.desk:
        raise SystemExit('a live run needs --desk: the owner confirms they are at the '
                         'desk, and their words go in the receipt')
    if args.mutant and not args.rehearse:
        raise SystemExit('a mutant is a rehearsal; it never spends a token')
    if args.relay and not args.rehearse:
        raise SystemExit('--relay is a rehearsal; a live desk always waits for its files')
    args.out.mkdir(parents=True, exist_ok=True)
    rehearsal = 'rehearsal' if LEAD == 'L1' else f'rehearsal-{LEAD}'
    name = (rehearsal if args.rehearse else (args.attempt or LEAD)) + \
        (f'-mutant-{args.mutant}' if args.mutant else '') + ('-relay' if args.relay else '')
    args.receipt = args.out / f'{name}.json'
    if args.mutant == 'runner-killed' and not args.inner:
        args.receipt.unlink(missing_ok=True)
        return runner_killed(args)
    args.receipt.unlink(missing_ok=True)
    try:
        record = run(args)
    except BaseException as error:
        if args.mutant in ('setup-fails', 'helper-elsewhere', 'experimental-feature-on',
                           'experimental-alias-on', *UNMETERED_REFUSED):
            root = getattr(args, 'root', None)
            assert root is not None and not Path(root).exists(), (
                f'the setup failed and left its tree behind: {root}')
            assert not args.receipt.exists(), 'a receipt for a run that never started'
            if args.mutant == 'helper-elsewhere':
                assert "points a helper at a provider other than the plan's" in str(error) \
                    and 'small_model' in str(error), error
            if args.mutant == 'experimental-feature-on':
                assert 'exec_permission_approvals (by exec_permission_approvals)' in str(error), error
            if args.mutant == 'experimental-alias-on':
                assert 'exec_permission_approvals (by request_permissions)' in str(error), error
            if args.mutant in UNMETERED_REFUSED:
                # Refused for its own feature, by the key that decided it,
                # and for no other.
                feature, why = UNMETERED_REFUSED[args.mutant]
                named = {f for f in UNMETERED if f'{f} (' in str(error)}
                wanted_features = set(UNMETERED) if feature == 'all five' else {feature}
                assert named == wanted_features and why in str(error), error
            print(f'mutant {args.mutant}: dies on {MUTANTS[args.mutant]!r}: '
                  f'{type(error).__name__}, and the tree is gone: {redact(str(error))[:600]}')
            raise SystemExit(1)
        if not args.mutant or not args.receipt.exists():
            raise
        # The exit path wrote this, or nothing did.
        print(f'the run ended with {type(error).__name__}: {error}')
        record = json.loads(args.receipt.read_text())
    for row in record['rows']:
        mark = {True: 'ok  ', False: 'FAIL', None: 'n/a '}[row['holds']]
        if row['kind'] == 'record':
            mark = 'rec '
        print(f"{mark} {row['row']}: {json.dumps(row['observed'])[:140]}")
    unproven = [r['row'] for r in record['rows'] if r['holds'] is None and r['kind'] == 'row']
    print(f"\n{len(record['rows'])} rows; not provable here: {unproven}")
    print(f"charged: {json.dumps(record['charge'])[:400]}")
    if args.mutant in HOLDING_MUTANTS:
        wanted = HOLDING_MUTANTS[args.mutant]
        row = next(r for r in record['rows'] if r['row'] == wanted)
        assert row['holds'] is True, f'mutant {args.mutant}: {wanted!r} did not hold: {row}'
        assert not record['failed'], record['failed']
        if args.mutant == 'qualified-as-committed':
            print(f'mutant {args.mutant}: holds on {wanted!r}, against the committed record '
                  f"for {row['observed']['ran']['pinned']['version']}")
            return
        if args.mutant == 'overrides-on':
            # Every feature off per launch, over an owner's configuration
            # that turns each on; the whole set on every thread; and so no
            # agent spawned and no memory pipeline wrote.
            routes = {f: u['route'] for f, u in record['unmetered']['features'].items()}
            assert routes == {f: f'per launch, under {FEATURES_OFF_REHEARSAL!r}'
                              for f in UNMETERED}, routes
            assert all(u['off'] is True for u in record['unmetered']['features'].values())
            assert all(sent == FEATURES_OFF for sent in record['features_off_sent'].values()) \
                and len(record['features_off_sent']) == len(RUNS), record['features_off_sent']
            for name in ("Codex's memory pipeline wrote nothing during the run",
                         'No run took a turn of its own after its turn ended'):
                held = next(r for r in record['rows'] if r['row'] == name)
                assert held['holds'] is True, held
            retries = next(r for r in record['rows'] if r['row'] == 'Stream retries Codex reported')
            assert retries['observed'] == {LEAD: 0, f'{LEAD}.alpha': 0, f'{LEAD}.beta': 2}, retries
            print(f'mutant {args.mutant}: holds on {wanted!r}, with all {len(UNMETERED)} '
                  f"features off per launch on {len(record['features_off_sent'])} threads")
            return
        at_limit = [r for r in row['observed'] if r['runtime'] not in ('exited', None)
                    and (r['seconds'] or 0) >= READ_WAIT]
        exited_long = [r for r in row['observed'] if r['runtime'] == 'exited'
                       and (r['seconds'] or 0) >= WAITED]
        # Held by the limit and by nothing else, or it proves nothing.
        assert at_limit and not exited_long, row['observed']
        print(f'mutant {args.mutant}: holds on {wanted!r}, through a read that came back '
              f"{at_limit[0]['runtime']} after {at_limit[0]['seconds']} s")
        return
    if args.mutant in INCONCLUSIVE_MUTANTS:
        wanted = INCONCLUSIVE_MUTANTS[args.mutant]
        row = next(r for r in record['rows'] if r['row'] == wanted)
        assert row['holds'] is None and not row['proven'], (
            f'mutant {args.mutant} left {wanted!r} at {row["holds"]}, not inconclusive')
        print(f'mutant {args.mutant}: dies on {wanted!r}: inconclusive, not passed')
        raise SystemExit(1)
    if args.mutant:
        wanted = MUTANTS[args.mutant]
        assert wanted in record['failed'], (
            f'mutant {args.mutant} did not fail its row {wanted!r}; failed: {record["failed"]}')
        # The receipt is on disk, and it charged every run that ran.
        assert args.receipt.exists(), 'no receipt was written'
        charged = record['charge']['lines']
        ran = {i for i, u in record['usage'].items()
               if u['basis'] not in ('refused before any model call', NO_SUCH_EXECUTION)}
        assert ran <= set(charged) and all(charged[i]['charged'] > 0 for i in ran), (
            ran, charged)
        assert not record['charge']['reservations_left'], record['charge']
        if args.mutant == 'lead-loops':
            stops = [s for s in record.get('ceiling_stops', []) if s['run'] == LEAD]
            assert stops and any('tool calls' in w for w in stops[0]['why']), stops
            held = [e for e in record['tool_log'] if e.get('event') == 'held']
            assert held, 'the tool did not hold the call past its ceiling'
        if args.mutant == 'stop-ignored':
            # Still running when its usage was read, and charged what it
            # reported plus a step: past its share, which the receipt says.
            alpha = record['usage'][f'{LEAD}.alpha']
            assert alpha['basis'] == NOT_EXITED_BASIS, alpha
            assert alpha['charged'] >= alpha['reported_total'] + CODEX['in_flight'] > CHILD_SHARE, alpha
        if args.mutant == 'interrupted':
            assert record['error']['type'] == 'KeyboardInterrupt', record.get('error')
            assert record.get('stopped_on_exit'), 'nothing was stopped on the way out'
        if args.mutant == 'child-overspends':
            # alpha was stopped while its answering step was still being
            # produced, and Codex reports no usage for a step cut short
            # before its response completed (0.157.0 records usage only on a
            # completed response; review of L3, round 2, SB-4): its report is
            # its first step alone, and the step in flight is the charge's.
            reported = record['usage'][f'{LEAD}.alpha']['reported_total']
            reaching = -(-CHILD_CEILING // CODEX['in_flight']) * CODEX['in_flight']
            assert reported == reaching, record['usage'][f'{LEAD}.alpha']
            # Cut off before it answered, alpha answered nothing: its
            # preamble's 30 is not taken for a count (review of L3, round 2,
            # V-7).
            counts = next(r for r in record['rows']
                          if r['row'] == 'Each child reported the true count')['observed']
            assert counts[CHILDREN['alpha']] is None, counts
            # The stop is proven here, where one is made: it reached Codex,
            # and the run was charged its step in flight, within its share.
            for name in ('Every stop the runner made reached Codex',
                         "Every run's charge covers what it could have spent",
                         "Every run's charge stayed within its reserved share"):
                row = next(r for r in record['rows'] if r['row'] == name)
                assert row['holds'] is True, row
        if args.mutant in ('usage-suppressed', 'asked-silent'):
            silenced = {s['run'] for s in record.get('ceiling_stops', [])
                        if any('no usage reported' in w for w in s['why'])}
            assert {LEAD, f'{LEAD}.alpha'} <= silenced, record.get('ceiling_stops')
            assert all(record['usage'][i]['charged'] == (LEAD_SHARE if i == LEAD else CHILD_SHARE)
                       for i in RUNS), record['usage']
        if args.mutant in ('lead-tool-error-past-hold', 'lead-shell-past-hold'):
            # Stopped for the right reason, and every result it got was
            # checked against its hold: the error was held like a result, the
            # shell's result met by a stop. Within its ceiling plus one step.
            why = 'withheld' if args.mutant == 'lead-tool-error-past-hold' else 'never saw'
            stops = [s for s in record.get('ceiling_stops', []) if s['run'] == LEAD]
            assert stops and any(why in w for w in stops[0]['why']), stops
            row = next(r for r in record['rows'] if r['row'] ==
                       'Every result the lead got was checked against its hold')
            assert row['holds'] is True, row
            assert record['usage'][LEAD]['reported_total'] <= HOLD_ABOVE + 2 * CODEX['in_flight'], \
                record['usage'][LEAD]
        if args.mutant == 'lead-heavy':
            assert record['usage'][LEAD]['reported_total'] <= HOLD_ABOVE + 2 * CODEX['in_flight'], \
                record['usage'][LEAD]
            # Stopped because its tool withheld a result, and within its
            # share: 120,000 reported at the withheld call, the step that
            # made it in flight, and no step after.
            stops = [s for s in record.get('ceiling_stops', []) if s['run'] == LEAD]
            assert stops and any('withheld' in w for w in stops[0]['why']), stops
            assert [e for e in record['tool_log'] if e.get('result_withheld')], 'nothing withheld'
            for name in ("Every run's charge covers what it could have spent",
                         "Every run's charge stayed within its reserved share"):
                row = next(r for r in record['rows'] if r['row'] == name)
                assert row['holds'] is True, row
            assert record['usage'][LEAD]['reported_total'] <= LEAD_SHARE, record['usage'][LEAD]
        if args.mutant == 'child-renamed':
            # The tool refused both before anything reached the service, and
            # nothing under another name ran.
            starts = [e for e in record['tool_log'] if e.get('event') == 'tool_call'
                      and e.get('tool') == 'start_run']
            assert starts and all(((e.get('result') or {}).get('refused') or {}).get('code')
                                  == 'not_a_planned_child' for e in starts), starts
            assert not record.get('unplanned_runs'), record.get('unplanned_runs')
        if args.mutant == 'child-renamed-unchecked':
            # Run under names the plan does not have, and still metered,
            # stopped at the ceiling, cancelled and charged on lines of their
            # own (review of L3, round 3, SPEND-1).
            renamed = {f'{LEAD}.alpha_run', f'{LEAD}.beta_run'}
            assert renamed <= {u['run'] for u in record.get('unplanned_runs', [])}, \
                record.get('unplanned_runs')
            alpha = f'{LEAD}.alpha_run'
            assert any(s['run'] == alpha for s in record.get('ceiling_stops', [])), \
                record.get('ceiling_stops')
            spent = record['usage'][alpha]
            assert spent['charged'] >= spent['reported_total'] + CODEX['in_flight'], spent
            lines = record['charge']['lines']
            assert all(lines[f'{names_of(record)}{i[len(LEAD):]}']['charged']
                       == record['usage'][i]['charged'] > 0 for i in renamed), lines
        def row_of(name):
            return next(r for r in record['rows'] if r['row'] == name)
        alpha = f'{LEAD}.alpha'
        spent = record['usage'].get(alpha, {})
        stops = [s for s in record.get('ceiling_stops', []) if s['run'] == alpha]
        if args.mutant == 'subagent-spawned':
            # Stopped when its agent appeared, charged a step in flight on
            # each of its two threads, and both rows that judge that hold.
            assert stops and any('sub-agent' in w for w in stops[0]['why']), stops
            assert spent['subagents'] and spent['basis'] == STOPPED_BASIS, spent
            assert spent['charged'] >= (spent['reported_total'] or 0) + 2 * CODEX['in_flight'], spent
            for name in ('Every run that spawned a sub-agent was stopped when it appeared',
                         "Every run's charge covers what it could have spent"):
                assert row_of(name)['holds'] is True, row_of(name)
            assert row_of('No run spawned a sub-agent')['observed']['threads'][alpha], \
                row_of('No run spawned a sub-agent')
        if args.mutant == 'subagent-not-stopped':
            assert not any('sub-agent' in w for s in stops for w in s['why']), stops
        if args.mutant == 'subagent-uncounted':
            floor = row_of("Every run's charge covers what it could have spent")['observed']
            assert floor['runs'][alpha]['charged'] < floor['runs'][alpha]['at_least'], floor
        if args.mutant == 'step-past-in-flight':
            assert stops and any('the bound assumes' in w for w in stops[0]['why']), stops
            assert spent['charged'] >= spent['reported_total'] + 40_000, spent
        if args.mutant == 'stopped-past-share':
            # Stopped at 60,000, answered anyway, and charged its report and
            # a step, past its share, uncapped.
            assert spent['reported_total'] == (CHILD_CEILING // CODEX['in_flight'] + 2) \
                * CODEX['in_flight'], spent
            assert spent['charged'] == spent['reported_total'] + CODEX['in_flight'] > CHILD_SHARE, spent
            assert row_of("Every run's charge covers what it could have spent")['holds'] is True
        if args.mutant == 'stopped-charge-capped':
            assert spent['charged'] == CHILD_SHARE, spent
            assert row_of("Every run's charge stayed within its reserved share")['holds'] is True
        if args.mutant == 'deadline-interrupted':
            # The host's own deadline cut alpha short, not the runner: charged
            # as a stopped run, and the floor holds.
            assert not stops and spent['cut_short'] and spent['basis'] == STOPPED_BASIS, spent
            assert spent['charged'] == spent['reported_total'] + CODEX['in_flight'], spent
            assert row_of("Every run's charge covers what it could have spent")['holds'] is True
        if args.mutant == 'lead-asked-by-user-input':
            # What was asked, never how: no question, header or option text
            # reaches the receipt (review of L3, round 3, R3-HC-5).
            assert 'SENTINEL' not in args.receipt.read_text(), 'a question text leaked'
        if args.mutant == 'meter-dies':
            # Every run still going was cancelled by the meter itself, within
            # seconds of its failing, and alpha never reached the ceiling it
            # passed unstopped at 866a903.
            died = record['meter_died']
            assert set(died['cancelled']) >= {LEAD, alpha}, died
            took = (time.mktime(time.strptime(died['done_at'], '%Y-%m-%dT%H:%M:%SZ'))
                    - time.mktime(time.strptime(died['at'], '%Y-%m-%dT%H:%M:%SZ')))
            assert took <= 5, died
            assert (spent.get('reported_total') or 0) < CHILD_CEILING, spent
            assert 'the meter stopped' in record['error']['message'], record['error']
        if args.mutant == 'probe-over-share':
            # Metered, stopped and charged past its share: its report plus
            # its step in flight, and the floor agrees.
            probe = record['probes_admitted'][f'{LEAD}.third']
            assert probe['charged'] == probe['seen'] + probe['allowance'] > CHILD_SHARE, probe
            assert row_of("Every run's charge covers what it could have spent")['holds'] is True
            assert record['charge']['lines'][f'{names_of(record)}:{LEAD}.third']['charged'] \
                == probe['charged'], record['charge']['lines']
        if args.mutant == 'late-step-after-halt':
            # The meter never saw the 40,000 step; the host's files did, and
            # so did the charge: its report plus that step in flight.
            assert record['meters'][alpha]['largest_step'] < 40_000, record['meters'][alpha]
            assert spent['host_files']['largest'] == 40_000, spent
            assert spent['charged'] == spent['reported_total'] + 40_000, spent
            assert row_of("Every run's charge covers what it could have spent")['holds'] is True
        if args.mutant == 'goal-continued':
            # Interrupted by the host, and charged as cut short: its report
            # plus a step in flight, which the floor agrees with.
            beta = record['usage'][f'{LEAD}.beta']
            assert beta['cut_short'] and beta['basis'] == STOPPED_BASIS, beta
            assert beta['charged'] == beta['reported_total'] + beta['allowance'], beta
            assert row_of("Every run's charge covers what it could have spent")['holds'] is True
            continued = row_of('No run took a turn of its own after its turn ended')['observed']
            assert continued['turns'][f'{LEAD}.beta'], continued
        if args.mutant == 'memory-pipeline-ran':
            changes = row_of("Codex's memory pipeline wrote nothing during the run")['observed']
            paths = {c['path'] for c in changes['changes']}
            assert {'memories/raw_memories.md', 'memories_1.sqlite'} <= paths, paths
            assert any(p.startswith('memories/rollout_summaries/sha256:') for p in paths), paths
            assert 'private-topic' not in args.receipt.read_text(), 'a session-given name leaked'
        if args.mutant == 'third-admitted':
            probe = record['probes_admitted'].get(f'{LEAD}.third')
            assert probe and probe['basis'].startswith(PROBE_BASIS) \
                and probe['charged'] == CHILD_SHARE, record['probes_admitted']
            assert record['charge']['lines'][f'{names_of(record)}:{LEAD}.third']['charged'] \
                == CHILD_SHARE, record['charge']['lines']
        if args.mutant == 'lead-submit-invalid':
            lead = record['usage'][LEAD]
            assert (lead['charged'], lead['basis']) == (0, NO_SUCH_EXECUTION), lead
            assert record['charge']['sequence_charged'] == 0, record['charge']
        if args.mutant == 'qualified-elsewhere':
            # Refused before anything was reserved, so nothing is charged.
            assert 'not the committed identity' in record['error']['message'], record['error']
            assert 'reserved' not in record and not record['charge']['lines'], record['charge']
            assert record['charge']['sequence_charged'] == 0, record['charge']
        if args.mutant == 'service-never-ready':
            # Found at once, not after the readiness wait; nothing reserved,
            # nothing charged, and the next attempt not blocked.
            assert 'exited with code' in record['error']['message'], record['error']
            assert 'reserved' not in record and not record['charge']['lines'], record['charge']
            assert record['charge']['sequence_charged'] == 0, record['charge']
        print(f'mutant {args.mutant}: dies on {wanted!r}, receipt and charge written')
        raise SystemExit(1)
    if record['failed']:
        raise SystemExit(f"FAILED ROWS: {record['failed']}")
    print('lead run: every row holds')


if __name__ == '__main__':
    main()
