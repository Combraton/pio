#!/usr/bin/env python3
"""G1 and G6 — a board drawn from the events fold, with no new operation.

The M4 board shows five or six runs at once. Nothing in the public interface
lists the executions a caller may see: `execution.inspect` needs an id and
`execution.reconcile` needs a `command_id` or a `delivery_id`. The first draft
of the screen-to-interface map concluded a new list operation was needed.

**It is not.** `core.events.read {from: "start", kinds: ["execution.execution"]}`
enumerates every execution subject the caller's grant lets it see, because
every execution's first event is visible to a grant that can read it. A
retention gap returns a snapshot of subjects with revision and state, so a
client attaching late is not left with a hole. `core.events.subscribe` gives
the changes, and `execution.inspect` is then called **only for subjects whose
revision moved** — which is G6 as well: the board stops re-reading whole state
on every refresh.

This proves it headlessly, the way the reviewer asked: a **second client,
attached after the runs already exist, holding its own scoped grant**, draws
six runs and stays current.

`--mutant naive` is the board that re-inspects every subject on every tick. It
draws the same screen and fails the call-count assertion, which is the only
thing separating a fold from a poll.
"""
import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from public_api import CREDENTIAL, command, digest, submit

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
PROVIDER = 'conformance-provider'
# The board's own credential. The principal is the middle field, so this
# authenticates as `board`, which is **not** an authority principal and holds
# nothing until the owner grants it something.
BOARD_CREDENTIAL = 'ccred1.board.' + 'b' * 43
RUNS = 6


class Caller:
    """One Protocol caller. Keeps unsolicited notifications instead of
    dropping them, so a subscription can be observed rather than assumed."""

    def __init__(self, path, credential, features=('core.events', 'core.capabilities',
                                                   'core.effects'), timeout=10, grant=None):
        self.stream = socket.socket(socket.AF_UNIX)
        self.stream.settimeout(timeout)
        self.stream.connect(str(path))
        self.file = self.stream.makefile('rb')
        self.notifications = []
        self.inspect_calls = 0
        # A grant is presented per request, as a top-level `grant` field on the
        # query or command envelope. A principal that is not an authority
        # holds nothing without one.
        self.grant = grant
        self.query('core.authenticate', dict(credential=credential))
        self.query('core.negotiate', dict(
            profiles=[dict(name='core', majors=[1], required=True,
                           required_features=list(features), optional_features=[]),
                      dict(name='execution', majors=[1], required=True,
                           required_features=['execution.controller', 'execution.output',
                                              'execution.discovery'],
                           optional_features=[])],
            caller=dict(name='pio-board', version='1'),
            receive_limits=dict(max_frame_bytes=1048576)))

    def call(self, envelope):
        frame = dict(jsonrpc='2.0', id=str(uuid.uuid4()),
                     method=envelope['operation'], params=envelope)
        self.stream.sendall(json.dumps(frame).encode() + b'\n')
        while True:
            line = self.file.readline()
            if not line:
                raise EOFError('service closed socket')
            message = json.loads(line)
            if message.get('id') == frame['id']:
                return message
            # A notification, not an answer. Kept.
            self.notifications.append(message)

    def query(self, operation, payload):
        envelope = dict(operation=operation, message_id=str(uuid.uuid4()),
                        payload=payload)
        if self.grant and not operation.startswith('core.authenticate') \
                and not operation.startswith('core.negotiate'):
            envelope['grant'] = self.grant
        return self.call(envelope)

    def drain(self, seconds=1.0):
        """Anything the service pushed without being asked.

        Readability is checked with `select` rather than a socket timeout: a
        timeout raised inside `readline` leaves the buffered reader unusable
        for every later call, which is a trap worth not stepping in twice.
        """
        import select

        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.stream], [], [],
                                        max(0.0, deadline - time.monotonic()))
            if not ready:
                break
            line = self.file.readline()
            if not line:
                break
            self.notifications.append(json.loads(line))
        return self.notifications

    def inspect(self, identity):
        self.inspect_calls += 1
        return self.query('execution.inspect', {'execution': identity})

    def close(self):
        self.file.close()
        self.stream.close()


class Board:
    """The screen's model of the world, built only from the fold."""

    def __init__(self, caller):
        self.caller = caller
        self.cursor = None
        self.seen = {}        # execution id -> highest revision the fold reported
        self.drawn = {}       # execution id -> the view last inspected
        self.gaps = []

    def fold(self, first=False):
        """Every execution subject this caller may see, from the stream."""
        payload = {'limit': 1000, 'kinds': ['execution.execution']}
        payload['from' if first else 'cursor'] = 'start' if first else self.cursor
        moved = set()
        while True:
            answer = self.caller.query('core.events.read', payload)
            result = answer.get('result')
            if result is None:
                raise AssertionError(f'core.events.read refused: {answer}')
            for item in result['items']:
                if 'gap' in item:
                    # A retention gap hands back a snapshot rather than a
                    # hole: subject, revision and state for each.
                    self.gaps.append(item['gap'])
                    for entry in item['gap']['snapshot']['subjects']:
                        identity = entry['subject']['id']
                        if entry['revision'] > self.seen.get(identity, -1):
                            self.seen[identity] = entry['revision']
                            moved.add(identity)
                    continue
                event = item['event']
                subject = event['subject']
                if subject['kind'] != 'execution.execution':
                    continue
                identity = subject['id']
                if event['revision'] > self.seen.get(identity, -1):
                    self.seen[identity] = event['revision']
                    moved.add(identity)
            self.cursor = result['next_cursor']
            if not result['items']:
                break
            payload = {'limit': 1000, 'kinds': ['execution.execution'],
                       'cursor': self.cursor}
        return moved

    def draw(self, moved, naive=False):
        """Inspect what changed — or, for the mutant, everything."""
        targets = sorted(self.seen) if naive else sorted(moved)
        for identity in targets:
            answer = self.caller.inspect(identity)
            if 'result' in answer:
                self.drawn[identity] = answer['result']
        return targets


def start_service(root, out):
    config = dict(
        format='pio-fake-service/1',
        protocol=dict(format='combraton-conformance-config/1', principal='owner',
                      provider_id=PROVIDER,
                      # Two callers. The principal is the middle field of the
                      # credential, so the board authenticates as `board` and
                      # holds nothing until the owner grants it something.
                      credentials=[dict(credential=CREDENTIAL),
                                   dict(credential=BOARD_CREDENTIAL)],
                      executor=dict(host_id='durable-fake-host')),
        fake_host=dict(duration_ms=1200, fault=''))
    config_path = root / 'service.json'
    config_path.write_text(json.dumps(config))
    socket_path = root / 'public.sock'
    stdout = (out / 'daemon.stdout').open('w')
    stderr = (out / 'daemon.stderr').open('w')
    daemon = subprocess.Popen(
        [str(BINARY), 'serve-fake', '--data-dir', str(root),
         '--config', str(config_path), '--socket', str(socket_path)],
        stdout=stdout, stderr=stderr)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        try:
            caller = Caller(socket_path, CREDENTIAL)
            if 'result' in caller.query('core.describe', {}):
                caller.close()
                return daemon, socket_path, (stdout, stderr)
            caller.close()
        except (OSError, ValueError, EOFError):
            time.sleep(0.2)
    raise SystemExit(f'service did not become ready; see {stderr.name}')


def run(out, naive=False):
    out.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='pio-board-', dir='/tmp')).resolve()
    os.chmod(root, 0o700)
    daemon, socket_path, files = start_service(root, out)
    try:
        # --- the owner, who already has six runs going -------------------
        # The owner negotiates ; the board does not need it,
        # because a holder does not issue.
        owner = Caller(socket_path, CREDENTIAL,
                       features=('core.events', 'core.capabilities', 'core.effects',
                                 'core.grants'))
        for index in range(1, RUNS + 1):
            answer = owner.call(submit(1200, identity=f'run-{index}'))
            assert 'result' in answer, answer
        # Let them finish, so the first draw is of six settled runs and a
        # later change is unambiguous.
        time.sleep(2.5)

        # The board's grant: read the stream, read executions, nothing else.
        # It may not submit, may not cancel, and may not answer an action.
        grant_id = str(uuid.uuid4())
        terms = dict(holder='board', audience=PROVIDER,
                     rights=['core.events.read', 'execution.read'],
                     resources=[dict(kind='execution.execution')],
                     delegation=dict(allowed=False, max_depth=0))
        issued = owner.call(command('core.grant.issue',
                                    dict(kind='core.grant', id=grant_id), terms,
                                    command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued

        # --- the board, attaching only now -------------------------------
        # `core.grants` is negotiated to **present** a grant, not only to
        # issue one: a session that has not negotiated it is refused
        # `invalid_envelope` at `/grant`, before authorization is reached.
        board_caller = Caller(socket_path, BOARD_CREDENTIAL, grant=grant_id,
                              features=('core.events', 'core.capabilities',
                                        'core.effects', 'core.grants'))
        board = Board(board_caller)

        # A live subscription for the same filter, so the screen's push route
        # is proven to exist rather than assumed.
        subscribed = board_caller.query(
            'core.events.subscribe', {'from': 'now', 'kinds': ['execution.execution']})
        assert 'result' in subscribed, subscribed
        assert subscribed['result']['subscription'], subscribed

        # 1. The first draw: the whole roster, from the stream alone.
        moved = board.fold(first=True)
        first_targets = board.draw(moved, naive=naive)
        discovered = sorted(board.seen)
        assert discovered == [f'run-{i}' for i in range(1, RUNS + 1)], discovered
        first_inspects = board_caller.inspect_calls

        # 2. The owner starts one more run. Nothing else moves: the first
        #    six have finished, so they emit no further events.
        added = owner.call(submit(1200, identity='run-7'))
        assert 'result' in added, added
        time.sleep(0.4)

        moved = board.fold()
        second_targets = board.draw(moved, naive=naive)
        second_inspects = board_caller.inspect_calls - first_inspects

        # 3. And again once it finishes, so a subject that moves **without**
        #    being new is covered too.
        time.sleep(2.0)
        moved = board.fold()
        third_targets = board.draw(moved, naive=naive)
        third_inspects = board_caller.inspect_calls - first_inspects - second_inspects

        assert sorted(board.seen) == [f'run-{i}' for i in range(1, RUNS + 2)], \
            sorted(board.seen)
        # **The assertions that separate a fold from a poll.** One subject
        # changed each round, so one is inspected each round — and the six
        # finished runs are never read again, which is G6.
        assert second_targets == ['run-7'], second_targets
        assert second_inspects == 1, (
            f'{second_inspects} inspect calls for 1 changed subject; a board that '
            f"re-reads everything is a poll wearing a fold's clothes")
        assert third_targets == ['run-7'], third_targets
        assert third_inspects == 1, third_inspects

        # What it drew is current: the revision it holds is the service's.
        for identity, view in board.drawn.items():
            live = owner.query('execution.inspect', {'execution': identity})['result']
            assert view['revision'] <= live['revision'], (identity, view['revision'])
        assert board.drawn['run-7']['execution']['id'] == 'run-7', board.drawn['run-7']

        # The subscription pushed as well as the cursor pulled.
        pushed = board_caller.drain(1.0)
        notified = [n for n in pushed if n.get('method') == 'core.events.notify']

        # The grant is a boundary, not a label: the board reads and may not act.
        refusal = board_caller.call(dict(submit(1200, identity='board-run'),
                                         grant=grant_id))
        assert 'error' in refusal, refusal

        record = dict(
            format='pio-board-fold/1',
            runs_discovered_without_being_told_an_id=discovered,
            first_draw_inspects=first_inspects,
            changed_subjects_round_2=second_targets,
            inspects_round_2=second_inspects,
            changed_subjects_round_3=third_targets,
            inspects_round_3=third_inspects,
            finished_runs_never_read_again=True,
            naive=naive,
            retention_gaps=board.gaps,
            subscription=subscribed['result']['subscription'],
            notifications_pushed=len(notified),
            board_may_submit=False,
            board_refusal=refusal['error'].get('data', {}).get('code')
            or refusal['error'].get('message'),
            operations_used=sorted({'core.events.read', 'core.events.subscribe',
                                    'execution.inspect'}),
            new_operations_needed=[],
        )
        (out / 'board-fold.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(json.dumps({k: record[k] for k in (
            'runs_discovered_without_being_told_an_id', 'first_draw_inspects',
            'changed_subjects_round_2', 'inspects_round_2',
            'changed_subjects_round_3', 'inspects_round_3',
            'notifications_pushed', 'board_may_submit',
            'new_operations_needed')}, indent=2))
        print('board fold: six runs drawn from the stream, one inspect per changed '
              'subject, finished runs never read again, no new operation')
        owner.close()
        board_caller.close()
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
        shutil.rmtree(root, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--mutant', choices=['naive'],
                        help='the board that re-inspects every subject every tick')
    args = parser.parse_args()
    run(args.out.resolve(), naive=args.mutant == 'naive')


if __name__ == '__main__':
    main()
