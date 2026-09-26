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
                                                   'core.effects'), timeout=10, grant=None,
                 execution_features=('execution.controller', 'execution.output',
                                     'execution.discovery')):
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
                           required_features=list(execution_features),
                           optional_features=[])],
            caller=dict(name='pio-board', version='1'),
            receive_limits=dict(max_frame_bytes=1048576)))

    def call(self, envelope):
        # The grant rides on **every** request, query or command. It used to
        # be attached in `query` alone, which was enough for a board that
        # only reads and silently wrong for a lead that submits: the command
        # went out with no grant and came back `grant_required`. The digest
        # covers the intent, and `grant` sits outside it, so attaching it
        # here changes nothing a signature would notice.
        operation = envelope.get('operation', '')
        if self.grant and not operation.startswith(('core.authenticate',
                                                    'core.negotiate')):
            envelope = dict(envelope, grant=self.grant)
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
        return self.call(dict(operation=operation, message_id=str(uuid.uuid4()),
                              payload=payload))

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

    def wait_for_notification(self, seconds=10.0):
        """Block until the service pushes something, or give up.

        The screen is push-driven: it folds because a notification arrived,
        not because a timer fired. Proving that path needs a wait that only
        the push can end.
        """
        import select

        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.stream], [], [],
                                        max(0.0, deadline - time.monotonic()))
            if not ready:
                return None
            line = self.file.readline()
            if not line:
                return None
            message = json.loads(line)
            self.notifications.append(message)
            if message.get('method'):
                return message
        return None

    def inspect(self, identity):
        self.inspect_calls += 1
        return self.query('execution.inspect', {'execution': identity})

    def close(self):
        self.file.close()
        self.stream.close()


def wait_until(predicate, what, seconds=30.0):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            if predicate():
                return True
        except (KeyError, TypeError):
            pass
        time.sleep(0.1)
    raise AssertionError(f'timed out waiting for {what}')


class Board:
    """The screen's model of the world, built only from the fold."""

    def __init__(self, caller):
        self.caller = caller
        self.cursor = None
        self.seen = {}        # execution id -> highest revision the fold reported
        self.drawn = {}       # execution id -> the view last inspected
        self.draws = {}       # execution id -> how many times it was inspected
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
            moved |= self.absorb(result)
            if not result['items']:
                break
            payload = {'limit': 1000, 'kinds': ['execution.execution'],
                       'cursor': self.cursor}
        return moved

    def absorb(self, result):
        """One page of the stream (a `core.events.read` result or a
        notification's params): which subjects moved. No I/O, so the Rust
        port (`pio_client::board`) is checked against it on recorded pages."""
        moved = set()
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
            if 'event' not in item:
                # An epoch change says the stream restarted its numbering;
                # it names no subject.
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
        return moved

    def fold_peek(self):
        """Which subjects the stream would report next, without advancing."""
        answer = self.caller.query('core.events.read',
                                   {'limit': 1000, 'cursor': self.cursor,
                                    'kinds': ['execution.execution']})
        return {item['event']['subject']['id']
                for item in answer['result']['items'] if 'event' in item}

    def draw(self, moved, naive=False):
        """Inspect what changed — or, for the mutant, everything."""
        targets = sorted(self.seen) if naive else sorted(moved)
        for identity in targets:
            answer = self.caller.inspect(identity)
            self.draws[identity] = self.draws.get(identity, 0) + 1
            if 'result' in answer:
                self.drawn[identity] = answer['result']
        return targets


# --- what the board draws -----------------------------------------------------

# The design's one vocabulary (M4-DESIGN-INPUT.md): a glyph and a word for
# every run, so meaning never rides on color alone.
GLYPHS = {'needs approval': '\u25d0', 'uncertain': '\u25c7', 'unknown': '\u25c7',
          'running': '\u25cf', 'refused': '\u2715', 'failed': '\u2715',
          'cancelled': '\u2715', 'finished': '\u25cb'}
# The order the board groups runs in: what needs the person first.
GROUPS = ['needs approval', 'uncertain', 'unknown', 'running', 'refused', 'failed',
          'cancelled', 'finished']


def run_state(view):
    """One word for one run, from its view alone, first match wins.

    A run seen on the stream but not (yet) read has no view, and is
    `unknown` rather than a guess. `uncertain` is the violet state and means
    the **outcome** is in doubt: delivery ambiguous, the host lost (a runtime
    PIO cannot see), or a run that exited with neither an exit status nor a
    result observed. Every run of the labeled fakes exits with `result:
    absent` and an exit code, and its outcome is not in doubt, so an absent
    result counts only beside an unavailable exit.

    Usage PIO has not resolved is **not** an outcome in doubt: it is a usage
    marker on the row (`row_markers`), never the run's state. The
    orchestrator's decision of 2026-09-26, refining the design input's
    "usage unknown" under uncertain; before it, every exited OpenCode run
    read uncertain.
    """
    if view is None:
        return 'unknown'
    if any(a.get('state') == 'pending' for a in view.get('actions') or []):
        return 'needs approval'
    if view.get('delivery') == 'ambiguous' or view.get('runtime') == 'unknown':
        return 'uncertain'
    if view.get('admission') == 'refused':
        return 'refused'
    if view.get('delivery') in ('not_delivered', 'failed_before_delivery'):
        return 'failed'
    if (view.get('cancellation') or {}).get('outcome') == 'cancelled':
        return 'cancelled'
    if view.get('runtime') == 'exited':
        if view.get('exit') == 'unavailable' and (view.get('result') or 'absent') == 'absent':
            return 'uncertain'
        return 'finished'
    return 'running'


def row_markers(view):
    """What a row says beside its state. Usage lives here, never in it."""
    usage = (view or {}).get('usage') or {}
    return ['usage unresolved'] if usage.get('liability') == 'unresolved' else []


def board_view(board):
    """The board as the screen draws it: one row per run the caller can see,
    grouped by state, with the counts and the two attention notes."""
    rows = []
    for identity in sorted(board.seen):
        view = board.drawn.get(identity)
        state = run_state(view)
        pending = [a['action_id'] for a in (view or {}).get('actions') or []
                   if a.get('state') == 'pending']
        rows.append(dict(
            id=identity, revision=board.seen[identity], state=state, glyph=GLYPHS[state],
            drawn_revision=(view or {}).get('revision'),
            admission=(view or {}).get('admission'), runtime=(view or {}).get('runtime'),
            delivery=(view or {}).get('delivery'),
            liability=((view or {}).get('usage') or {}).get('liability'),
            exit=(view or {}).get('exit'), pending_actions=pending,
            markers=row_markers(view)))
    counts = {}
    for row in rows:
        counts[row['state']] = counts.get(row['state'], 0) + 1
    return dict(
        runs=rows,
        groups=[dict(state=state, runs=[r['id'] for r in rows if r['state'] == state])
                for state in GROUPS if counts.get(state)],
        counts=counts,
        notes=dict(approvals=sum(len(r['pending_actions']) for r in rows),
                   uncertain=counts.get('uncertain', 0) + counts.get('unknown', 0)),
        cursor=board.cursor, gaps=len(board.gaps))


def start_service(root, out, events=None, name='daemon', executor=None,
                  credentials=(), duration_ms=1200):
    config = dict(
        format='pio-fake-service/1',
        protocol=dict(format='combraton-conformance-config/1', principal='owner',
                      provider_id=PROVIDER,
                      # Two callers, or more. The principal is the middle
                      # field of the credential, so the board authenticates
                      # as `board` and holds nothing until the owner grants
                      # it something.
                      credentials=[dict(credential=CREDENTIAL),
                                   dict(credential=BOARD_CREDENTIAL)]
                      + [dict(credential=c) for c in credentials],
                      executor=dict(host_id='durable-fake-host', **(executor or {})),
                      **({'events': events} if events else {})),
        fake_host=dict(duration_ms=duration_ms, fault=''))
    config_path = root / 'service.json'
    config_path.write_text(json.dumps(config))
    socket_path = root / 'public.sock'
    stdout = (out / f'{name}.stdout').open('w')
    stderr = (out / f'{name}.stderr').open('w')
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
        # Wait for the state, not for a clock: a sleep long enough to see the
        # run appear is also long enough to see it finish, and then round 3
        # has nothing left to observe.
        wait_until(lambda: 'run-7' in board.fold_peek(), 'run-7 to appear')

        moved = board.fold()
        second_targets = board.draw(moved, naive=naive)
        second_inspects = board_caller.inspect_calls - first_inspects

        # 3. A round the board folds **because a notification arrived**, not
        #    on a timer. The screen is push-driven, so that is the path to
        #    prove rather than the polling one.
        #
        #    A subject that moves *without* being new is not asserted here:
        #    this host's whole lifecycle is shorter than the round trip that
        #    would observe it half-way, so such a claim would be a race
        #    dressed as a test. What is asserted instead is sharper, and is
        #    the whole of G6 — **every run is inspected exactly once across
        #    the entire session**.
        before_push = board_caller.inspect_calls
        owner.call(submit(1200, identity='run-8'))
        woken = board_caller.wait_for_notification(20.0)
        assert woken is not None, 'no notification arrived for a new run'
        assert woken['method'] == 'core.events.notify', woken
        wait_until(lambda: 'run-8' in board.fold_peek(), 'run-8 in the stream')
        moved = board.fold()
        third_targets = board.draw(moved, naive=naive)
        third_inspects = board_caller.inspect_calls - before_push

        assert sorted(board.seen) == [f'run-{i}' for i in range(1, RUNS + 3)], \
            sorted(board.seen)
        # **The assertions that separate a fold from a poll.**
        assert second_targets == ['run-7'], second_targets
        assert second_inspects == 1, (
            f'{second_inspects} inspect calls for 1 changed subject; a board that '
            f"re-reads everything is a poll wearing a fold's clothes")
        # The new run is there, and nothing settled came with it. run-7 may
        # or may not appear again depending on when its last event landed —
        # that is a real race in the harness, not in the board, so it is
        # allowed rather than asserted either way.
        assert 'run-8' in third_targets, third_targets
        assert set(third_targets) <= {'run-7', 'run-8'}, third_targets
        assert third_inspects == len(third_targets), (third_inspects, third_targets)
        # **The whole of G6, and it is deterministic:** every one of the six
        # runs that had already settled before the board attached was
        # inspected exactly once, and never again.
        settled = {f'run-{i}': board.draws.get(f'run-{i}') for i in range(1, RUNS + 1)}
        assert set(settled.values()) == {1}, settled
        # And what it draws of them: every settled run finished, from its
        # view, with no approval waiting and nothing uncertain.
        drawn = board_view(board)
        states = {r['id']: r['state'] for r in drawn['runs']}
        assert all(states[f'run-{i}'] == 'finished' for i in range(1, RUNS + 1)), states
        assert drawn['notes'] == dict(approvals=0, uncertain=0), drawn['notes']

        pushed = board_caller.drain(0.5)
        notified = [n for n in pushed if n.get('method') == 'core.events.notify']
        assert len(notified) >= 1 or woken, 'the subscription pushed nothing'

        # What it drew is current: the revision it holds is the service's.
        for identity, view in board.drawn.items():
            live = owner.query('execution.inspect', {'execution': identity})['result']
            assert view['revision'] <= live['revision'], (identity, view['revision'])
        assert board.drawn['run-7']['execution']['id'] == 'run-7', board.drawn['run-7']

        # The grant is a boundary, not a label: the board reads and may not
        # act. Pinned to the reason, because any error would also pass an
        # unimplemented operation or a malformed envelope.
        refusal = board_caller.call(dict(submit(1200, identity='board-run'),
                                         grant=grant_id))
        assert 'error' in refusal, refusal
        assert refusal['error']['data']['code'] == 'permission_denied', refusal

        record = dict(
            format='pio-board-fold/1',
            runs_discovered_without_being_told_an_id=discovered,
            first_draw_inspects=first_inspects,
            changed_subjects_round_2=second_targets,
            inspects_round_2=second_inspects,
            changed_subjects_round_3=third_targets,
            inspects_round_3=third_inspects,
            round_3_folded_on_a_push=True,
            total_inspects=board_caller.inspect_calls,
            draws_per_run=dict(sorted(board.draws.items())),
            settled_runs_read_exactly_once=True,
            finished_runs_never_read_again=True,
            naive=naive,
            retention_gaps=board.gaps,
            subscription=subscribed['result']['subscription'],
            notifications_pushed=max(len(notified), 1),
            board_may_submit=False,
            board_refusal=refusal['error'].get('data', {}).get('code')
            or refusal['error'].get('message'),
            operations_used=sorted({'core.events.read', 'core.events.subscribe',
                                    'execution.inspect'}),
            new_operations_needed=[],
            board=drawn,
        )
        (out / 'board-fold.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(json.dumps({k: record[k] for k in (
            'runs_discovered_without_being_told_an_id', 'first_draw_inspects',
            'changed_subjects_round_2', 'inspects_round_2',
            'changed_subjects_round_3', 'inspects_round_3',
            'total_inspects', 'draws_per_run', 'notifications_pushed',
            'board_may_submit',
            'new_operations_needed')}, indent=2))
        print('board fold: six runs drawn from the stream, one inspect per changed '
              'subject, finished runs never read again, no new operation')
        owner.close()
        board_caller.close()
        return record
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
        shutil.rmtree(root, ignore_errors=True)


def retention_pass(out, naive=False):
    """The roster from a **snapshot**, when the stream no longer has the events.

    The first pass records `retention_gaps: []`, so the snapshot claim it
    makes is not exercised. Here the service keeps only the last four events,
    so a board attaching afterwards cannot possibly fold six runs out of the
    stream — and is handed `snapshot.subjects[]` instead. A client attaching
    late gets a roster, not a hole.
    """
    out.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='pio-board-ret-', dir='/tmp')).resolve()
    os.chmod(root, 0o700)
    daemon, socket_path, files = start_service(
        root, out, events={'retain_last': 4}, name='daemon-retention')
    try:
        owner = Caller(socket_path, CREDENTIAL,
                       features=('core.events', 'core.capabilities', 'core.effects',
                                 'core.grants'))
        for index in range(1, RUNS + 1):
            assert 'result' in owner.call(submit(1200, identity=f'run-{index}'))
        time.sleep(2.5)

        grant_id = str(uuid.uuid4())
        assert 'result' in owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=grant_id),
            dict(holder='board', audience=PROVIDER,
                 rights=['core.events.read', 'execution.read'],
                 resources=[dict(kind='execution.execution')],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{grant_id}'))

        board_caller = Caller(socket_path, BOARD_CREDENTIAL, grant=grant_id,
                              features=('core.events', 'core.capabilities',
                                        'core.effects', 'core.grants'))
        board = Board(board_caller)
        moved = board.fold(first=True)
        board.draw(moved, naive=naive)

        assert board.gaps, 'no retention gap, so the snapshot path is untested'
        gap = board.gaps[0]
        assert gap['kind'] == 'retention', gap
        from_snapshot = {entry['subject']['id']
                         for entry in gap['snapshot']['subjects']
                         if entry['subject']['kind'] == 'execution.execution'}
        # The roster came from the snapshot, not from events that are gone.
        assert from_snapshot >= {f'run-{i}' for i in range(1, RUNS + 1)}, from_snapshot
        for entry in gap['snapshot']['subjects']:
            assert 'revision' in entry and 'state' in entry, entry
        assert sorted(board.seen) == [f'run-{i}' for i in range(1, RUNS + 1)], \
            sorted(board.seen)
        record = dict(format='pio-board-fold-retention/1',
                      retain_last=4,
                      gaps=len(board.gaps),
                      roster_from_snapshot=sorted(from_snapshot),
                      runs_discovered=sorted(board.seen))
        (out / 'board-fold-retention.json').write_text(
            json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(f'retention: {len(board.gaps)} gap, roster of '
              f'{len(from_snapshot)} from snapshot.subjects[]')
        owner.close()
        board_caller.close()
        return record
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
        shutil.rmtree(root, ignore_errors=True)


def scoped_pass(out):
    """A grant scoped to one run sees one run, and says so.

    The M4b lead holds exactly this shape: a principal that may watch the runs
    it started and nothing else. `filtered: true` is how the stream tells a
    caller that something was withheld rather than absent.
    """
    out.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='pio-board-scope-', dir='/tmp')).resolve()
    os.chmod(root, 0o700)
    daemon, socket_path, files = start_service(root, out, name='daemon-scoped')
    try:
        owner = Caller(socket_path, CREDENTIAL,
                       features=('core.events', 'core.capabilities', 'core.effects',
                                 'core.grants'))
        for index in range(1, 4):
            assert 'result' in owner.call(submit(1200, identity=f'run-{index}'))
        time.sleep(2.5)

        grant_id = str(uuid.uuid4())
        assert 'result' in owner.call(command(
            'core.grant.issue', dict(kind='core.grant', id=grant_id),
            dict(holder='board', audience=PROVIDER,
                 rights=['core.events.read', 'execution.read'],
                 # One run, by prefix.
                 resources=[dict(kind='execution.execution', id_prefix='run-1')],
                 delegation=dict(allowed=False, max_depth=0)),
            command_id=f'grant-{grant_id}'))

        scoped = Caller(socket_path, BOARD_CREDENTIAL, grant=grant_id,
                        features=('core.events', 'core.capabilities',
                                  'core.effects', 'core.grants'))
        answer = scoped.query('core.events.read',
                              {'limit': 1000, 'from': 'start',
                               'kinds': ['execution.execution']})
        result = answer['result']
        seen = {item['event']['subject']['id'] for item in result['items']
                if 'event' in item}
        assert seen == {'run-1'}, seen
        # Withheld, and the stream says so rather than pretending there was
        # nothing there.
        assert result['filtered'] is True, result

        assert 'result' in scoped.query('execution.inspect', {'execution': 'run-1'})
        denied = scoped.query('execution.inspect', {'execution': 'run-2'})
        assert 'error' in denied, denied
        assert denied['error']['data']['code'] == 'permission_denied', denied
        assert denied['error']['data']['details']['reason'] == 'out_of_scope', denied

        record = dict(format='pio-board-fold-scoped/1',
                      resource=dict(kind='execution.execution', id_prefix='run-1'),
                      runs_visible=sorted(seen), filtered=result['filtered'],
                      other_run_refusal='out_of_scope')
        (out / 'board-fold-scoped.json').write_text(
            json.dumps(record, indent=2, sort_keys=True) + '\n')
        print('scoped grant: one run visible, filtered: true, another refused out_of_scope')
        owner.close()
        scoped.close()
        return record
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
    out = args.out.resolve()
    run(out, naive=args.mutant == 'naive')
    retention_pass(out, naive=args.mutant == 'naive')
    scoped_pass(out)


if __name__ == '__main__':
    main()
