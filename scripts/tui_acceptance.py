#!/usr/bin/env python3
"""`pio tui` in a real terminal: tmux panes driven by keystrokes, read as text.

M4 T2's acceptance. The screen is launched inside a tmux pane of a private
tmux server (`tmux -L`, no configuration file), at 80x24 and at 120x40,
against a real service: `serve-fake`, and `serve-opencode` with its labeled
fake for approvals. It is driven with `tmux send-keys` and read with `tmux
capture-pane -p`, and every step's frame is saved and asserted on. What the
frames say is checked against an independent reader of the same socket (the
Python test caller), never against the screen's own idea of itself.

Each pane is recorded as an asciicast v2 file (`.cast`, JSON lines) without
asciinema: `tmux pipe-pane -O` hands the pane's output bytes to this script
(`--stamp`), which timestamps them; the keys sent and the resizes are merged
in as `i` and `r` events. The screen runs on the pane's own terminal, so the
`stty -g` read in the pane before and after it is the terminal it changed.

Scenarios:

- `board_live` (serve-fake, 80x24 then 120x40 then 80x24): two finished runs
  drawn with their state words; a run submitted while the screen is open
  appears as running and then turns finished, with no key pressed (the
  event stream, read from the board's cursor); the narrow layout keeps every
  state word and both attention notes and drops delivery detail; opening a
  run shows its identity (execution id, delivery, runtime, exit, usage and
  liability) and its transcript blocks, and `e` folds the truth line open;
  a resize to 120x40 puts the preview beside the runs with the delivery
  evidence back, and a resize back drops it again; `?` shows the keys; `q`
  detaches, and `stty -g` is the same before and after, the alternate screen
  is left and the cursor is shown.
- `approvals` (serve-opencode with its labeled fake, 120x40): two runs
  needing approval, bold on the board, on the amber note with their
  deadlines; `a` walks to one; its run view shows the tool use with what it
  was aimed at, `not yet classified` and `decider unknown` (no guess), and
  WAITING FOR YOU with the time left. Answered by an independent caller, the
  run exits and the audit fills the blanks in place (`not classifiable`,
  decided by the caller); the run reads **finished with the marker usage
  unresolved, never uncertain** (the orchestrator's rule). `]` goes to the
  other run; `c` then `n` sends nothing (the run still waits, checked on the
  socket); `c` then `y` cancels it, and the screen's word for it is the
  word the fold gives the service's own view.
- `uncertain` (serve-fake whose host is lost after release, 80x24): the run
  reads uncertain, the violet note says host lost, `u` selects it, and its
  run view says runtime unknown and exit unavailable, with usage unknown
  rather than zero.
- `signals` (serve-fake, 80x24): SIGTERM, SIGINT, and a panic (debug builds
  only: `PIO_TUI_TEST_PANIC=1` and `!`), each in its own pane; after each,
  `stty -g` is the same, the alternate screen is left and the cursor shown.

Mutants, each built from a worktree of HEAD with one edit
(`source_mutant.py`; the services still run from the checkout), each
required to fail its named assertion:

- `state-word`: a row carrying a marker reads uncertain (the T1 first cut's
  rule, in the screen), and `approvals` fails "unresolved usage is a marker";
- `missed-restore`: raw mode is never turned off, and `board_live` fails
  "stty -g is the same before and after";
- `ignores-new-events`: the stream is read once and never again, and
  `board_live` fails "a run submitted while the screen is open appears".

    tui_acceptance.py --out DIR [--scenario NAME]... [--mutant NAME]

Frames go to DIR/frames/SCENARIO/, casts to DIR/*.cast, the summary to
DIR/tui-acceptance.json.
"""
import argparse
import codecs
import json
import os
import re
import shlex
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
SCRIPT = Path(__file__).resolve()
# The services always run from the checkout's binary; the screen runs from
# TUI_BINARY, which a mutant replaces with one built from a mutated tree.
BINARY = ROOT / 'target/debug/pio'
TUI_BINARY = BINARY
GLYPHS = '\u25cf\u25d0\u25c7\u25cb\u2715?'
LABEL = f'pio-tui-{os.getpid()}'


class Failed(AssertionError):
    pass


def check(condition, what, detail=''):
    if not condition:
        raise Failed(f'{what}: {detail}' if detail else what)


def private_dir(prefix):
    path = Path(tempfile.mkdtemp(prefix=prefix, dir='/tmp')).resolve()
    os.chmod(path, 0o700)
    return path


def utf8_env(extra=None):
    env = dict(os.environ)
    if 'UTF-8' not in (env.get('LC_ALL') or env.get('LANG') or '').upper():
        env['LC_ALL'] = 'C.UTF-8' if sys.platform.startswith('linux') else 'en_US.UTF-8'
    env.update(extra or {})
    return env


def tmux(*args, check_=True, env=None):
    result = subprocess.run(['tmux', '-u', '-L', LABEL, '-f', '/dev/null', *args],
                            capture_output=True, text=True, env=env or utf8_env())
    if check_ and result.returncode != 0:
        raise Failed(f'tmux {" ".join(args)} failed: {result.stderr.strip()}')
    return result.stdout


def wait_for(what, probe, seconds=30):
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


# --- frames ------------------------------------------------------------------------

def row(frame, run):
    """The runs card's line for one run: its glyph, then its id."""
    pattern = re.compile(rf'[{GLYPHS}] {re.escape(run)}\s')
    return next((line for line in frame.splitlines() if pattern.search(line)), '')


PREVIEW = re.compile('\u256d (\\S+)  [' + GLYPHS + '] ')
SELECTED = re.compile('\u258c[' + GLYPHS + '] (\\S+)')


def previewed(frame):
    """The run the preview card is titled with (its name chip)."""
    found = PREVIEW.search(frame)
    return found.group(1) if found else None


def selected(frame):
    """The run the runs card's selection bar is on."""
    found = SELECTED.search(frame)
    return found.group(1) if found else None


def header(frame):
    return next((line for line in frame.splitlines() if '\u2190 board' in line), '')


def line_with(frame, needle):
    return next((line for line in frame.splitlines() if needle in line), '')


def one_line(text):
    return ' '.join(text.split())


# --- a pane --------------------------------------------------------------------------

class Pane:
    """One `pio tui` in one tmux pane, recorded, its frames kept."""

    count = 0

    def __init__(self, out, name, socket, width, height, env=None):
        Pane.count += 1
        self.name = name
        self.session = f'{name}-{os.getpid()}-{Pane.count}'
        self.dir = private_dir('pio-tu-')
        self.frames = out / 'frames' / name
        self.frames.mkdir(parents=True, exist_ok=True)
        self.cast = out / f'{name}.cast'
        self.size = (width, height)
        self.inputs, self.resizes, self.steps = [], [], []
        self.started = None
        self.socket = socket
        self.env = env or {}
        self.closed = False

    def start(self):
        credential = self.dir / 'credential'
        credential.write_text(CREDENTIAL)
        credential.chmod(0o600)
        d = shlex.quote(str(self.dir))
        exports = ''.join(f'export {k}={shlex.quote(v)}\n' for k, v in self.env.items())
        (self.dir / 'run.sh').write_text(f'''#!/bin/sh
{exports}while [ ! -e {d}/go ]; do sleep 0.05; done
stty -g > {d}/stty-before
{shlex.quote(str(TUI_BINARY))} tui --socket {shlex.quote(str(self.socket))} \\
    --credential-file {shlex.quote(str(credential))}
rc=$?
stty -g > {d}/stty-after
echo $rc > {d}/rc
if cmp -s {d}/stty-before {d}/stty-after; then
  echo "pio tui exited $rc; stty -g unchanged"
else
  echo "pio tui exited $rc; stty -g CHANGED"
fi
exec sleep 600
''')
        width, height = self.size
        tmux('new-session', '-d', '-s', self.session, '-x', str(width), '-y', str(height),
             f'sh {shlex.quote(str(self.dir / "run.sh"))}')
        stamper = ' '.join(shlex.quote(p) for p in (
            sys.executable, str(SCRIPT), '--stamp', str(self.dir / 'raw.jsonl'),
            str(self.dir / 'stamping')))
        tmux('pipe-pane', '-O', '-t', self.target, stamper)
        wait_for('the recorder to start', lambda: (self.dir / 'stamping').exists(), 10)
        self.started = time.time()
        (self.dir / 'go').touch()
        return self

    @property
    def target(self):
        return f'={self.session}:'

    def frame(self):
        return tmux('capture-pane', '-p', '-t', self.target)

    def save(self, step, frame):
        self.steps.append(step)
        path = self.frames / f'{len(self.steps):02d}-{step}.txt'
        path.write_text(frame)
        return frame

    def wait(self, step, predicate, seconds=20):
        """The first frame for which `predicate` holds, saved as `step`; a
        timeout fails the step with the last frame saved beside it."""
        deadline = time.monotonic() + seconds
        frame = ''
        while time.monotonic() < deadline:
            frame = self.frame()
            if predicate(frame):
                return self.save(step, frame)
            time.sleep(0.15)
        self.save(f'{step}-TIMEOUT', frame)
        raise Failed(f'{step}: no frame within {seconds}s; the last one:\n{frame}')

    def keys(self, *keys):
        self.inputs.append((time.time(), ' '.join(keys)))
        tmux('send-keys', '-t', self.target, *keys)

    def resize(self, width, height):
        tmux('resize-window', '-t', f'={self.session}', '-x', str(width), '-y', str(height))
        self.resizes.append((time.time(), f'{width}x{height}'))

    def tui_pid(self):
        shell = int(tmux('display-message', '-p', '-t', self.target, '#{pane_pid}').strip())
        table = subprocess.run(['ps', '-A', '-o', 'pid=', '-o', 'ppid=', '-o', 'command='],
                               capture_output=True, text=True).stdout
        for line in table.splitlines():
            pid, ppid, command = line.split(None, 2)
            if int(ppid) == shell and ' tui ' in f'{command} ':
                return int(pid)
        raise Failed('the pio tui process was not found under the pane')

    def exited(self, step, seconds=15):
        """After the screen leaves: its exit status, whether `stty -g` is
        the same, and what tmux says of the alternate screen and cursor."""
        rc = wait_for('the screen to exit', lambda: (self.dir / 'rc').read_text().strip(),
                      seconds)
        frame = self.wait(step, lambda f: 'pio tui exited' in f)
        before = (self.dir / 'stty-before').read_text().strip()
        after = (self.dir / 'stty-after').read_text().strip()
        alternate, cursor = tmux('display-message', '-p', '-t', self.target,
                                 '#{alternate_on} #{cursor_flag}').split()
        return dict(rc=int(rc), stty_before=before, stty_after=after, same=before == after,
                    alternate_on=alternate, cursor_flag=cursor, frame=frame)

    def close(self):
        if self.closed:
            return
        self.closed = True
        tmux('kill-session', '-t', f'={self.session}', check_=False)
        done = self.dir / 'stamping.done'
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and not done.exists():
            time.sleep(0.05)
        self.write_cast()
        case_cleanup.release(self.dir)

    def write_cast(self):
        raw = self.dir / 'raw.jsonl'
        if self.started is None or not raw.exists():
            return
        t0 = self.started
        events = [(t, 'o', text) for t, text in map(json.loads, raw.read_text().splitlines())]
        events += [(t, 'i', keys) for t, keys in self.inputs]
        events += [(t, 'r', size) for t, size in self.resizes]
        events.sort(key=lambda e: e[0])
        width, height = self.size
        head = dict(version=2, width=width, height=height, timestamp=int(t0),
                    title=f'pio tui: {self.name}',
                    env=dict(TERM='tmux-256color', SHELL='/bin/sh'))
        with self.cast.open('w') as out:
            out.write(json.dumps(head) + '\n')
            for t, kind, data in events:
                out.write(json.dumps([round(max(t - t0, 0), 6), kind, data]) + '\n')


def stamp(raw, ready):
    """The recorder: the pane's output bytes, timestamped, until EOF."""
    decoder = codecs.getincrementaldecoder('utf-8')('replace')
    with open(raw, 'w') as out:
        Path(ready).touch()
        while True:
            data = os.read(0, 65536)
            text = decoder.decode(data, final=not data)
            if text:
                out.write(json.dumps([time.time(), text]) + '\n')
                out.flush()
            if not data:
                break
    Path(f'{ready}.done').touch()


def restored(result, what):
    check(result['same'], f'{what}: stty -g is the same before and after',
          f"{result['stty_before']} != {result['stty_after']}")
    check(result['alternate_on'] == '0', f'{what}: the alternate screen is left',
          result['alternate_on'])
    check(result['cursor_flag'] == '1', f'{what}: the cursor is shown', result['cursor_flag'])


# --- services ------------------------------------------------------------------------

class FakeService:
    """serve-fake, with the host's run length and fault of the case's own."""

    def __init__(self, out, name, duration_ms, fault=''):
        self.work = private_dir('pio-tf-')
        config = dict(format='pio-fake-service/1',
                      protocol=dict(format='combraton-conformance-config/1', principal='owner',
                                    credentials=[dict(credential=CREDENTIAL)],
                                    executor=dict(host_id='durable-fake-host')),
                      fake_host=dict(duration_ms=duration_ms, fault=fault))
        (self.work / 'service.json').write_text(json.dumps(config))
        self.socket = self.work / 'public.sock'
        self.files = [(out / f'{name}.stdout').open('w'), (out / f'{name}.stderr').open('w')]
        self.daemon = subprocess.Popen(
            [str(BINARY), 'serve-fake', '--data-dir', str(self.work / 'store'), '--config',
             str(self.work / 'service.json'), '--socket', str(self.socket)],
            stdout=self.files[0], stderr=self.files[1])
        wait_for(f'{name} to be ready', lambda: 'result' in self.query('core.describe', {}))

    def query(self, operation, payload):
        with Client(self.socket) as client:
            return client.query(operation, payload)

    def submit(self, identity):
        with Client(self.socket) as client:
            answer = client.call(submit(600, identity=identity))
        check(answer['result']['outcome']['admission'] == 'admitted',
              f'{identity} was admitted', answer)

    def inspect(self, identity):
        return self.query('execution.inspect', {'execution': identity})['result']

    def close(self):
        self.daemon.kill()
        self.daemon.wait(timeout=10)
        for handle in self.files:
            handle.close()
        case_cleanup.release(self.work)


def fold_state(view):
    """The fold's word for a view, from the Python fold (the independent
    reader), to compare with what the screen says."""
    from board_fold import run_state
    return run_state(view)


# --- scenarios -----------------------------------------------------------------------

def open_run(pane, run, step):
    """Select `run` with the arrow keys, by where the selection bar is, and
    open it: `Home`, then down until the bar is on it."""
    pane.keys('Home')
    for _ in range(12):
        time.sleep(0.3)
        if selected(pane.frame()) == run:
            break
        pane.keys('Down')
    check(selected(pane.frame()) == run, f'the arrow keys reach {run}', pane.frame())
    pane.keys('Enter')
    return pane.wait(step, lambda f: run in header(f))


def narrow_readable(frame, runs, what):
    lines = frame.splitlines()
    check(len(lines) <= 24 and all(len(l) <= 80 for l in lines),
          f'{what}: the frame fits 80x24', f'{len(lines)} lines')
    for run, state in runs.items():
        check(state in row(frame, run), f'{what}: {run} reads {state} in full', row(frame, run))
    for needle in ('need you', 'uncertain \u00b7 press u', 'done', 'q detach'):
        check(needle in frame, f'{what}: the notes, the status bar and the keys stay', needle)


def scenario_board_live(out):
    service = FakeService(out, 'board-live', duration_ms=4000)
    pane = None
    try:
        for run in ('run-a', 'run-b'):
            service.submit(run)
        wait_for('run-a and run-b to finish', lambda: all(
            service.inspect(r)['runtime'] == 'exited' for r in ('run-a', 'run-b')), 30)
        pane = Pane(out, 'board-live', service.socket, 80, 24).start()
        frame = pane.wait('board', lambda f: 'finished' in row(f, 'run-a')
                          and 'finished' in row(f, 'run-b'))
        check('\u25cb' in row(frame, 'run-a'), 'a finished run has its glyph beside its word',
              row(frame, 'run-a'))
        check(frame.startswith('pio \u00b7 '), 'the top line names the product', frame[:80])
        check('\u25cb 2 done' in frame and '0 need you' in frame and '0 uncertain' in frame,
              'the status bar and the two notes', frame)
        narrow_readable(frame, {'run-a': 'finished', 'run-b': 'finished'}, 'narrow board')
        check('RUNS' in frame and previewed(frame)
              and previewed(frame) not in line_with(frame, 'RUNS'),
              'narrow: the preview sits underneath the runs', line_with(frame, 'RUNS'))
        check('child_release_marker' not in frame,
              'narrow: delivery detail is the first thing dropped', frame)

        # A run submitted while the screen is open, and no key pressed.
        service.submit('run-c')
        frame = pane.wait('new-run-running', lambda f: 'run-c' in row(f, 'run-c'), 15)
        check('running' in row(frame, 'run-c') or 'finished' in row(frame, 'run-c'),
              'a run submitted while the screen is open appears', row(frame, 'run-c'))
        seen_running = 'running' in row(frame, 'run-c')
        frame = pane.wait('new-run-finished', lambda f: 'finished' in row(f, 'run-c'), 20)
        check(service.inspect('run-c')['runtime'] == 'exited',
              'the screen says finished only after the service does')

        frame = open_run(pane, 'run-a', 'run-view')
        text = one_line(frame)
        for needle in ('\u2190 board', 'run-a', 'delivery delivered', 'runtime exited',
                       'exit 0', 'result absent', 'liability none', 'tokens unknown',
                       'deterministic fake work started', 'deterministic fake work ended'):
            check(needle in text, f'the run view shows {needle!r}', frame)
        check('0 tokens' not in text, 'usage never observed is unknown, never zero', frame)
        pane.keys('e')
        frame = pane.wait('evidence', lambda f: 'execution run-a' in f)
        check('by child_release_marker' in one_line(frame) and 'revision' in frame,
              'the truth line folds open into its evidence', frame)
        pane.keys('Escape')
        pane.wait('back-to-board', lambda f: 'RUNS' in f and '\u2190 board' not in f)

        pane.resize(120, 40)
        frame = pane.wait('wide', lambda f: len(f.splitlines()) >= 39
                          and max(map(len, f.splitlines())) > 100)
        runs_line = line_with(frame, 'RUNS')
        check(previewed(frame) and previewed(frame) in runs_line,
              'wide: the preview sits beside the runs', runs_line)
        check('by child_release_marker' in frame, 'wide: the delivery evidence is back', frame)
        pane.resize(80, 24)
        frame = pane.wait('narrow-again', lambda f: max(map(len, f.splitlines())) <= 80
                          and 'run-a' in row(f, 'run-a'))
        narrow_readable(frame, {'run-a': 'finished', 'run-b': 'finished',
                                'run-c': 'finished'}, 'resized back to 80x24')
        check('child_release_marker' not in frame, 'narrow again: delivery detail dropped')

        pane.keys('?')
        pane.wait('keys', lambda f: 'KEYS' in f and 'detach' in f)
        pane.keys('Escape')
        pane.wait('keys-closed', lambda f: 'KEYS' not in f)
        pane.keys('q')
        result = pane.exited('detached')
        check(result['rc'] == 0, 'q detaches with exit 0', result['rc'])
        restored(result, 'after q')
        check('PIO: detached' in result['frame'], 'the detach line is printed', result['frame'])
        return dict(stty=result['stty_before'], run_c_seen_running=seen_running,
                    frames=pane.steps)
    finally:
        if pane:
            pane.close()
        service.close()


def scenario_approvals(out):
    from approval_desk import ASKS, service as opencode_service
    pane = None
    with opencode_service(out, 'tui-approvals', permission_request=ASKS) as case:
        try:
            case.start()
            for identity in ('run-1', 'run-2'):
                case.submit(identity=identity, delivery_timeout=300)
                wait_for(f'{identity} to wait', lambda: (
                    lambda v: v['runtime'] == 'requires_action'
                    and v['delivery'] == 'acknowledged')(case.inspect(identity)), 120)
            pane = Pane(out, 'approvals', case.socket, 120, 40).start()
            frame = pane.wait('board', lambda f: all(
                'needs approval' in row(f, r) for r in ('run-1', 'run-2')))
            check(all('\u25d0' in row(frame, r) for r in ('run-1', 'run-2')),
                  'a run needing approval has its glyph', frame)
            note = line_with(frame, 'need you')
            check('2 need you' in note, 'the amber note counts both requests', note)
            for run in ('run-1', 'run-2'):
                check(re.search(rf'{run}\s.*\d\d:\d\d left', frame),
                      f'the amber note gives {run} its time left', frame)
            check('\u25d0 2 waiting' in frame, 'the status bar counts the waiting requests')

            first = selected(frame)
            pane.keys('a')
            frame = pane.wait('a-walks', lambda f: selected(f) not in (None, first)
                              and 'WAITING FOR YOU' in f)
            check(previewed(frame) == selected(frame), 'a selects and previews the next waiting run',
                  frame)
            frame = open_run(pane, 'run-1', 'run-view-waiting')
            text = one_line(frame)
            for needle in ('run a command: git tag pio-approval-marker',
                           '\u25c7 not yet classified', 'decider unknown', 'WAITING FOR YOU',
                           'delivery acknowledged', 'runtime requires_action',
                           'exit unavailable', 'liability none', 'tokens unknown'):
                check(needle in text, f'the waiting run view shows {needle!r}', frame)
            check(re.search(r'WAITING FOR YOU \u00b7 \d\d:\d\d left', text),
                  'the request shows its time left', frame)

            # An independent caller answers; the run exits and the audit
            # fills the blanks in place.
            view = case.inspect('run-1')
            action = view['actions'][0]['action_id']
            answered = case.respond(action, 'allow', view['revision'], identity='run-1')
            check('result' in answered, 'the independent answer was taken', answered)
            wait_for('run-1 to exit', lambda: case.inspect('run-1')['runtime'] == 'exited', 60)
            frame = pane.wait('audit-filled', lambda f: 'not classifiable' in f
                              and 'decided by the caller' in f, 20)
            state = one_line(header(frame))
            truth = fold_state(case.inspect('run-1'))
            check(truth == 'finished', 'the fold says run-1 finished', truth)
            check('\u25cb finished' in state and 'uncertain' not in state,
                  'unresolved usage is a marker beside finished, never uncertain', state)
            check('usage unresolved' in frame and 'liability unresolved' in frame,
                  'the usage marker is shown', frame)
            check('not yet classified' not in frame,
                  'every placement is filled once the audit lands', frame)

            pane.keys(']')
            frame = pane.wait('next-run', lambda f: 'run-2' in header(f))
            pane.keys('c')
            pane.wait('confirm', lambda f: 'cancel run-2?' in f)
            pane.keys('n')
            pane.wait('withdrawn', lambda f: 'nothing was sent' in f)
            time.sleep(1)
            kept = case.inspect('run-2')
            check(kept['runtime'] == 'requires_action' and 'cancellation' not in kept,
                  'c then n sends nothing: run-2 still waits', kept.get('cancellation'))
            pane.keys('c')
            pane.wait('confirm-again', lambda f: 'cancel run-2?' in f)
            pane.keys('y')
            wait_for('run-2 to exit after the cancel',
                     lambda: case.inspect('run-2')['runtime'] == 'exited', 60)
            truth = fold_state(case.inspect('run-2'))
            frame = pane.wait('cancelled', lambda f: truth in header(f), 20)
            check('cancellation' in case.inspect('run-2'), 'the cancel reached the service')
            pane.keys('Escape')
            frame = pane.wait('board-after', lambda f: 'RUNS' in f and truth in row(f, 'run-2'))
            check('usage unresolved' in row(frame, 'run-1') and 'finished' in row(frame, 'run-1'),
                  'unresolved usage is a marker beside finished on the board too',
                  row(frame, 'run-1'))
            check('0 uncertain' in frame, 'no run is in doubt', line_with(frame, 'uncertain'))
            pane.keys('q')
            result = pane.exited('detached')
            restored(result, 'after q')
            case.finish()
            return dict(run_1='finished + usage unresolved', run_2=truth, frames=pane.steps)
        finally:
            if pane:
                pane.close()


def scenario_uncertain(out):
    service = FakeService(out, 'uncertain', duration_ms=600, fault='after_release')
    pane = None
    try:
        service.submit('run-u')
        wait_for('run-u to lose its host', lambda: service.inspect('run-u')['runtime']
                 == 'unknown', 30)
        check(fold_state(service.inspect('run-u')) == 'uncertain', 'the fold says uncertain')
        pane = Pane(out, 'uncertain', service.socket, 80, 24).start()
        frame = pane.wait('board', lambda f: 'uncertain' in row(f, 'run-u'))
        check('\u25c7' in row(frame, 'run-u'), 'an uncertain run has its glyph', row(frame, 'run-u'))
        check('1 uncertain' in frame and 'host lost' in line_with(frame, 'run-u  host'),
              'the violet note says why', frame)
        narrow_readable(frame, {'run-u': 'uncertain'}, 'uncertain board')
        pane.keys('u')
        frame = pane.wait('u-walks', lambda f: selected(f) == 'run-u' and previewed(f) == 'run-u')
        pane.keys('Enter')
        frame = pane.wait('run-view', lambda f: 'run-u' in header(f))
        text = one_line(frame)
        for needle in ('\u25c7 uncertain', 'runtime unknown', 'exit unavailable',
                       'result absent', 'tokens unknown'):
            check(needle in text, f'the uncertain run view shows {needle!r}', frame)
        pane.keys('q')
        result = pane.exited('detached')
        restored(result, 'after q')
        return dict(frames=pane.steps)
    finally:
        if pane:
            pane.close()
        service.close()


def scenario_signals(out):
    service = FakeService(out, 'signals', duration_ms=600)
    results = {}
    try:
        service.submit('run-s')
        wait_for('run-s to finish', lambda: service.inspect('run-s')['runtime'] == 'exited')
        for name, how in (('sigterm', signal.SIGTERM), ('sigint', signal.SIGINT),
                          ('panic', None)):
            env = {'PIO_TUI_TEST_PANIC': '1'} if how is None else {}
            pane = Pane(out, f'signal-{name}', service.socket, 80, 24, env=env).start()
            try:
                pane.wait('board', lambda f: 'finished' in row(f, 'run-s'))
                if how is None:
                    pane.keys('!')
                else:
                    os.kill(pane.tui_pid(), how)
                result = pane.exited('after')
                restored(result, name)
                if how is None:
                    check(result['rc'] == 101 and 'the test panic' in result['frame'],
                          'a panic exits 101 with its message on a restored terminal',
                          result['frame'])
                else:
                    check(result['rc'] == 0, f'{name}: detached with exit 0', result['rc'])
                results[name] = dict(rc=result['rc'], stty_same=result['same'])
            finally:
                pane.close()
        return results
    finally:
        service.close()


SCENARIOS = dict(board_live=scenario_board_live, approvals=scenario_approvals,
                 uncertain=scenario_uncertain, signals=scenario_signals)

TUI = 'crates/pio-tui/src/'
MUTANTS = {
    # The T1 first cut's rule, in the screen: a row with a marker reads
    # uncertain.
    'state-word': ('approvals', 'unresolved usage is a marker', [(
        TUI + 'model.rs', '    row["state"].as_str().unwrap_or("unknown")\n',
        '    if row["markers"].as_array().is_some_and(|m| !m.is_empty()) {\n'
        '        return "uncertain";\n    }\n'
        '    row["state"].as_str().unwrap_or("unknown")\n')]),
    'missed-restore': ('board_live', 'stty -g is the same before and after', [(
        TUI + 'terminal.rs', '        let _ = disable_raw_mode();\n', '')]),
    'ignores-new-events': ('board_live', 'new-run-running', [(
        TUI + 'feed.rs', '        changed |= self.read_events()?;\n',
        '        changed |= self.board.cursor.is_none() && self.read_events()?;\n')]),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path)
    parser.add_argument('--scenario', choices=sorted(SCENARIOS), action='append')
    parser.add_argument('--mutant', choices=sorted(MUTANTS))
    parser.add_argument('--stamp', nargs=2, metavar=('RAW', 'READY'), help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.stamp:
        stamp(*args.stamp)
        return
    if not args.out:
        parser.error('--out is required')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    global TUI_BINARY
    if args.mutant:
        name, needle, edits = MUTANTS[args.mutant]
        with source_mutant.mutated(edits) as tree:
            source_mutant.cargo(tree, 'build', '--quiet', '--locked', '-p', 'pio-cli')
        TUI_BINARY = source_mutant.TARGET / 'debug/pio'
        names = [name]
    else:
        names = args.scenario or list(SCENARIOS)
    results, failed = {}, []
    for name in names:
        started = time.monotonic()
        (out / name).mkdir(parents=True, exist_ok=True)
        try:
            results[name] = dict(outcome='pass', **SCENARIOS[name](out))
        except Failed as failure:
            results[name] = dict(outcome='fail', reason=str(failure))
            failed.append(name)
        results[name]['seconds'] = round(time.monotonic() - started, 1)
        reason = results[name].get('reason', '').splitlines()[:1]
        print(f"{results[name]['outcome']}  {name}" + (f'  ({reason[0]})' if reason else ''))
    (out / 'tui-acceptance.json').write_text(json.dumps(
        dict(format='pio-tui-acceptance/1', mutant=args.mutant, scenarios=results),
        indent=2) + '\n')
    if args.mutant:
        needle = MUTANTS[args.mutant][1]
        if not failed:
            raise SystemExit(f'mutant {args.mutant} SURVIVED')
        reason = results[failed[0]]['reason']
        if needle not in reason:
            raise SystemExit(f'mutant {args.mutant}: failed, but not for the named reason '
                             f'({needle!r}):\n{reason[:2000]}')
        print(f'mutant {args.mutant} killed: {reason.splitlines()[0][:240]}')
        return
    if failed:
        raise SystemExit(f'{len(failed)} scenario(s) failed: {failed}')
    print(f'tui acceptance: {len(results)} scenarios pass')


if __name__ == '__main__':
    main()
