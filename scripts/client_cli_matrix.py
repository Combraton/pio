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
- `approvals_answer_and_refusals` (opencode): two runs waiting, drawn by
  `client list` and walked by `client approvals`; an answer aimed at the
  wrong run is refused `not_found`, one fenced at a stale epoch is refused
  `stale_authority_epoch`, each printed as the service sent it, exit 3, and
  both runs still waiting afterwards; then a real answer, on the stream as
  the caller's.
- `cancel` (opencode): a waiting run cancelled with `client cancel`, and
  exited afterwards; `client steer` on a harness with no steering is refused
  in the service's words.

Mutants, each required to fail its named assertion:

- `cross-run` aims the wrong-run answer at its own run, so the refusal is
  shown to come from the mismatch and not from a malformed answer;
- `fresh-epoch` sends the stale answer without `--epoch`, so the refusal is
  shown to come from the pinned epoch;
- `forgot-last` erases the delivered position between two watchers (a
  watcher that kept only the page cursor), and events print twice;
- `saved-page-end` moves the saved cursor to the end of the page a watcher
  stopped inside (a watcher that saved `next_cursor` early), and events are
  lost.
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
from public_api import CREDENTIAL, Client, submit

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
MUTANTS = ('cross-run', 'fresh-epoch', 'forgot-last', 'saved-page-end')


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
        return [str(BINARY), 'client', *args, '--socket', str(self.socket),
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
            case.finish()
            return dict(cancel=cancelled['outcome']['receipt'],
                        cancellation=view['cancellation'].get('outcome'), exit=view['exit'])
    finally:
        case_cleanup.release(work)


CASES = dict(submit_list_inspect_output=case_submit_list_inspect_output,
             watch_detach_resume=case_watch_detach_resume,
             approvals_answer_and_refusals=case_approvals_answer_and_refusals,
             cancel=case_cancel)
MUTANT_CASE = {'cross-run': 'approvals_answer_and_refusals',
               'fresh-epoch': 'approvals_answer_and_refusals',
               'forgot-last': 'watch_detach_resume',
               'saved-page-end': 'watch_detach_resume'}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--case', choices=sorted(CASES), action='append')
    parser.add_argument('--mutant', choices=MUTANTS)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    names = [MUTANT_CASE[args.mutant]] if args.mutant else (args.case or list(CASES))
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
