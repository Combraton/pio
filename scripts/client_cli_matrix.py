#!/usr/bin/env python3
"""`pio client` end to end, against real services and the public socket.

The command line is the first consumer of `pio_client`, the public client the
M4 screen will use, so this drives the **binary** the way a person or a
script does and checks what it printed and how it exited against what an
independent reader (the Python test caller in `public_api.py`) sees on the
same socket.

`serve-fake` covers what it can: submit through the caller ledger, the
board, inspect, output as bytes, and a watcher that detaches and comes back.
It advertises no approvals and cannot cancel, so the approval cases and the
cancel run through **`serve-opencode` with the labeled ACP fake behind it**,
as `approval_desk.py` does: a real host, a real journal, no real harness.

Cases:

- `submit_list_inspect_output` (fake): two runs submitted with `client
  submit`, drawn by `client list` as running and then finished, `client
  inspect` equal to the service's own view, and `client output --follow`
  byte-identical to the spool.
- `watch_detach_resume` (fake): four watchers over one state file, stopped
  by `--max-events`, by `--idle-exit` and by SIGTERM, between which new runs
  start. Together they print every event exactly once, in order: nothing
  lost, nothing twice.
- `watch_notices` (fake, keeping four events): a watcher from the start is
  handed a retention gap and prints it; one that comes back does not print it
  again, and prints each event once; a person sees `-- retention gap:`.
- `watch_other_store` (fake): a position saved against one store and used
  against another is dropped with a notice, and the new store's stream is
  printed from its beginning, rather than `invalid_cursor` on every run.
- `ledger_exit_codes` (fake): `client submit` and `client reconcile` end as
  the service said: an `internal_error` on submit is not known yet (exit 4,
  pending, run reconcile), the reconcile replays it (exit 0), and a refusal
  with `retry: no` exits 3 with the error object verbatim.
- `approvals_answer_and_refusals` (opencode): two runs waiting, drawn by
  `client list` and walked by `client approvals`; an answer aimed at the
  wrong run is refused `not_found` (exit 3 with `--json` and without, the
  error object verbatim), one fenced at a stale epoch is refused
  `stale_authority_epoch`, and both runs still wait; then a real answer, on
  the stream as the caller's.
- `approvals_after_retention` (opencode, keeping three events): the walk
  says it has a gap and still lists both waiting requests, from the runs'
  views, marked lost to retention with no invented deadline.
- `allow_on_each_harness` (opencode, claude, codex): `client answer ...
  allow` on each labeled fake; the stream has the caller's allow (Codex:
  accept) and the fake's own marker shows the single-use allow it received
  (OpenCode `opt_1`/`allow_once`, Claude `allow` with no widening field,
  Codex `{"decision": "accept"}`).
- `cancel` (opencode): a waiting run cancelled with `client cancel`, and
  exited afterwards; the fake leaves its action pending on the exited run,
  and the board reads it `uncertain` with no approval waiting (not "needs
  approval"), the walk agrees, and an answer to it is refused
  `run_not_running`; `client steer` on a harness with no steering is refused
  in the service's words.

Mutants the script plays (the command line is sound; the test makes the
wrong move), each required to fail its named assertion:

- `cross-run` aims the wrong-run answer at its own run, so the refusal is
  shown to come from the mismatch and not from a malformed answer;
- `fresh-epoch` sends the stale answer without `--epoch`, so the refusal is
  shown to come from the pinned epoch;
- `forgot-last` erases the delivered position between two watchers (a
  watcher that kept only the page cursor), and events print twice;
- `saved-page-end` moves the saved cursor to the end of the page a watcher
  stopped inside (a watcher that saved `next_cursor` early), and events are
  lost.

Mutants in the source (`pio client` built from a worktree of HEAD with one
edit, `source_mutant.py`; the services still run from the checkout):

- `human-refusal-exits-0` (M3 of the review): a refusal read by a person
  exits 0;
- `answer-always-deny` (M4 of the review): every answer sends deny;
- `approvals-gap-blind`: the walk is not completed after a gap;
- `ledger-exits-0`: submit and reconcile exit 0 whatever the service said;
- `watch-ignores-stream`: a saved position is used against any store;
- `watch-drops-notices` (M6 of the review's second round): gaps and epoch
  changes are not printed.
"""
import argparse
import base64
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
import source_mutant
from public_api import CREDENTIAL, Client, submit

ROOT = Path(__file__).resolve().parents[1]
# The services always run from the checkout's binary. `pio client` runs from
# CLI_BINARY, which a source mutant replaces with one built from a mutated
# worktree of HEAD (`source_mutant.py`), so only the command line is broken.
BINARY = ROOT / 'target/debug/pio'
CLI_BINARY = BINARY


class Failed(AssertionError):
    pass


def check(condition, what, detail=''):
    if not condition:
        raise Failed(f'{what}: {detail}' if detail else what)


def private_dir(prefix):
    path = Path(tempfile.mkdtemp(prefix=prefix, dir='/tmp')).resolve()
    os.chmod(path, 0o700)
    return path


class Cli:
    """The binary, pointed at one service with one credential file."""

    def __init__(self, socket, work):
        self.socket = socket
        self.credential = work / 'credential'
        self.credential.write_text(CREDENTIAL)
        self.credential.chmod(0o600)
        self.calls = []

    def argv(self, *args, json_out=True):
        return [str(CLI_BINARY), 'client', *args, '--socket', str(self.socket),
                '--credential-file', str(self.credential)] + (['--json'] if json_out else [])

    def run(self, *args, json_out=True, timeout=120):
        result = subprocess.run(self.argv(*args, json_out=json_out), capture_output=True,
                                text=True, timeout=timeout)
        self.calls.append(dict(args=list(args), exit=result.returncode,
                               stdout=result.stdout[-4000:], stderr=result.stderr[-2000:]))
        return result

    def json(self, *args):
        result = self.run(*args)
        check(result.returncode == 0, f'pio client {" ".join(args)} failed',
              result.stderr or result.stdout)
        return json.loads(result.stdout)


def wait_for(what, probe, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            value = probe()
            if value:
                return value
        except (OSError, EOFError, KeyError, ValueError):
            pass
        time.sleep(0.2)
    raise Failed(f'timed out waiting for {what}')


def witness(socket, operation, payload):
    """The independent reader: the Python test caller, not the CLI."""
    with Client(socket) as client:
        return client.query(operation, payload)


def verbatim(result):
    """The refusal a command printed, from both of its channels: the JSON on
    stdout and the line on stderr must carry the same error object."""
    printed = json.loads(result.stdout)['refused']['error']
    said = json.loads(result.stderr.split('refused by the service: ', 1)[1])
    check(printed == said, 'the refusal is printed as the service sent it',
          f'{printed} != {said}')
    return printed


def all_events(socket):
    """Every event on the stream, by an independent reader."""
    events, payload = [], {'limit': 1000, 'from': 'start'}
    with Client(socket) as client:
        while True:
            result = client.query('core.events.read', payload)['result']
            events += [item['event'] for item in result['items'] if 'event' in item]
            if not result['items']:
                return events
            payload = {'limit': 1000, 'cursor': result['next_cursor']}


# --- serve-fake cases -----------------------------------------------------------

def fake_service(out, name, work, duration_ms=800):
    from board_fold import start_service
    daemon, socket, files = start_service(work, out, name=f'daemon-{name}',
                                          duration_ms=duration_ms)
    return daemon, socket, files


def stop_fake(daemon, files):
    daemon.kill()
    daemon.wait(timeout=10)
    for handle in files:
        handle.close()


def submit_run(cli, work, identity, duration=800):
    request = work / f'{identity}.json'
    request.write_text(json.dumps(submit(duration, identity=identity)))
    store = work / 'caller'
    store.mkdir(mode=0o700, exist_ok=True)
    answer = cli.json('submit', '--store', str(store), '--request', str(request))
    check(answer['response']['result']['outcome']['admission'] == 'admitted',
          f'{identity} was not admitted', answer)
    return answer


def case_submit_list_inspect_output(out, mutant):
    work = private_dir('pio-cm-')
    daemon, socket, files = fake_service(out, 'submit', work)
    try:
        cli = Cli(socket, work)
        for identity in ('run-1', 'run-2'):
            submit_run(cli, work, identity)
        early = cli.json('list')
        check([r['id'] for r in early['runs']] == ['run-1', 'run-2'],
              'the board lists the two runs', early)
        wait_for('both runs to finish', lambda: all(
            r['state'] == 'finished' for r in cli.json('list')['runs']))
        board = cli.json('list')
        check(board['counts'] == {'finished': 2} and board['notes'] ==
              {'approvals': 0, 'uncertain': 0}, 'the board draws two finished runs', board)
        human = cli.run('list', json_out=False)
        check(human.returncode == 0 and 'finished' in human.stdout
              and 'run-1' in human.stdout, 'the human board names its runs', human.stdout)

        view = cli.json('inspect', 'run-1')
        direct = witness(socket, 'execution.inspect', {'execution': 'run-1'})['result']
        check(view == direct, "client inspect is the service's own view",
              f'{view} != {direct}')

        spooled, offset = b'', 0
        with Client(socket) as client:
            while True:
                result = client.query('execution.output.read',
                                      {'execution': 'run-1', 'offset': offset,
                                       'max_bytes': 65536})['result']
                data = base64.b64decode(result['data_base64'])
                if not data:
                    break
                spooled += data
                offset = result['next_offset']
        printed = subprocess.run(cli.argv('output', 'run-1', '--follow', json_out=False),
                                 capture_output=True, timeout=60)
        check(printed.returncode == 0 and printed.stdout == spooled and spooled,
              'client output prints the spool byte for byte',
              f'{len(printed.stdout)} bytes printed, {len(spooled)} spooled')
        return dict(runs=len(board['runs']), board_counts=board['counts'],
                    inspect_equal=True, output_bytes=len(spooled))
    finally:
        stop_fake(daemon, files)
        case_cleanup.release(work)


def case_watch_detach_resume(out, mutant):
    work = private_dir('pio-cw-')
    daemon, socket, files = fake_service(out, 'watch', work, duration_ms=600)
    try:
        cli = Cli(socket, work)
        state = work / 'watch.json'
        for identity in ('run-1', 'run-2', 'run-3'):
            submit_run(cli, work, identity, 600)
        wait_for('three runs to finish', lambda: sum(
            1 for e in all_events(socket) if e['type'] == 'execution.exit.observed') == 3)
        printed = []

        def lines(text):
            return [e for e in (json.loads(line) for line in text.splitlines() if line.strip())
                    if 'sequence' in e]

        # A: stops after five events, inside the first page.
        first = cli.run('watch', '--state', str(state), '--max-events', '5')
        check(first.returncode == 0, 'the first watcher ran', first.stderr)
        printed += lines(first.stdout)
        check(len(printed) == 5, 'the first watcher printed five events', len(printed))
        saved = json.loads(state.read_text())
        check(saved['last']['sequence'] == printed[-1]['sequence'],
              'the position is the last event printed', saved)
        if mutant == 'forgot-last':
            saved['last'] = None
            state.write_text(json.dumps(saved))
        if mutant == 'saved-page-end':
            with Client(socket) as client:
                page = client.query('core.events.read', {'limit': 1000, 'from': 'start'})
            saved['cursor'] = page['result']['next_cursor']
            state.write_text(json.dumps(saved))

        # B: comes back and leaves once the stream is quiet.
        second = cli.run('watch', '--state', str(state), '--idle-exit', '1')
        check(second.returncode == 0, 'the second watcher ran', second.stderr)
        printed += lines(second.stdout)

        # C: follows while a run starts, and is detached with SIGTERM.
        follower = subprocess.Popen(cli.argv('watch', '--state', str(state)),
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            time.sleep(0.5)
            submit_run(cli, work, 'run-4', 600)
            wait_for('run-4 to finish', lambda: any(
                e['type'] == 'execution.exit.observed' and e['subject']['id'] == 'run-4'
                for e in all_events(socket)))
            time.sleep(1.0)
            follower.send_signal(signal.SIGTERM)
            stdout, stderr = follower.communicate(timeout=20)
        finally:
            if follower.poll() is None:
                follower.kill()
                follower.wait()
        check(follower.returncode == 0, 'a watcher detached by SIGTERM exits cleanly',
              f'{follower.returncode}: {stderr}')
        third = lines(stdout)
        check(any(e['subject']['id'] == 'run-4' for e in third),
              'the follower saw the new run while it was attached', len(third))
        printed += third

        # D: a run starts while nobody watches; the next watcher sees it.
        submit_run(cli, work, 'run-5', 600)
        wait_for('run-5 to finish', lambda: any(
            e['type'] == 'execution.exit.observed' and e['subject']['id'] == 'run-5'
            for e in all_events(socket)))
        fourth = cli.run('watch', '--state', str(state), '--idle-exit', '1')
        check(fourth.returncode == 0, 'the fourth watcher ran', fourth.stderr)
        printed += lines(fourth.stdout)

        truth = [(e['epoch'], e['sequence']) for e in all_events(socket)]
        seen = [(e['epoch'], e['sequence']) for e in printed]
        duplicated = sorted({s for s in seen if seen.count(s) > 1})
        lost = [s for s in truth if s not in seen]
        check(not duplicated, 'no event is printed twice across detaches', duplicated[:5])
        check(not lost, 'no event is lost across detaches', lost[:5])
        check(seen == truth, 'the watchers print the stream in order')
        human = cli.run('watch', '--state', str(work / 'human.json'), '--max-events', '2',
                        json_out=False)
        check(human.returncode == 0 and human.stdout.count('\n') == 2
              and 'position is saved' in human.stderr, 'the human watcher', human.stderr)
        return dict(events=len(truth), watchers=4,
                    per_watcher=[5, len(lines(second.stdout)), len(third),
                                 len(lines(fourth.stdout))],
                    detached_by=['--max-events', '--idle-exit', 'SIGTERM', '--idle-exit'],
                    duplicated=0, lost=0)
    finally:
        stop_fake(daemon, files)
        case_cleanup.release(work)


# --- serve-opencode cases -------------------------------------------------------

def opencode_service(out, name, **scenario):
    from approval_desk import service
    return service(out, name, **scenario)


def waiting(case, identity):
    return wait_for(f'{identity} to wait on an approval', lambda: (
        lambda v: v if v['runtime'] == 'requires_action' and v['delivery'] == 'acknowledged'
        else None)(case.inspect(identity)), seconds=120)


def case_approvals_answer_and_refusals(out, mutant):
    from approval_desk import ASKS, DECISION, events_of
    work = private_dir('pio-ca-')
    try:
        with opencode_service(out, 'cli-desk', permission_request=ASKS) as case:
            case.start()
            for identity in ('run-1', 'run-2'):
                case.submit(identity=identity, delivery_timeout=300)
                waiting(case, identity)
            cli = Cli(case.socket, work)

            board = cli.json('list')
            check(board['counts'] == {'needs approval': 2} and board['notes']['approvals'] == 2,
                  'the board shows two runs needing approval', board)
            desk = cli.json('approvals')
            rows = desk['waiting']
            check(sorted(r['run'] for r in rows) == ['run-1', 'run-2'],
                  'the walk has both requests', rows)
            for row in rows:
                check([o['kind'] for o in row['options']] ==
                      ['allow_once', 'allow_always', 'reject_once'],
                      'the walk shows what the harness offered', row)
                check(row['if_nobody_answers']['option_kind'] == 'reject_once',
                      'the walk shows what PIO will send', row)
                check(row['due'] is not None, 'the walk shows a deadline', row)
            human = cli.run('approvals', json_out=False)
            check(human.returncode == 0 and 'never sent by PIO' in human.stdout
                  and all(r['action_id'] in human.stdout for r in rows),
                  'the human walk', human.stdout)
            action = {r['run']: r['action_id'] for r in rows}

            # An answer aimed at the wrong run: run-2's action, on run-1.
            target = 'run-2' if mutant == 'cross-run' else 'run-1'
            wrong = cli.run('answer', target, action['run-2'], 'allow')
            check(wrong.returncode == 3,
                  'an answer aimed at the wrong run is refused, never a success',
                  f'exit {wrong.returncode}: {wrong.stdout}')
            wrong_error = verbatim(wrong)
            check(wrong_error['data']['code'] == 'not_found',
                  'the wrong-run refusal is the service\'s own', wrong_error)
            # And for a person, without --json: the same exit status, the
            # same error object on stderr, nothing on stdout that reads as done.
            person = cli.run('answer', target, action['run-2'], 'allow', json_out=False)
            check(person.returncode == 3,
                  'a refusal read by a person exits 3 too, never a success',
                  f'exit {person.returncode}: {person.stdout}{person.stderr}')
            check(person.stdout == '' and json.loads(
                person.stderr.split('refused by the service: ', 1)[1]) == wrong_error,
                'a person sees the refusal verbatim on stderr', person.stderr)
            for identity in ('run-1', 'run-2'):
                check(case.inspect(identity)['runtime'] == 'requires_action',
                      f'{identity} still waits after the wrong-run answer')

            # A stale controller: the epoch moves, and an answer fenced at the
            # old one is refused.
            claim = cli.json('claim', '--host', 'opencode-host')
            epoch = claim['outcome']['epoch']
            check(epoch >= 1, 'the claim moved the epoch', claim)
            stale = ['--epoch', str(epoch - 1)] if mutant != 'fresh-epoch' else []
            late = cli.run('answer', 'run-1', action['run-1'], 'allow', *stale)
            check(late.returncode == 3,
                  'an answer from a stale controller is refused, never a success',
                  f'exit {late.returncode}: {late.stdout}')
            late_error = verbatim(late)
            check(late_error['data']['code'] == 'stale_authority_epoch',
                  'the stale-epoch refusal is the service\'s own', late_error)
            for identity in ('run-1', 'run-2'):
                check(case.inspect(identity)['runtime'] == 'requires_action',
                      f'{identity} still waits after the stale answer')
            check(not [e for e in events_of(case) if e['type'] == 'execution.action.answered'],
                  'nothing was approved by either refused answer')

            # The real answer, at the current fence.
            done = cli.run('answer', 'run-1', action['run-1'], 'deny', json_out=False)
            check(done.returncode == 0 and 'answered' in done.stdout,
                  'a correct answer is taken', done.stderr)
            answered = [e for e in events_of(case) if e['type'] == 'execution.action.answered']
            check([(e['subject']['id'], e['payload'][DECISION]) for e in answered] ==
                  [('run-1', {'decided_by': 'caller', 'decision': 'deny'})],
                  'the answer is on the stream as the caller\'s, on its own run', answered)
            after = cli.json('approvals')
            check([r['run'] for r in after['waiting']] == ['run-2'],
                  'the walk drops the answered request', after['waiting'])
            check([(d['run'], d['decided_by']) for d in after['decided']] ==
                  [('run-1', 'caller')], 'the decision is listed', after['decided'])
            case.finish()
            return dict(waiting=2, wrong_run=wrong_error['data']['code'],
                        stale_epoch=late_error['data']['code'], epoch=epoch,
                        answered=[e['payload'][DECISION] for e in answered])
    finally:
        case_cleanup.release(work)


def case_cancel(out, mutant):
    from approval_desk import ASKS
    work = private_dir('pio-cc-')
    try:
        with opencode_service(out, 'cli-cancel', permission_request=ASKS) as case:
            case.start()
            case.submit(identity='run-1', delivery_timeout=300)
            waiting(case, 'run-1')
            cli = Cli(case.socket, work)
            steer = cli.run('steer', 'run-1', 'please stop')
            check(steer.returncode == 3, 'a steer the harness cannot take is refused',
                  steer.stdout)
            check(verbatim(steer)['data']['code'] == 'unsupported_required_feature',
                  'a steer to a harness with no steering is refused in the service\'s words',
                  steer.stdout)
            cancelled = cli.json('cancel', 'run-1', '--reason', 'matrix')
            check(cancelled['outcome']['receipt']['state'] == 'cancel_requested',
                  'the cancel is acknowledged', cancelled)
            view = wait_for('run-1 to exit after the cancel', lambda: (
                lambda v: v if v['runtime'] == 'exited' else None)(case.inspect('run-1')),
                seconds=60)
            check(view['cancellation']['receipt']['state'] == 'cancel_requested',
                  'the run carries the cancel', view.get('cancellation'))
            # The fake leaves the action pending on the exited run (a host
            # item the orchestrator tracks). It waits on nobody: the board
            # must not say "needs approval", and the walk must agree.
            leftover = [a['state'] for a in view.get('actions', [])]
            board = cli.json('list')
            row = board['runs'][0]
            check(row['state'] == 'uncertain' and row['pending_actions'] == []
                  and board['notes']['approvals'] == 0,
                  'an exited run with a leftover pending action waits on nobody',
                  f'{leftover} -> {row} {board["notes"]}')
            human = cli.run('list', json_out=False)
            check(human.returncode == 0 and 'needs approval' not in human.stdout
                  and '0 approvals waiting' in human.stdout,
                  'a person is not told an exited run needs approval', human.stdout)
            check(cli.json('approvals')['waiting'] == [], 'the walk agrees: none waiting')
            if 'pending' in leftover:
                late = cli.run('answer', 'run-1', view['actions'][0]['action_id'], 'allow')
                check(late.returncode == 3 and verbatim(late)['data']['details'].get('reason')
                      == 'run_not_running', 'and the service refuses an answer to it', late.stdout)
            case.finish()
            return dict(cancel=cancelled['outcome']['receipt'],
                        cancellation=view['cancellation'].get('outcome'), exit=view['exit'],
                        leftover_actions=leftover, board_state=row['state'])
    finally:
        case_cleanup.release(work)


def case_allow_on_each_harness(out, mutant):
    """`allow`, end to end, on each release harness's labeled fake: the
    decision on the stream is the caller's `allow` (Codex: `accept`), and
    what the fake itself received is the single-use allow, never always."""
    import proof_harness
    from approval_desk import DECISION, events_of
    results = {}
    for name in ('opencode', 'claude', 'codex'):
        harness = proof_harness.load(name)
        work = private_dir('pio-cl-')
        try:
            with harness.service(out, f'allow-{name}', **harness.ask) as svc:
                svc.start()
                svc.submit(identity='run-1', delivery_timeout=300)
                view = wait_for(f'{name}: run-1 to wait on an approval', lambda: (
                    lambda v: v if v['runtime'] == 'requires_action' else None)(
                        svc.inspect('run-1')), seconds=120)
                action = view['runtime_detail']['action_id']
                cli = Cli(getattr(svc.case, 'socket', None)
                          or svc.case.root / 'public.sock', work)
                done = cli.run('answer', 'run-1', action, 'allow', json_out=False)
                check(done.returncode == 0 and 'answered' in done.stdout,
                      f'{name}: an allow is taken', done.stdout + done.stderr)
                wait_for(f'{name}: run-1 to exit', lambda: svc.inspect('run-1')['runtime']
                         == 'exited', seconds=120)
                with svc.client() as client:
                    stream = client.query('core.events.read', {
                        'limit': 1000, 'from': 'start',
                        'kinds': ['execution.execution']})['result']
                answered = [i['event']['payload'].get(DECISION) for i in stream['items']
                            if 'event' in i and i['event']['type']
                            == 'execution.action.answered']
                word = harness.allow
                check(answered == [{'decided_by': 'caller', 'decision': word}],
                      f'{name}: the stream has the caller\'s {word}', answered)
                received = received_by(name, svc.case)
                check(received['allowed'] and not received['always'],
                      f'{name}: the fake received a single-use allow', received)
                svc.finish()
                results[name] = dict(stream=answered[0], received=received['raw'])
        finally:
            case_cleanup.release(work)
    return results


def received_by(name, case):
    """What the labeled fake itself says it was sent: its own marker file,
    not PIO's record of what it sent."""
    if name == 'codex':
        import codex_host_matrix
        wire = codex_host_matrix.answered_once(case)
        check(len(wire) == 1, 'codex: one answer reached the fake', wire)
        result = wire[0]['result']
        return dict(allowed=result == {'decision': 'accept'},
                    always='acceptForSession' in json.dumps(result) or 'persist' in result,
                    raw=result)
    decided = case.markers_of('permission_decision')
    check(len(decided) == 1, f'{name}: one decision reached the fake', decided)
    record = decided[0]
    if name == 'claude':
        return dict(allowed=record.get('behavior') == 'allow',
                    always=bool(record.get('widening_fields_received')), raw=record)
    return dict(allowed=record.get('option_kind') == 'allow_once'
                and record.get('option_id') == 'opt_1',
                always=bool(record.get('always_option_taken'))
                or bool(record.get('widening_fields_received')), raw=record)


def case_approvals_after_retention(out, mutant):
    """Two requests waiting on a service that keeps only its last three
    events: the events that carried them are gone. The walk must say it has
    a gap, and still list both, from the runs' views, marked lost."""
    from approval_desk import ASKS
    work = private_dir('pio-cr-')
    try:
        with opencode_service(out, 'cli-retention', permission_request=ASKS) as case:
            config = json.loads(case.config_path.read_text())
            config['protocol']['events'] = {'retain_last': 3}
            case.config_path.write_text(json.dumps(config))
            case.start()
            for identity in ('run-1', 'run-2'):
                case.submit(identity=identity, delivery_timeout=300)
                waiting(case, identity)
            cli = Cli(case.socket, work)
            board = cli.json('list')
            check(board['counts'] == {'needs approval': 2} and board['gaps'] >= 1,
                  'the board, with a gap, shows two runs needing approval', board)
            desk = cli.json('approvals')
            check(desk['gaps'] >= 1 and desk['walk_complete'] is False,
                  'the walk says it has a gap', desk)
            actions = sorted(a['action_id'] for identity in ('run-1', 'run-2')
                             for a in case.inspect(identity)['actions']
                             if a['state'] == 'pending')
            check(sorted(r['action_id'] for r in desk['waiting']) == actions,
                  'every waiting request is listed after the gap',
                  f"{[r['action_id'] for r in desk['waiting']]} != {actions}")
            lost = [r for r in desk['waiting'] if r.get('lost_to_retention')]
            check(lost and all(r['due'] is None and 'lost to retention' in r['details']
                               for r in lost),
                  'a request the stream lost is marked lost, with no invented deadline', lost)
            human = cli.run('approvals', json_out=False)
            check(human.returncode == 0 and 'no approvals waiting' not in human.stdout
                  and 'no longer holds all of its history' in human.stdout
                  and all(a in human.stdout for a in actions),
                  'a person is told of the gap and shown both requests', human.stdout)
            case.finish()
            return dict(gaps=desk['gaps'], waiting=len(desk['waiting']), lost=len(lost))
    finally:
        case_cleanup.release(work)


def start_fake(work, out, name, duration_ms=600, **protocol):
    """serve-fake with protocol settings of the case's own (faults, events)."""
    config = dict(format='pio-fake-service/1',
                  protocol=dict(format='combraton-conformance-config/1', principal='owner',
                                credentials=[dict(credential=CREDENTIAL)],
                                executor=dict(host_id='durable-fake-host'), **protocol),
                  fake_host=dict(duration_ms=duration_ms, fault=''))
    (work / f'{name}.json').write_text(json.dumps(config))
    socket = work / f'{name}.sock'
    stdout = (out / f'{name}.stdout').open('w')
    stderr = (out / f'{name}.stderr').open('w')
    daemon = subprocess.Popen([str(BINARY), 'serve-fake', '--data-dir', str(work / name),
                               '--config', str(work / f'{name}.json'), '--socket', str(socket)],
                              stdout=stdout, stderr=stderr)
    wait_for(f'{name} to be ready', lambda: 'result' in witness(socket, 'core.describe', {}))
    return daemon, socket, (stdout, stderr)


def case_ledger_exit_codes(out, mutant):
    """submit and reconcile end as the service said: 0 taken, 3 refused, 4
    not known yet (the ledger keeps it pending)."""
    work = private_dir('pio-cg-')
    daemon, socket, files = start_fake(
        work, out, 'ledger',
        faults={'response_internal_error': [{'operation': 'execution.submit', 'times': 1}]})
    try:
        cli = Cli(socket, work)
        store = work / 'caller'
        store.mkdir(mode=0o700)
        request = work / 'run-1.json'
        request.write_text(json.dumps(submit(600, identity='run-1')))
        unknown = cli.run('submit', '--store', str(store), '--request', str(request),
                          json_out=False)
        check(unknown.returncode == 4 and 'pending' in unknown.stdout
              and 'pio client reconcile' in unknown.stdout,
              'an internal_error on submit is not known yet: exit 4, pending, reconcile',
              f'exit {unknown.returncode}: {unknown.stdout}{unknown.stderr}')
        recovered = cli.run('reconcile', '--store', str(store), json_out=False)
        check(recovered.returncode == 0 and 'replay' in recovered.stdout,
              'the reconcile establishes it and replays the first answer: exit 0',
              f'exit {recovered.returncode}: {recovered.stdout}{recovered.stderr}')
        bad = submit(600, identity='run-2')
        bad['command_digest'] = 'sha256:' + '0' * 64
        request2 = work / 'run-2.json'
        request2.write_text(json.dumps(bad))
        refused = cli.run('submit', '--store', str(store), '--request', str(request2),
                          json_out=False)
        check(refused.returncode == 3 and 'refused by the service' in refused.stdout
              and 'digest_mismatch' in refused.stdout,
              'a refusal with retry no exits 3, verbatim',
              f'exit {refused.returncode}: {refused.stdout}{refused.stderr}')
        check('"retry":"no"' in refused.stdout, 'the refusal said retry no', refused.stdout)
        again = cli.run('submit', '--store', str(work / 'caller-2'), '--request', str(request2))
        check(again.returncode == 3 and json.loads(again.stdout)['response']['error']['data']
              ['code'] == 'digest_mismatch', 'with --json the same exit 3', again.stdout)
        return dict(internal_error_exit=4, reconcile_exit=0, refused_exit=3)
    finally:
        stop_fake(daemon, files)
        case_cleanup.release(work)


def case_watch_notices(out, mutant):
    """Retention gaps are said, once each, across a detach. A service that
    keeps only its last four events hands a watcher from the start a gap,
    not the events; the watcher must print it, and a watcher that comes back
    must not print it again (a later, different gap is its own notice)."""
    work = private_dir('pio-cn-')
    daemon, socket, files = start_fake(work, out, 'notices', events={'retain_last': 4})
    try:
        cli = Cli(socket, work)
        for identity in ('run-1', 'run-2', 'run-3'):
            submit_run(cli, work, identity, 600)
        # The stream keeps four events, so the runs are watched by their views.
        exited = lambda identity: witness(socket, 'execution.inspect', {
            'execution': identity})['result']['runtime'] == 'exited'
        wait_for('three runs to finish', lambda: all(
            exited(i) for i in ('run-1', 'run-2', 'run-3')))
        state = work / 'watch.json'
        printed = []

        def lines(text):
            return [json.loads(line) for line in text.splitlines() if line.strip()]

        first = cli.run('watch', '--state', str(state), '--idle-exit', '1')
        check(first.returncode == 0, 'the first watcher ran', first.stderr)
        printed += lines(first.stdout)
        submit_run(cli, work, 'run-4', 600)
        wait_for('run-4 to finish', lambda: exited('run-4'))
        second = cli.run('watch', '--state', str(state), '--idle-exit', '1')
        check(second.returncode == 0, 'the second watcher ran', second.stderr)
        printed += lines(second.stdout)
        gaps = [(l['gap']['to']['epoch'], l['gap']['to']['sequence']) for l in printed
                if 'gap' in l]
        check(gaps, 'a watcher that starts before the retained history prints the gap',
              [sorted(l) for l in printed][:3])
        check(len(gaps) == len(set(gaps)), 'each gap is printed once across a detach', gaps)
        events = [(l['epoch'], l['sequence']) for l in printed if 'sequence' in l]
        check(len(events) == len(set(events)), 'no event is printed twice', events)
        human = cli.run('watch', '--state', str(work / 'human.json'), '--idle-exit', '1',
                        json_out=False)
        check(human.returncode == 0 and '-- retention gap:' in human.stdout,
              'a person is told of the gap', human.stdout[:400])
        return dict(gaps=gaps, events=len(events))
    finally:
        stop_fake(daemon, files)
        case_cleanup.release(work)


def case_watch_other_store(out, mutant):
    """A position saved against one store, used against another: the
    watcher says so and starts from the beginning, rather than failing
    `invalid_cursor` on every run."""
    work = private_dir('pio-co-')
    first, socket_a, files_a = start_fake(work, out, 'store-a')
    second = None
    try:
        cli = Cli(socket_a, work)
        submit_run(cli, work, 'run-a', 600)
        wait_for('run-a to finish', lambda: any(
            e['type'] == 'execution.exit.observed' for e in all_events(socket_a)))
        state = work / 'watch.json'
        # Read to the end, so the saved position holds a cursor of store A's,
        # which store B refuses `invalid_cursor` if it is ever sent.
        before = cli.run('watch', '--state', str(state), '--idle-exit', '1')
        check(before.returncode == 0, 'the watcher ran against store A', before.stderr)
        check(json.loads(state.read_text())['cursor'], 'a cursor of store A is saved')
        saved = json.loads(state.read_text())['stream']
        second, socket_b, files_b = start_fake(work, out, 'store-b')
        cli_b = Cli(socket_b, work)
        submit_run(cli_b, work, 'run-b', 600)
        wait_for('run-b to finish', lambda: any(
            e['type'] == 'execution.exit.observed' for e in all_events(socket_b)))
        after = cli_b.run('watch', '--state', str(state), '--idle-exit', '1')
        check(after.returncode == 0,
              'a position from another store does not fail the watcher',
              f'exit {after.returncode}: {after.stdout}{after.stderr}')
        lines = [json.loads(line) for line in after.stdout.splitlines() if line.strip()]
        notices = [l for l in lines if 'notice' in l]
        check(len(notices) == 1 and saved in notices[0]['notice'],
              'the watcher says the saved position was from another stream', notices)
        events = [(l['epoch'], l['sequence']) for l in lines if 'sequence' in l]
        truth = [(e['epoch'], e['sequence']) for e in all_events(socket_b)]
        check(events == truth, "store B's stream from its beginning, once", f'{events} != {truth}')
        check(json.loads(state.read_text())['stream'] != saved,
              'the position now belongs to store B')
        stop_fake(second, files_b)
        return dict(dropped=saved, events=len(events))
    finally:
        stop_fake(first, files_a)
        if second and second.poll() is None:
            second.kill()
            second.wait()
        case_cleanup.release(work)


CASES = dict(submit_list_inspect_output=case_submit_list_inspect_output,
             watch_detach_resume=case_watch_detach_resume,
             watch_other_store=case_watch_other_store,
             watch_notices=case_watch_notices,
             ledger_exit_codes=case_ledger_exit_codes,
             approvals_answer_and_refusals=case_approvals_answer_and_refusals,
             approvals_after_retention=case_approvals_after_retention,
             allow_on_each_harness=case_allow_on_each_harness,
             cancel=case_cancel)
# Mutants the script plays itself: the command line is sound, and the test
# makes the wrong move to show the refusal is about that move.
MUTANT_CASE = {'cross-run': 'approvals_answer_and_refusals',
               'fresh-epoch': 'approvals_answer_and_refusals',
               'forgot-last': 'watch_detach_resume',
               'saved-page-end': 'watch_detach_resume'}
# Mutants in the source: `pio client` built from a worktree of HEAD with one
# edit, and the named case run against it.
CLI = 'crates/pio-client-cli/src/client_cli.rs'
SOURCE_MUTANTS = {
    # M3 of the review: a refusal read by a person exits 0.
    'human-refusal-exits-0': ('approvals_answer_and_refusals', CLI,
                              'std::process::exit(3);\n}',
                              'std::process::exit(if json { 3 } else { 0 });\n}'),
    # M4 of the review: every answer sends deny.
    'answer-always-deny': ('allow_on_each_harness', CLI,
                           'sent = decision_word(owner, allow);',
                           'sent = decision_word(owner, false);'),
    'approvals-gap-blind': ('approvals_after_retention', CLI,
                            'let lost = if gaps > 0 {', 'let lost = if false {'),
    'ledger-exits-0': ('ledger_exit_codes', CLI,
                       'if status != 0 {', 'if status == 99 {'),
    # M6 of the review: a watcher that prints no gap or epoch notice.
    'watch-drops-notices': ('watch_notices', CLI,
                            'None => print_notice(json, item)?,', 'None => {}'),
    'watch-ignores-stream': ('watch_other_store', CLI,
                             'if state.stream.is_some() || state.cursor.is_some() {\n'
                             '        let probe',
                             'if false {\n        let probe'),
}
MUTANTS = sorted(MUTANT_CASE) + sorted(SOURCE_MUTANTS)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--case', choices=sorted(CASES), action='append')
    parser.add_argument('--mutant', choices=MUTANTS)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    global CLI_BINARY
    if args.mutant in SOURCE_MUTANTS:
        name, relative, old, new = SOURCE_MUTANTS[args.mutant]
        with source_mutant.mutated([(relative, old, new)]) as tree:
            source_mutant.cargo(tree, 'build', '--quiet', '--locked', '-p', 'pio-cli')
        CLI_BINARY = source_mutant.TARGET / 'debug/pio'
        names = [name]
    elif args.mutant:
        names = [MUTANT_CASE[args.mutant]]
    else:
        names = args.case or list(CASES)
    results, failed = {}, []
    for name in names:
        started = time.monotonic()
        (out / name).mkdir(parents=True, exist_ok=True)
        try:
            results[name] = dict(outcome='pass', **CASES[name](out / name, args.mutant))
        except Failed as failure:
            results[name] = dict(outcome='fail', reason=str(failure))
            failed.append(name)
        results[name]['seconds'] = round(time.monotonic() - started, 1)
        print(f"{results[name]['outcome']}  {name}"
              + (f"  ({results[name]['reason']})" if name in failed else ''))
    (out / 'client-cli-matrix.json').write_text(json.dumps(
        dict(format='pio-client-cli-matrix/1', mutant=args.mutant, cases=results),
        indent=2) + '\n')
    if args.mutant:
        if not failed:
            raise SystemExit(f'mutant {args.mutant} SURVIVED')
        reason = results[failed[0]]['reason'].splitlines()[0][:240]
        print(f'mutant {args.mutant} killed: {reason}')
        return
    if failed:
        raise SystemExit(f'{len(failed)} case(s) failed: {failed}')
    print(f'client CLI matrix: {len(results)} cases pass')


if __name__ == '__main__':
    main()
