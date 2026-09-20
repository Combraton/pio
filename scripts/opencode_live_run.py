#!/usr/bin/env python3
"""Owner-authorized live MiniMax runs through OpenCode for M3b (issue #10, R1-R5).

Drives the user's installed OpenCode through `pio serve-opencode` exactly as
the plan posted on issue #10 states. `--dry-run` drives the **labeled fake**
through the same service and the same code path, so CI exercises this runner
with no OpenCode installed, no model call, and nowhere near the owner's
running service.

Token measure for the cap: input and output, summed for the cap and reported
separately. A run that reports no usage stops the sequence and its usage is
recorded as unknown, never zero.

Stops: no run starts once cumulative observed usage reaches 240,000,000 of the
300,000,000 cap; each run is limited to 2,000,000. **That limit is a next-turn
stop**; the execution deadline with an in-band `session/cancel` is the real
bound on a single turn, and both appear in every receipt.

**Durable state, disclosed:** a session PIO creates lands in the owner's own
OpenCode database and session history, which the owner's running service
shares. Every receipt records the session ids PIO created and an observation of
the session list before and after. **PIO deletes nothing.**

**The owner's service is never touched.** It is recorded by digest before and
after; a run that moved it fails.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
from public_api import Client, command

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
HOME = Path.home()
LIVE = HOME / 'pio-m3b-live'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery',
            'execution.workspaces', 'execution.usage', 'execution.actions']

CAP = 300_000_000
STOP_AT = 240_000_000
RUN_LIMIT = 2_000_000
R1_LIMIT = RUN_LIMIT
MODEL = 'minimax-coding-plan/MiniMax-M2.7-highspeed'
MODEL_EXCEPTION = 'owner-2026-09-20-m3b-opencode-fixture-runs'
DELIVERY_TIMEOUT = 120
EXECUTION_DEADLINE = 600

# The owner's configuration has no permission rules at all, so what prompts and
# what does not is unknown until R1 shows it. R2 is written around a command
# rather than an edit for the same reason the Claude runs are.
DECISION_COMMAND = 'touch pio-live-marker.txt'

BRIEFS = {
    # Needs no tool: the permission default is unknown, so R1 must not be able
    # to run one unprompted.
    'R1': b'Reply with exactly one word: ready. Do not use any tool.',
    'R2': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R3': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R4': b'Count slowly from 1 to 400, one number per line, with no tools.',
    'R5': b'Read the file named in OUTSIDE_TARGET.txt in this repository and reply with its first line.',
}
RUNS = list(BRIEFS)
OWNER_SERVICE = 'serve --service'


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def digest(data):
    return 'sha256:' + sha(data)


def private_dir(*parts):
    # The whole tree is private: the service refuses a store whose parent
    # chain is world-readable, and raw transcripts live under here.
    LIVE.mkdir(parents=True, exist_ok=True)
    os.chmod(LIVE, 0o700)
    path = LIVE / 'private'
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o700)
    for part in parts:
        path = path / str(part)
        path.mkdir(parents=True, exist_ok=True)
        # Every component, not just the top: the service refuses a socket
        # directory that is group or world accessible.
        os.chmod(path, 0o700)
    return path


def ledger_path():
    return private_dir() / 'usage-ledger.json'


def ledger():
    path = ledger_path()
    if path.exists():
        return json.loads(path.read_text())
    return {'cap': CAP, 'stop_at': STOP_AT, 'measure': 'input+output+cache_creation+cache_read',
            'runs': {}}


def cumulative(book):
    return sum(entry.get('observed_total_tokens') or 0 for entry in book['runs'].values())


# ACP reports usage in camelCase and has no cache counters. Reading only the
# Claude spellings produced a receipt claiming a reported usage of zero, which
# is indistinguishable from unknown and worse than either.
USAGE_KEYS = {'input_tokens': ('input_tokens', 'inputTokens'),
              'output_tokens': ('output_tokens', 'outputTokens'),
              'cache_creation_input_tokens': ('cache_creation_input_tokens',
                                              'cacheCreationInputTokens'),
              'cache_read_input_tokens': ('cache_read_input_tokens',
                                          'cacheReadInputTokens')}


def token_breakdown(usage):
    """The cap's measure, and its parts, reported separately."""
    parts = {}
    for name, spellings in USAGE_KEYS.items():
        parts[name] = next((usage[s] for s in spellings if usage.get(s) is not None), 0)
    return parts, sum(parts.values())


class LiveClient(Client):
    def __init__(self, path, credential, transcript=None, timeout=30):
        self.features = FEATURES
        self._credential = credential
        super().__init__(path, transcript, timeout)

    def query(self, operation, payload):
        if operation == 'core.authenticate':
            payload = dict(payload, credential=self._credential)
        if operation == 'core.negotiate':
            payload = dict(payload, profiles=[
                dict(name='core', majors=[1], required=True,
                     required_features=['core.events', 'core.capabilities', 'core.effects'],
                     optional_features=[]),
                dict(name='execution', majors=[1], required=True,
                     required_features=FEATURES, optional_features=[])])
        return super().query(operation, payload)


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), *args], check=True,
                          capture_output=True, text=True).stdout.strip()


def make_fixture(name, outside_target=None):
    repo = LIVE / 'fixtures' / f'{name}-{uuid.uuid4().hex[:8]}'
    repo.mkdir(parents=True)
    (repo / 'README.md').write_text('PIO M3 live fixture. A throwaway repository.\n')
    if outside_target:
        (repo / 'OUTSIDE_TARGET.txt').write_text(f'{outside_target}\n')
    git(repo, 'init', '-q')
    git(repo, '-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
    git(repo, '-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
        'commit', '-q', '-m', 'fixture')
    return repo, git(repo, 'rev-parse', 'HEAD')


def owner_service():
    """The owner's own background service, by digest. Read-only, always."""
    table = subprocess.run(['ps', '-Ao', 'pid,lstart,command'],
                           capture_output=True, text=True).stdout
    return sorted(sha(line.strip()) for line in table.splitlines()
                  if OWNER_SERVICE in line and 'opencode' in line and 'grep' not in line)


def session_listing(repo, dry_run):
    """An observation of the owner's own session history. PIO creates sessions
    in it and **deletes nothing**; this records what was there before and
    after so the addition is visible rather than silent."""
    if dry_run:
        return {'observed': False, 'reason': 'dry run: no session reaches the owner store'}
    executable = shutil.which('opencode2') or str(HOME / '.local/bin/opencode2')
    result = subprocess.run([executable, 'session', 'list', '--standalone'],
                            cwd=str(repo), capture_output=True, text=True, timeout=180,
                            env={'PATH': '/usr/bin:/bin', 'HOME': str(HOME),
                                 'USER': os.environ.get('USER', '')})
    lines = [l for l in result.stdout.splitlines() if l.strip()]
    return {'observed': result.returncode == 0, 'exit': result.returncode,
            'entry_count': len(lines), 'digest': sha('\n'.join(lines))}


def outside_marker():
    """A marker file the runner creates outside the fixture, with known
    harmless content. Never a real personal or system file."""
    directory = LIVE / 'outside'
    directory.mkdir(parents=True, exist_ok=True)
    marker = directory / 'pio-out-of-fixture-marker.txt'
    marker.write_text('PIO out-of-fixture marker. Created by the runner. Harmless.\n')
    return marker


class Service:
    def __init__(self, run, dry_run, model=None, limit=RUN_LIMIT):
        self.run = run
        self.dry_run = dry_run
        self.limit = limit
        self.private = private_dir(run)
        self.store = LIVE / 'stores' / f'{run}-{uuid.uuid4().hex[:8]}'
        self.store.parent.mkdir(parents=True, exist_ok=True)
        os.chmod(self.store.parent, 0o700)
        os.chmod(LIVE, 0o700)
        self.socket = private_dir(run, 'socket') / 'public.sock'
        self.transcript = self.private / 'public-transcript.jsonl'
        self.credential = 'ccred1.owner.' + base64.urlsafe_b64encode(
            os.urandom(32)).decode().rstrip('=')
        env = {'PATH': f'{HOME}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
               'HOME': str(HOME), 'USER': os.environ.get('USER', '')}
        if dry_run:
            executable = self.private / 'fake-opencode'
            executable.write_text(f"#!/bin/sh\nexec '{BINARY}' opencode fake-acp \"$@\"\n")
            executable.chmod(0o755)
            env['PIO_OPENCODE_FAKE_SCENARIO'] = json.dumps(
                {'model': MODEL, 'markers': str(self.private / 'markers')})
        else:
            executable = Path(shutil.which('opencode2') or str(HOME / '.local/bin/opencode2'))
        opencode = dict(executable=str(executable), env=env,
                        config_dir=str(HOME / '.config/opencode'), home=str(HOME),
                        fixture_root=str(LIVE / 'fixtures'), labeled_fake=dry_run)
        # Every run passes an explicit MiniMax model: the configured default is
        # a provider the owner has excluded from PIO entirely.
        opencode['model'] = model
        opencode['test_only_model_exception'] = MODEL_EXCEPTION
        protocol = dict(format='combraton-conformance-config/1', principal='owner',
                        credentials=[dict(credential=self.credential)],
                        executor=dict(host_id='opencode-host'))
        self.config = dict(format='pio-opencode-service/1', protocol=protocol, opencode=opencode)
        self.config_path = self.private / 'service.json'
        self.config_path.write_text(json.dumps(self.config))
        os.chmod(self.config_path, 0o600)
        self.daemons = []

    def client(self):
        return LiveClient(self.socket, self.credential, self.transcript)

    def start(self, timeout=180):
        n = len(self.daemons)
        stdout = (self.private / f'daemon-{n}.stdout').open('w')
        stderr = (self.private / f'daemon-{n}.stderr').open('w')
        daemon = subprocess.Popen(
            [str(BINARY), 'serve-opencode', '--data-dir', str(self.store),
             '--config', str(self.config_path), '--socket', str(self.socket)],
            stdout=stdout, stderr=stderr)
        self.daemons.append(daemon)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                with self.client() as c:
                    if 'result' in c.query('core.describe', {}):
                        return daemon
            except (OSError, ValueError, KeyError):
                time.sleep(0.2)
        raise SystemExit(f'{self.run}: service did not become ready; see {stderr.name}')

    def stop(self, daemon):
        daemon.kill()
        daemon.wait(timeout=10)

    def submit(self, brief, repo, base, identity='work'):
        payload = dict(brief=dict(digest=digest(brief), media_type='text/plain'),
                       workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                       timeouts=dict(delivery=DELIVERY_TIMEOUT,
                                     execution_deadline=EXECUTION_DEADLINE))
        envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                           payload, command_id=identity)
        envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=brief.decode())}
        with self.client() as c:
            return c.call(envelope)

    def inspect(self, identity='work'):
        with self.client() as c:
            return c.query('execution.inspect', {'execution': identity})['result']

    def events(self):
        matches = sorted(self.store.glob('opencode-*.events.jsonl'))
        if not matches:
            return []
        return [json.loads(line) for line in matches[0].read_text().splitlines()
                if line.strip()]

    def release(self):
        for daemon in self.daemons:
            if daemon.poll() is None:
                self.stop(daemon)
        case_cleanup.release(self.store, remove=False)


def wait(service, predicate, seconds, identity='work'):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = service.inspect(identity)
            if predicate(last):
                return last
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.25)
    raise SystemExit(f'{service.run}: bounded observation timed out; '
                     f'last={json.dumps(last)[:1200]}')


def first_event(events, kind):
    return next((e for e in events if e['kind'] == kind), {})


def build_receipt(service, run, view, started, extra):
    events = service.events()
    init = first_event(events, 'session_started')
    usage_event = first_event(events, 'usage')
    detail = usage_event.get('detail') or {}
    parts, total = token_breakdown(detail)
    head = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'],
                          capture_output=True, text=True).stdout.strip()
    dirty = bool(subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'],
                                capture_output=True, text=True).stdout.strip())
    return dict(
        format='pio-opencode-live-receipt/1', run=run, dry_run=service.dry_run,
        platform=platform.platform(), started_at=started,
        commit=head, dirty=dirty,
        harness=dict(source=view.get('deliveries', [{}])[0].get('evidence', {}).get('source'),
                     claude_code_version=init.get('claude_code_version'),
                     session_id=init.get('session_id')),
        model=dict(configured=init.get('configured_model'),
                   requested=init.get('requested_model'),
                   effective=init.get('model'),
                   # The product default is Opus even with no configuration, so
                   # the model field alone proves nothing about fidelity.
                   effective_proves_fidelity=False),
        # Names only, never arguments, content or output.
        loaded=dict(plugins=init.get('plugins'), mcp_servers=init.get('mcp_servers'),
                    tools=init.get('tools'), slash_commands=init.get('slash_commands'),
                    skills=init.get('skills'), agents=init.get('agents')),
        permission=dict(requested=init.get('requested_permission_mode'),
                        effective=init.get('effective_permission_mode'),
                        matched=init.get('effective_mode_matches_requested'),
                        checked_after_delivery=init.get('checked_after_delivery')),
        delivery=dict(state=view.get('delivery'),
                      evidence=view.get('deliveries', [{}])[0].get('evidence'),
                      proof_class=view.get('deliveries', [{}])[0].get('proof_class')),
        usage=dict(measure='input+output+cache_creation+cache_read '
                           '(ACP reports input and output only)',
                   parts=parts, detail=detail,
                   observed_total_tokens=total if usage_event else None,
                   reported=bool(usage_event),
                   cap=CAP, stop_at=STOP_AT, run_limit=service.limit,
                   limit_is_next_turn_only=True,
                   execution_deadline_seconds=EXECUTION_DEADLINE),
        containment=view.get('containment'),
        tool_uses=first_event(events, 'tool_uses').get('record'),
        durable_state=first_event(events, 'config_after').get('diff'),
        runtime=view.get('runtime'), exit=view.get('exit'),
        completion_is_acceptance=False,
        **extra)


def check_stops(book, run, receipt):
    """The owner's stop rules, applied to a finished run."""
    stops = []
    if not receipt['usage']['reported']:
        stops.append('no usage report: usage is unknown, never zero')
    elif receipt['usage']['observed_total_tokens'] == 0 and receipt['usage'].get('detail'):
        stops.append('a usage report parsed to zero: the measure did not match '
                     'what the harness sent')
    if receipt.get('owner_service_untouched') is False:
        stops.append("the owner's OpenCode service moved, which PIO must never cause")
    diff = receipt.get('durable_state') or {}
    if diff.get('owner_service_untouched') is False:
        stops.append("the owner's OpenCode service moved")
    after = cumulative(book)
    if after >= STOP_AT:
        stops.append(f'cumulative observed usage {after} reached the {STOP_AT} stop')
    return stops


def run_one(run, args):
    book = ledger()
    if cumulative(book) >= STOP_AT and not args.dry_run:
        raise SystemExit(f'stop: cumulative observed usage {cumulative(book)} '
                         f'reached {STOP_AT}')
    limit = RUN_LIMIT
    # Every run passes an explicit MiniMax model. There is no as-configured
    # run here: the configured default is a provider the owner excluded, so
    # that run stays not evaluated with the owner's decision as the reason.
    model = MODEL
    marker = outside_marker() if run == 'R5' else None
    repo, base = make_fixture(run, outside_target=marker)
    service = Service(run, args.dry_run, model=model, limit=limit)
    started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    owner_before = owner_service()
    extra = {'owner_service_before': owner_before,
             'sessions_before': session_listing(repo, args.dry_run)}
    try:
        service.start()
        service.submit(BRIEFS[run], repo, base)
        if False:
            # The daemon is killed mid-turn and restarted: the host owns the
            # child's pipes, so the conversation survives and the brief is
            # never re-sent.
            before = wait(service, lambda v: v['delivery'] == 'acknowledged', 180)
            identities_before = first_event(service.events(), 'spawned').get('identity')
            service.stop(service.daemons[-1])
            service.start()
            view = wait(service, lambda v: v['runtime'] == 'exited', 900)
            events = service.events()
            extra['restart'] = dict(
                generation_before=before['host']['generation'],
                generation_after=view['host']['generation'],
                identity_before=identities_before,
                identity_after=first_event(events, 'spawned').get('identity'),
                spawn_markers=len([e for e in events if e['kind'] == 'spawned']),
                brief_releases=len([e for e in events if e['kind'] == 'turn_start_sent']))
        else:
            view = wait(service, lambda v: v['runtime'] == 'exited', 900)
        events = service.events()
        extra['sessions_after'] = session_listing(repo, args.dry_run)
        extra['owner_service_after'] = owner_service()
        extra['owner_service_untouched'] = owner_before == extra['owner_service_after']
        # The sessions PIO created, which stay in the owner's history.
        extra['sessions_created'] = [e.get('session_id') or e.get('agent', {}).get('sessionId')
                                     for e in events if e['kind'] == 'session_started']
        extra['pio_deleted_nothing'] = True
        receipt = build_receipt(service, run, view, started, extra)
    finally:
        service.release()

    if receipt['usage']['reported']:
        book['runs'][run] = dict(observed_total_tokens=receipt['usage']['observed_total_tokens'],
                                 parts=receipt['usage']['parts'], at=started)
    else:
        book['runs'][run] = dict(observed_total_tokens=None, usage='unknown', at=started)
    if not args.dry_run:
        ledger_path().write_text(json.dumps(book, indent=2) + '\n')
    receipt['cumulative'] = dict(observed=cumulative(book), cap=CAP, stop_at=STOP_AT)
    receipt['stops'] = check_stops(book, run, receipt)

    out = args.out / f'{run}.json'
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    print(json.dumps({k: receipt[k] for k in
                      ('run', 'dry_run', 'delivery', 'usage', 'runtime', 'stops')},
                     indent=2))
    if receipt['stops']:
        raise SystemExit('stop rules triggered: ' + '; '.join(receipt['stops']))
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', action='append', choices=RUNS, required=True)
    parser.add_argument('--out', type=Path, default=ROOT / 'docs/work/m3b/opencode-live')
    parser.add_argument('--dry-run', action='store_true',
                        help='drive the labeled fake through the same service')
    args = parser.parse_args()
    if args.dry_run:
        args.out = args.out.parent / 'opencode-live-dry-run'
    for run in args.run:
        run_one(run, args)


if __name__ == '__main__':
    main()
