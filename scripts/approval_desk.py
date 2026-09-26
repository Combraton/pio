#!/usr/bin/env python3
"""G2 — the approval walk, proven through a real host path.

`serve-fake` has no approvals, so this drives **`serve-opencode`** with the
labeled ACP fake behind it: a real host process, a real journal, the real
projection, and the public Unix API in front. Everything the walk needs is
read from `core.events.read` and `execution.inspect` — never from the host's
private events file, which no caller can read.

What it proves, in one service holding two runs:

1. **Two requests pending across two runs are ordered by their deadlines.**
   The deadline is not in `actions[]` — that shape is closed — so it rides in
   the payload of the `execution.runtime.changed` that already fires, under
   `pio.combraton.dev/approval`. Run 1 is submitted first and run 2 is due
   first, so arrival order and deadline order disagree and the walk has to
   use the deadline.
2. **A caller answers through `execution.respond_action`**, and the answer
   reaches the stream as `execution.action.answered` with
   `pio.combraton.dev/decision` naming the caller.
3. **An answer aimed at the wrong run is refused**, `not_found`, and leaves
   both runs as they were.
4. **A lapsed deadline is PIO's decision**, on the stream, with
   `decided_by: "pio"` — and the action stops being `pending`.
5. **A decline PIO made before any caller was asked** is the same kind of
   record, with its classification.
6. **A caller that ignores the namespaced keys sees what it saw before.**
   Proven by running the same two scenarios against the binary built at
   `a22c98c` and diffing.

For 4 and 5 the mutant is the defect itself: `--baseline-as-mutant` runs
passes 1 and 2 against the `a22c98c` binary, where `request_denied_by_default`
has no arm in the projection and a decline reaches only the private adapter
namespace, and requires both to fail.

`--mutant arrival-order` walks the runs in the order their requests arrived.
`--mutant cross-run` aims the misdirected answer at its **own** run, so a
refusal that came from a malformed envelope rather than from the run mismatch
would still look like a pass.
`--mutant unstripped` compares against `a22c98c` without removing the
namespaced keys, so "the only difference is the keys" is a claim the run can
fail.
"""
import argparse
import calendar
import contextlib
import json
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import opencode_host_matrix as matrix
import proof_harness
from opencode_host_matrix import ServiceCase, poll, release_live_cases

ROOT = Path(__file__).resolve().parents[1]
NS = 'pio.combraton.dev/'
APPROVAL = NS + 'approval'
DECISION = NS + 'decision'
BASELINE_COMMIT = 'a22c98c'
# Surfaced to the caller: inside the workspace, so PIO asks rather than decides.
ASKS = {'title': 'run a command', 'kind': 'execute',
        'input': {'command': 'git tag pio-approval-marker'}}
# Outside the workspace, so PIO declines it and no caller is ever asked.
DECLINES = {'title': 'read a file', 'kind': 'read',
            'input': {'file_path': '/etc/hosts'}}
INSTANT = re.compile(r'^\d{4}-\d\d-\d\dT[\d:.]+(Z|[+-]\d\d:\d\d)$')
TMPROOT = re.compile(r'/tmp/pio-oc-[A-Za-z0-9_]+')
SHA = re.compile(r'^[0-9a-f]{40}$')
UUID = re.compile(r'^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$')


# --- the Protocol event schema, read from the pin rather than restated -------

def event_schema():
    path = ROOT / 'crates/pio-protocol/vendor/schemas/core/1/common.schema.json'
    return json.loads(path.read_text())['$defs']['event']


def check_event(event, schema):
    """The vendored schema's own rules, applied to one event record.

    Only the three that matter here: the record is closed, the required
    fields are present, and `origin` decides whether `operation_ref` and
    `command_id` may appear at all. The last is the one a PIO-authored event
    can get wrong, and it is the reason this function exists.
    """
    allowed, required = set(schema['properties']), set(schema['required'])
    extra = set(event) - allowed
    assert not extra, f'{event["type"]}: unevaluatedProperties false, but {sorted(extra)}'
    assert required <= set(event), f'{event["type"]}: missing {sorted(required - set(event))}'
    named = {'operation_ref', 'command_id'} & set(event)
    if event['origin'] == 'command':
        assert named == {'operation_ref', 'command_id'}, \
            f'{event["type"]}: origin command without {sorted({"operation_ref","command_id"}-named)}'
    else:
        assert not named, \
            f'{event["type"]}: origin {event["origin"]} must not carry {sorted(named)}'


# --- the desk ----------------------------------------------------------------

@contextlib.contextmanager
def service(out, label, **scenario):
    """One service, released whether or not the pass that used it passed.

    The matrices wrap every case in a `finally` and release it there. These
    proofs did not: `cleanup()` sat on the success path, so a failing
    assertion left the store, the daemon and the harness behind until the
    process exited — and if the process was killed instead, for ever. Four
    stores survived on `/tmp` that way. A context manager makes the shape
    impossible to get wrong in the next pass someone adds.

    A cleanup that itself fails is not swallowed: a surviving process is a
    real failure, and Python shows it chained to whatever raised first.
    """
    case = ServiceCase(out, label, model=matrix.REQUESTED, scenario=scenario)
    try:
        yield case
    finally:
        case.cleanup()



def events_of(case, identity=None):
    """Every Protocol event this caller may see, from the fold."""
    items, cursor, payload = [], None, {'limit': 1000, 'from': 'start',
                                        'kinds': ['execution.execution']}
    schema = event_schema()
    with case.client() as client:
        while True:
            answer = client.query('core.events.read', payload)
            result = answer.get('result')
            assert result is not None, f'core.events.read refused: {answer}'
            for item in result['items']:
                assert 'gap' not in item, item
                check_event(item['event'], schema)
                items.append(item['event'])
            cursor = result.get('cursor')
            if not result.get('has_more'):
                break
            payload = {'limit': 1000, 'cursor': cursor,
                       'kinds': ['execution.execution']}
    if identity is not None:
        items = [e for e in items if e['subject']['id'] == identity]
    return items


def walk(events, order='deadline', assume=None):
    """The approval walk: every request still waiting, soonest due first.

    Built only from the stream. `actions[]` is where identity and state live;
    the deadline is not in it and cannot be, so it comes from the payload.
    """
    pending, over = {}, set()
    for event in events:
        approval = event['payload'].get(APPROVAL)
        if event['type'] == 'execution.runtime.changed' and approval:
            pending[approval['action_id']] = dict(
                approval, run=event['subject']['id'], arrived=event['sequence'])
        if event['type'] == 'execution.action.answered':
            pending.pop(event['payload']['action_id'], None)
        if event['type'] == 'execution.exit.observed':
            # A run that has exited cannot answer anything, so its requests
            # leave the walk whether or not anyone decided them. Without this
            # a run PIO stopped at its own delivery deadline would sit on the
            # screen for ever with a countdown that means nothing.
            over.add(event['subject']['id'])
    rows = [row for row in pending.values() if row['run'] not in over]
    if order == 'arrival':
        rows.sort(key=lambda r: r['arrived'])
    else:
        rows.sort(key=lambda r: r['arrived'])
        # Soonest due first, and a request that never becomes due sorts
        # after every one that does.
        rows.sort(key=lambda r: (due(r, assume) is None, due(r, assume) or 0))
    for row in rows:
        row['due'] = due(row, assume)
    return rows


def due(row, assume=None):
    """When PIO will decide, if nobody does — or `None` when it never will.

    Every release harness sets one now; `None` stays the honest answer for
    a request that carries no deadline, rather than an invented countdown.
    `assume` substitutes a default for a missing one.

    Seconds since the epoch. `requested_at` is UTC, so it is read as UTC:
    `time.mktime` read it as local time, which moved every deadline by the
    machine's offset and changed no order, so no proof noticed. The Rust
    port (`pio_client::walk`) reads it as UTC, and the parity check needs
    the same number from both.
    """
    seconds = row.get('answer_deadline_seconds')
    if seconds is None:
        if assume is None:
            return None
        seconds = assume
    stamp = time.strptime(row['requested_at'][:19], '%Y-%m-%dT%H:%M:%S')
    return calendar.timegm(stamp) + seconds


def decisions(events):
    """Every decision on the stream, in stream order, whoever made it.

    One shape for all of them: the caller's answer, PIO's lapse and decline,
    Codex settling a request itself (`decided_by: harness`) and a request
    that ended with its turn (`decided_by: nobody`). The last two, and PIO's
    own, carry `decision: null` and `sent: false` where nothing was sent;
    the record is shown as the service wrote it. A payload without the key
    decided nothing this walk can name, so its decider is `unknown`.
    """
    out = []
    for event in events:
        if event['type'] != 'execution.action.answered':
            continue
        record = event['payload'].get(DECISION)
        known = record or {}
        out.append(dict(run=event['subject']['id'],
                        action_id=event['payload'].get('action_id'),
                        sequence=event['sequence'], origin=event['origin'],
                        decided_by=known.get('decided_by', 'unknown'),
                        decision=known.get('decision'), sent=known.get('sent'),
                        basis=known.get('basis'), record=record))
    return out


def host_kinds(case):
    """Every kind the hosts recorded, per invocation file.

    Read **only** when an assertion has already failed. The proof itself
    never opens this file: a caller cannot, so neither may the walk.
    """
    out = {}
    for path in sorted(Path(case.store).glob('opencode-*.events.jsonl')):
        out[path.name] = [json.loads(line)['kind']
                          for line in path.read_text().splitlines() if line.strip()]
    return out


def text(value):
    return value if isinstance(value, str) else repr(value)


def journal_state(case, identity):
    """The service's own record for one execution, read-only.

    Diagnostic only, and only after an assertion has failed: it answers which
    timeout ended a run, which the Protocol event alone does not.
    """
    import sqlite3
    with sqlite3.connect(f'file:{case.store}/journal.sqlite3?mode=ro', uri=True) as db:
        rows = list(db.execute('select value from protocol_projection where key = ?',
                               (f'execution/{identity}',)))
    if not rows:
        return None
    e = json.loads(rows[0][0])
    return dict(timeouts=e.get('submit', {}).get('payload', {}).get('timeouts'),
                created_at=e.get('created_at'), admitted_at=e.get('admitted_at'),
                last_observed=e.get('last_observed'),
                timeouts_passed=e.get('timeouts_passed'),
                delivery=e.get('view', {}).get('delivery'),
                runtime=e.get('view', {}).get('runtime'))


def answered(events, action_id):
    for event in events:
        if event['type'] == 'execution.action.answered' \
                and event['payload']['action_id'] == action_id:
            return event
    return None


def pass_one(out, mutant=None):
    """Two runs, two pending requests, one caller."""
    with service(out, 'desk', permission_request=ASKS) as case:
        case.start()
        # Run 1 goes first and is due last. Arrival order and deadline order
        # disagree on purpose: a walk that sorts by arrival gets them backwards.
        #
        # The numbers are not free. `action_answer_timeout_seconds` **is**
        # `timeouts.delivery`, so PIO's answer deadline and PIO's delivery
        # deadline are the same number for a run. While delivery is still
        # `pending` the delivery timeout is live, and on a loaded machine it can
        # end the run before the host's answer clock ever lapses — which leaves
        # the action `pending` on a dead run and nothing for this pass to read.
        # That is a real interaction, not a flake in the fix, and it cost one run
        # in five before the wait below was added. So: deadlines long enough that
        # the fold is never the slow part, and **acknowledged delivery asserted
        # before the walk is drawn**, which is what makes the delivery timeout
        # dead and the answer timeout the only clock left.
        #
        # Run 1 is submitted **and surfaced** before run 2 is submitted at
        # all, so arrival order is `['run-1', 'run-2']` no matter how the two
        # hosts are scheduled. Submitting both at once left it to chance, and
        # chance obliged: the `arrival-order` mutant passed once because the
        # two orders happened to agree that time. A mutant that survives
        # because of scheduling is not a mutant.
        for identity, seconds in (('run-1', 300), ('run-2', 75)):
            case.submit(identity=identity, delivery_timeout=seconds)
            view = poll(lambda i=identity: case.inspect(i),
                        lambda v: v['runtime'] == 'requires_action'
                        and v['delivery'] == 'acknowledged', seconds=120)
            assert 'delivery' not in view.get('timeouts_passed', []), view

        events = events_of(case)
        # The premise, asserted rather than hoped for: the two orders disagree.
        assert [r['run'] for r in walk(events, 'arrival')] == ['run-1', 'run-2'], \
            walk(events, 'arrival')
        rows = walk(events, 'arrival' if mutant == 'arrival-order' else 'deadline')
        assert len(rows) == 2, rows
        # 1. Ordered by deadline, which is only on the stream because G2 put it
        #    there. `actions[]` is closed and holds `requested_at` alone.
        assert [r['run'] for r in rows] == ['run-2', 'run-1'], \
            [(r['run'], r['answer_deadline_seconds']) for r in rows]
        assert [r['answer_deadline_seconds'] for r in rows] == [75, 300], rows
        for row in rows:
            # The option list the harness offered, measured live in M3b, and the
            # one PIO will send if nobody answers — chosen by kind, never by id.
            assert [o['kind'] for o in row['options']] == \
                ['allow_once', 'allow_always', 'reject_once'], row
            assert row['if_nobody_answers']['option_kind'] == 'reject_once', row
            assert row['if_nobody_answers']['always_option_taken'] is False, row
            # Measured, not assumed: this harness announces an `execute` call
            # with a command line and no path, so the resolver cannot place it
            # and says so rather than guessing. That is the value the walk shows.
            assert row['classification']['disposition'] == 'surface_as_action', row
            assert row['classification']['placement'] == 'not_classifiable', row
            entry = [a for a in case.inspect(row['run'])['actions']
                     if a['action_id'] == row['action_id']]
            assert entry and entry[0]['state'] == 'pending', entry
            assert 'answer_deadline_seconds' not in entry[0], entry

        soon, later = rows[0], rows[1]
        # 3. An answer aimed at the wrong run. The mutant aims it at its own.
        target = soon['run'] if mutant == 'cross-run' else later['run']
        view = case.inspect(target)
        refusal = case.respond(soon['action_id'], 'allow', view['revision'], identity=target)
        assert 'error' in refusal, refusal
        assert refusal['error']['data']['code'] == 'not_found', refusal
        for identity in ('run-1', 'run-2'):
            assert case.inspect(identity)['runtime'] == 'requires_action', identity

        # 2. The caller answers its own run.
        view = case.inspect(later['run'])
        accepted = case.respond(later['action_id'], 'deny', view['revision'],
                                identity=later['run'])
        assert accepted.get('result', {}).get('outcome', {}).get('state') == 'answered', accepted
        events = events_of(case)
        by_caller = answered(events, later['action_id'])
        assert by_caller is not None, 'the caller answered and the stream did not say so'
        assert by_caller['origin'] == 'command', by_caller
        assert by_caller['payload'][DECISION] == {'decided_by': 'caller', 'decision': 'deny'}, \
            by_caller

        # 4. Nobody answers run-2. PIO decides, and says so on the stream.
        poll(lambda: case.inspect(soon['run']), lambda v: v['runtime'] == 'exited', seconds=200)
        events = events_of(case)
        by_pio = answered(events, soon['action_id'])
        # Two different failures wear the same face, and telling them apart is
        # the difference between "the fix regressed" and "this run never got the
        # chance". Seen three times in twenty-eight runs, always under load,
        # never since — and not reproducible on demand even with the machine
        # deliberately saturated, so it is reported rather than claimed fixed.
        # The clock, by name. `execution.timeout.passed` carries
        # `{"timeout": <name>}`, and which of the five it is decides whether
        # this is the delivery deadline sharing its number with the answer
        # deadline or something else entirely. Guessing cost three rounds;
        # the name is now recorded rather than inferred.
        clocks = [text(e['payload'].get('timeout')) for e in events
                  if e['type'] == 'execution.timeout.passed'
                  and e['subject']['id'] == soon['run']]
        assert not clocks, (
            f'this run was ended by the {clocks} timeout before its answer '
            'clock lapsed, so there was no lapse to observe. Not a regression '
            'of the fix — at a22c98c the same run also ends with the action '
            f"pending. journal={journal_state(case, soon['run'])} "
            f'host={host_kinds(case)}')
        assert by_pio is not None, (
            'the deadline lapsed and the stream never said so — actions[] still '
            f"reads pending. action={soon['action_id']} "
            f"actions={case.inspect(soon['run'])['actions']} "
            f"events={[(e['type'], e['payload']) for e in events if e['subject']['id'] == soon['run'] and e['type'] != 'execution.runtime.changed']} "
            f"host={host_kinds(case)} "
            f"journal={journal_state(case, soon['run'])} "
            f"timeout_payloads={[e['payload'] for e in events if e['type'] == 'execution.timeout.passed']}")
        assert by_pio['origin'] == 'provider', by_pio
        assert 'operation_ref' not in by_pio and 'command_id' not in by_pio, by_pio
        decision = by_pio['payload'][DECISION]
        assert decision['decided_by'] == 'pio', decision
        assert decision['decision'] == 'deny', decision
        assert decision['basis'] == 'deadline_lapsed', decision
        assert decision['after_seconds'] == 75, decision
        assert decision['option_kind'] == 'reject_once', decision
        assert decision['always_option_taken'] is False, decision
        lapsed = [a for a in case.inspect(soon['run'])['actions']
                  if a['action_id'] == soon['action_id']][0]
        assert lapsed['state'] == 'answered', lapsed
        assert 'answered_at' in lapsed, lapsed
        # And the walk, drawn again from the same fold, is empty.
        assert walk(events) == [], walk(events)
        # What stays is who decided each: the caller one, PIO the other.
        decided = [(d['run'], d['decided_by'], d['origin']) for d in decisions(events)]
        assert decided == [(later['run'], 'caller', 'command'),
                           (soon['run'], 'pio', 'provider')], decided

        record = dict(runs=2, ordered_by='deadline',
                      order=[r['run'] for r in rows],
                      deadlines=[r['answer_deadline_seconds'] for r in rows],
                      wrong_run_answer=refusal['error']['data']['code'],
                      caller_decision=by_caller['payload'][DECISION],
                      pio_decision=decision,
                      events=[e['type'] for e in events])
        case.finish()
        return record, events


def pass_two(out):
    """A decline PIO made before any caller was asked."""
    with service(out, 'decline', permission_request=DECLINES) as case:
        case.start()
        case.submit(identity='run-3')
        poll(lambda: case.inspect('run-3'), lambda v: v['runtime'] == 'exited', seconds=120)
        events = events_of(case, 'run-3')
        # PIO was not asking, so no request ever became `requires_action`.
        assert not [e for e in events
                    if e['type'] == 'execution.runtime.changed'
                    and e['payload']['runtime'] == 'requires_action'], \
            [e['payload'] for e in events if e['type'] == 'execution.runtime.changed']
        settled = [e for e in events if e['type'] == 'execution.action.answered']
        assert len(settled) == 1, \
            "PIO declined and the stream never said so: the decline reached only " \
            f"the private adapter namespace. events={[e['type'] for e in events]}"
        event = settled[0]
        assert event['origin'] == 'provider', event
        assert 'operation_ref' not in event and 'command_id' not in event, event
        decision = event['payload'][DECISION]
        assert decision['decided_by'] == 'pio', decision
        assert decision['decision'] == 'deny', decision
        assert decision['basis'] == 'out_of_scope', decision
        assert decision['classification']['disposition'] == 'decline', decision
        assert decision['classification']['placement'] == 'outside_fixture', decision
        assert decision['reason'], decision
        actions = case.inspect('run-3')['actions']
        assert len(actions) == 1, actions
        assert actions[0]['state'] == 'answered', actions
        assert actions[0]['action_id'] == event['payload']['action_id'], actions
        # Closed shape: the classification is on the event, never in the view.
        assert set(actions[0]) <= {'action_id', 'owner', 'state', 'requested_at',
                                   'answered_at', 'response_effect'}, actions
        record = dict(run='run-3', decision=decision, actions=actions,
                      events=[e['type'] for e in events])
        case.finish()
        return record, events


def pass_two_actions(out, lapse_after=20):
    """Two actions on one run, both lapsing.

    The default-deny arm resolves an action from `action_seq`. With a single
    action that number is always 1, so nothing distinguished "settles the
    right entry" from "settles the only entry". A turn that asks about two
    things does: each has to reach its own row, and the run has to end with
    two answered actions and two decisions, not one twice.

    **This one has no baseline mutant, and the reason is worth stating.**
    `--baseline-as-mutant` runs its probes against the binary built at
    `a22c98c`, where the fake has no `permission_requests` knob at all — so
    the scenario would produce no actions and the probe would fail because
    the fixture is newer than the binary, not because the projection lacks
    the arm. A mutant that dies for the wrong reason proves nothing, so this
    check carries its discrimination in the assertions instead: two distinct
    `action_id`s in order, two rows in `actions[]`, both `answered`, each
    with its own `answered_at`. A projection that settled only the first, or
    settled the same row twice, fails every one of them.
    """
    with service(out, 'two-actions',
                 permission_requests=[ASKS, dict(ASKS, title='run another command',
                                                 input={'command': 'git tag pio-second'})]) \
            as case:
        case.start()
        case.submit(identity='run-1', delivery_timeout=lapse_after)
        poll(lambda: case.inspect('run-1'),
             lambda v: v['runtime'] == 'requires_action'
             and v['delivery'] == 'acknowledged', seconds=120)
        view = poll(lambda: case.inspect('run-1'), lambda v: v['runtime'] == 'exited',
                    seconds=240)
        events = events_of(case, 'run-1')
        clocks = [e['payload'].get('timeout') for e in events
                  if e['type'] == 'execution.timeout.passed']
        assert not clocks, f'this run was ended by the {clocks} timeout: {view}'

        settled = [e for e in events if e['type'] == 'execution.action.answered']
        assert len(settled) == 2, [e['type'] for e in events]
        ids = [e['payload']['action_id'] for e in settled]
        assert ids == ['run-1.action-1', 'run-1.action-2'], ids
        for event in settled:
            decision = event['payload'][DECISION]
            assert event['origin'] == 'provider', event
            assert decision['decided_by'] == 'pio', decision
            assert decision['basis'] == 'deadline_lapsed', decision
            assert decision['after_seconds'] == lapse_after, decision
            assert decision['option_kind'] == 'reject_once', decision

        actions = case.inspect('run-1')['actions']
        assert [a['action_id'] for a in actions] == ids, actions
        assert [a['state'] for a in actions] == ['answered', 'answered'], actions
        assert all('answered_at' in a for a in actions), actions
        # Two rows, not one row twice: each keeps its own requested_at, and
        # the second was asked about only after the first had lapsed.
        assert actions[0]['requested_at'] <= actions[1]['requested_at'], actions
        assert len({a['action_id'] for a in actions}) == 2, actions
        assert walk(events) == [], walk(events)
        case.finish()
    return dict(actions=ids, states=['answered', 'answered'],
                decided_by=['pio', 'pio'], after_seconds=lapse_after)


# --- the walk against the other two release harnesses -------------------------

def pass_harness(out, harness, mutant=None):
    """The walk through `serve-claude` or `serve-codex`.

    What each harness **does not** do is the point. Codex offers no option
    list; its approval carries the caller's deadline and lapses to one decline
    of PIO's (owner decision, 2026-09-25; before that it waited for ever, and
    the walk asserted so). Claude Code offers a rule update with every
    request, and acting on one would widen a permission beyond it — so
    suggestions appear as offered and never as something PIO will send.
    """
    facts = {'harness': harness.serve}
    with harness.service(out, f'{harness.name}-walk', **harness.ask) as svc:
        svc.start()
        svc.submit(identity='run-1', delivery_timeout=300)
        poll(lambda: svc.inspect('run-1'),
             lambda v: v['runtime'] == 'requires_action', seconds=120)
        rows = walk(events_of(svc))
        assert len(rows) == 1, rows
        row = rows[0]
        facts['approval'] = {k: row[k] for k in
                             ('answer_deadline_seconds', 'options', 'method',
                              'approval_kind', 'if_nobody_answers') if k in row}

        # Every release harness now has a deadline: Codex's approvals lapse
        # to one decline of PIO's since 2026-09-25 (owner decision), as the
        # other two's always have.
        assert harness.lapses, harness.name
        assert row['answer_deadline_seconds'] == 300, row
        assert row['due'] is not None, row
        assert row['if_nobody_answers'], row
        if harness.name == 'codex':
            # What is being approved, so whoever decides can see it: the
            # command the request names (the fake's `echo fixture`).
            assert row['method'] and row['approval_kind'] and \
                row['command'] == 'echo fixture', row

        if harness.offers_options:
            assert [o['kind'] for o in row['options']] == \
                ['allow_once', 'allow_always', 'reject_once'], row
        else:
            assert row['options'] is None, (
                'this harness offers no option list, and an invented one '
                f'would be a list the screen could not send from: {row}')

        if harness.suggests:
            # Claude Code. The harness offers a rule update on every request.
            # What PIO will send is built by `permission_decision`, which
            # cannot encode one — so the walk shows the suggestion as offered
            # and never as an option PIO sends.
            sending = row['if_nobody_answers']
            assert sending['behavior'] == 'deny', sending
            assert sending['single_use'] is True, sending
            assert sending['suggestions_offered'] == 1, sending
            assert sending['widening_fields_sent'] == [], sending

        # The caller answers its own run, through the released operation.
        view = svc.inspect('run-1')
        accepted = svc.respond(row['action_id'], harness.allow, view['revision'],
                               identity='run-1')
        assert accepted.get('result', {}).get('outcome', {}).get('state') == 'answered', \
            accepted
        settled = answered(events_of(svc), row['action_id'])
        assert settled is not None, 'the caller answered and the stream did not say so'
        assert settled['origin'] == 'command', settled
        assert settled['payload'][DECISION] == {'decided_by': 'caller',
                                                'decision': harness.allow}, settled
        facts['caller_decision'] = settled['payload'][DECISION]
        poll(lambda: svc.inspect('run-1'), lambda v: v['runtime'] == 'exited',
             seconds=200)
        if harness.suggests:
            # Measured at the harness, not only in PIO's own payload: the
            # fake records any widening field it receives, and the Claude
            # matrix proves that detector fires when one does.
            received = [m for m in svc.case.markers_of('permission_decision')]
            assert received, 'the fake recorded no decision'
            assert all(m['widening_fields_received'] == [] for m in received), received
            facts['widening_fields_received'] = []
        svc.finish()

    return facts


# --- 6. what a caller that ignores the keys sees ------------------------------

def normalize(value, strip=True):
    if isinstance(value, dict):
        return {k: normalize(v, strip) for k, v in value.items()
                if not (strip and k.startswith(NS))}
    if isinstance(value, list):
        return [normalize(v, strip) for v in value]
    if isinstance(value, str):
        if INSTANT.match(value):
            return '<instant>'
        if SHA.match(value):
            return '<sha>'
        if UUID.match(value):
            return '<uuid>'
        return TMPROOT.sub('<root>', value)
    return value


def shape(events, strip=True):
    """What a reader sees, with everything that cannot repeat removed."""
    return [dict(type=e['type'], subject=e['subject']['id'], origin=e['origin'],
                 payload=normalize(e['payload'], strip))
            for e in events]


def collect(out, label, lapse, request=None, keep=False):
    """One run, and every Protocol event it produced.

    Asserts nothing about G2 — it has to behave the same on both binaries,
    because the whole point is to compare them.
    """
    with service(out, label, permission_request=request or ASKS) as case:
        case.start()
        case.submit(identity='run-1', delivery_timeout=20 if lapse else 150)
            # The same wait `pass_one` makes, and for the same reason: while
        # delivery is still `pending` the delivery timeout is live, and it
        # shares its number with the answer deadline. Without this, the run
        # whose lapse the probe is measuring could be ended by the other
        # clock instead — which is the open item below, reproduced here.
        view = poll(lambda: case.inspect('run-1'),
                    lambda v: v['runtime'] == 'exited'
                    or (v['runtime'] == 'requires_action'
                        and v['delivery'] == 'acknowledged'), seconds=120)
        if not lapse and view['runtime'] == 'requires_action':
            # A declined request never surfaces, so there is nothing to answer.
            case.respond(view['runtime_detail']['action_id'], 'deny', view['revision'],
                         identity='run-1')
        final = poll(lambda: case.inspect('run-1'), lambda v: v['runtime'] == 'exited',
                     seconds=150)
        events = events_of(case, 'run-1')
        case.finish()
        return (events, final) if keep else events


def build_baseline(commit):
    """The binary as it was before G2, built from its own worktree."""
    tree = ROOT.parent / f'pio-baseline-{commit}'
    binary = tree / 'target/debug/pio'
    if not tree.exists():
        subprocess.run(['git', '-C', str(ROOT), 'worktree', 'add', '--detach',
                        str(tree), commit], check=True, capture_output=True)
    subprocess.run(['cargo', 'build'], cwd=str(tree), check=True)
    assert binary.exists(), binary
    return binary


def with_binary(binary, work):
    previous = matrix.BINARY
    matrix.BINARY = Path(binary)
    try:
        return work()
    finally:
        matrix.BINARY = previous


def subsequence(small, large):
    """Index in `large` of each item of `small`, in order, or None."""
    where, cursor = [], 0
    for item in small:
        while cursor < len(large) and large[cursor] != item:
            cursor += 1
        if cursor == len(large):
            return None
        where.append(cursor)
        cursor += 1
    return where


def pass_three(out, commit, strip=True):
    """6. A caller that ignores the namespaced keys sees what it saw before."""
    binary = build_baseline(commit)
    report = {}

    # (a) A run with no PIO decision. Nothing new happens, so with the keys
    #     removed the two streams must be the same stream.
    now = shape(collect(out, 'diff-answered', lapse=False), strip)
    before = with_binary(binary, lambda: shape(
        collect(out / 'baseline', 'diff-answered', lapse=False), strip))
    assert now == before, next(
        (json.dumps(dict(at=i, before=b, now=n), sort_keys=True)[:900]
         for i, (b, n) in enumerate(zip(before, now)) if b != n),
        f'lengths differ: {len(before)} before, {len(now)} now')
    report['no_pio_decision'] = dict(events=len(now), identical=True)

    # (b) A run whose deadline lapses. Here the caller *should* see something
    #     it could not see before — that is the fix. So the exact statement
    #     is: every event the old stream carried is still there, unchanged
    #     and in the same order, and the only additions are PIO's decisions.
    #     Sequence and revision necessarily advance, which is why the diff is
    #     on type, subject, origin and payload rather than on counters.
    now = shape(collect(out, 'diff-lapsed', lapse=True), strip)
    before = with_binary(binary, lambda: shape(
        collect(out / 'baseline', 'diff-lapsed', lapse=True), strip))
    where = subsequence(before, now)
    assert where is not None, json.dumps(dict(before=before, now=now))[:1500]
    added = [row for i, row in enumerate(now) if i not in set(where)]
    assert [row['type'] for row in added] == ['execution.action.answered'], added
    assert added[0]['payload'] == {'action_id': added[0]['payload'].get('action_id')} \
        or strip is False, added
    report['pio_decided'] = dict(before=len(before), now=len(now),
                                 added=[row['type'] for row in added],
                                 stripped_payload_of_addition=added[0]['payload'])
    return report


def baseline_mutants(out, commit):
    """The mutant for requirements 1, 4 and 5 is the defect itself.

    Each is run twice — against the binary built at `commit`, where the
    assertion must fail, and against this one, where it must hold — so the
    pass is attributable to the change rather than to the scenario. Whole
    passes are not reused here: at `commit` the walk cannot be drawn at all,
    so every requirement would die on the first one and prove nothing about
    the rest.
    """
    binary = build_baseline(commit)
    probes, report = {}, {}

    def approval_fields(label):
        # 1. The deadline and the offered options on the stream.
        events = collect(out, f'{label}-ask', lapse=False)
        return [e['payload'][APPROVAL] for e in events
                if e['type'] == 'execution.runtime.changed' and APPROVAL in e['payload']]

    def lapse(label):
        # 4. A deadline that lapsed, and the state the view is left in.
        events, view = collect(out, f'{label}-lapse', lapse=True, keep=True)
        return dict(answered=[e['type'] for e in events
                              if e['type'] == 'execution.action.answered'],
                    states=[a['state'] for a in view['actions']],
                    # The rule above, on real data. At the mutant this is the
                    # only thing that empties the walk, because there is no
                    # answered event to remove the row.
                    walk_after_exit=len(walk(events)))

    def decline(label):
        # 5. A decline PIO made before any caller was asked.
        events = collect(out, f'{label}-decline', lapse=False, request=DECLINES)
        return dict(answered=[e['type'] for e in events
                              if e['type'] == 'execution.action.answered'])

    probes['approval_fields_reach_the_stream'] = (
        approval_fields, lambda r: len(r) == 1 and r[0]['answer_deadline_seconds'] > 0)
    probes['a_lapsed_deadline_is_pios_decision'] = (
        lapse, lambda r: r['answered'] == ['execution.action.answered']
        and r['states'] == ['answered'])
    walks = []
    probes['a_decline_is_pios_decision'] = (
        decline, lambda r: r['answered'] == ['execution.action.answered'])


    for name, (probe, holds) in probes.items():
        before = with_binary(binary, lambda: probe('mutant'))
        release_live_cases()
        assert not holds(before), \
            f'MUTANT SURVIVED: {name} already held at {commit}: {json.dumps(before)[:400]}'
        after = probe('fixed')
        release_live_cases()
        assert holds(after), f'{name} does not hold on this binary: {json.dumps(after)[:400]}'
        report[name] = dict(at_the_mutant=before, now=after)
        walks += [r['walk_after_exit'] for r in (before, after)
                  if isinstance(r, dict) and 'walk_after_exit' in r]
    assert walks and set(walks) == {0}, \
        f'a run that exited is still on the walk: {walks}'
    return dict(mutant=commit, requirements=report,
                walk_is_empty_after_the_run_exits=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/approval-desk')
    # `assume-countdown` is retired: it substituted a countdown for Codex's
    # missing deadline, and since 2026-09-25 Codex has one.
    parser.add_argument('--mutant', choices=['arrival-order', 'cross-run', 'unstripped'])
    parser.add_argument('--harness', default='opencode', choices=proof_harness.NAMES,
                        help='opencode runs the full proof; claude and codex run the '
                             'walk against the other two release harnesses')
    parser.add_argument('--baseline-as-mutant', action='store_true',
                        help='run passes 1 and 2 against the pre-G2 binary; '
                             'both must fail')
    parser.add_argument('--baseline-commit', default=BASELINE_COMMIT)
    parser.add_argument('--skip-baseline', action='store_true')
    args = parser.parse_args()
    out = args.out
    out.mkdir(parents=True, exist_ok=True)

    try:
        if args.baseline_as_mutant:
            print(json.dumps(baseline_mutants(out / 'mutant', args.baseline_commit),
                             indent=2, sort_keys=True))
            print('every mutant died')
            return

        if args.harness != 'opencode':
            # The other two release harnesses. The deep passes — two runs
            # ordered by deadline, the wrong-run refusal, the lapse, the
            # decline and the a22c98c diff — stay on OpenCode, whose
            # permission path is the one measured live. What these prove is
            # that the walk reads the same on all three, and that each
            # harness's own behaviour reaches it.
            harness = proof_harness.load(args.harness)
            record = pass_harness(out, harness, args.mutant)
            (out / f'approval-desk-{args.harness}.json').write_text(
                json.dumps(record, indent=2, sort_keys=True) + '\n')
            print(json.dumps(record, indent=2, sort_keys=True))
            print(f'approval desk ({harness.serve}): pass')
            return
        record = dict(harness='serve-opencode with the labeled ACP fake')
        record['desk'] = pass_one(out, args.mutant)[0]
        record['decline'] = pass_two(out)[0]
        record['two_actions_one_run'] = pass_two_actions(out)
        if not args.skip_baseline:
            record['unchanged_for_a_caller_that_ignores_the_keys'] = pass_three(
                out, args.baseline_commit, strip=args.mutant != 'unstripped')
        (out / 'approval-desk.json').write_text(
            json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(json.dumps(record, indent=2, sort_keys=True))
        print('approval desk: pass')
    finally:
        release_live_cases()


if __name__ == '__main__':
    main()
