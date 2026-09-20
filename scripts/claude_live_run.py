#!/usr/bin/env python3
"""Owner-authorized live Claude Code runs for M3 (issue #7 plan R1-R7).

Drives the user's installed Claude Code through `pio serve-claude` exactly as
the plan posted on issue #7 states. `--dry-run` drives the **labeled fake**
through the same service and the same code path, so CI exercises this runner
with no Claude Code installed and no model call.

Token measure for the cap (owner decision, 2026-09-20): input, output, cache
creation and cache read, **summed** for the cap and **reported separately** in
every receipt. A run that reports no usage stops the sequence and its usage is
recorded as unknown, never zero.

Stops: no run starts once cumulative observed usage reaches 800,000 of the
1,000,000 cap; R1 is limited to 150,000 with no retry; R2-R7 to 250,000 each.
Until R1 measures usage granularity these are **next-turn** stops — the
execution deadline with a cancel is the only bound on a single turn.
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
LIVE = HOME / 'pio-m3-live'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery',
            'execution.workspaces', 'execution.usage', 'execution.actions']

CAP = 1_000_000
STOP_AT = 800_000
RUN_LIMIT = 250_000
R1_LIMIT = 150_000
MODEL = 'claude-sonnet-5'
MODEL_EXCEPTION = 'owner-2026-09-20-m3-fixture-runs'
PERMISSION_MODE = 'acceptEdits'
DELIVERY_TIMEOUT = 120
EXECUTION_DEADLINE = 600

# Verified offline against the owner's 38 `Bash(...)` allow rules across both
# settings files: no rule matches. It mutates, so `acceptEdits` — which
# auto-accepts file edits, not Bash — should not auto-approve it. Whether it
# falls outside Claude Code's unpublished read-only set is what R3 measures.
DECISION_COMMAND = 'touch pio-live-marker.txt'

BRIEFS = {
    # Needs no tool and no plugin: a one-word reply.
    'R1': b'Reply with exactly one word: ready. Do not use any tool.',
    'R2': b'Read README.md in this repository and reply with its first line. Nothing else.',
    'R3': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R4': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R5': b'Count slowly from 1 to 400, one number per line, with no tools.',
    'R6': b'Read the file named in OUTSIDE_TARGET.txt in this repository and reply with its first line.',
    'R7': b'Count slowly from 1 to 200, one number per line, with no tools.',
}
RUNS = list(BRIEFS)

# R8, the optional steering run, is **not** here. The Claude host implements
# `respond_action` and `interrupt` and no steer control, so a second mid-turn
# message cannot be sent through the service. Listing R8 and running it anyway
# would produce a receipt for a plain turn labelled as a steering observation.
# It is deferred until the control exists, and ADR 004 §10 already records that
# steering behaviour is not observed for this harness.
DEFERRED = {'R8': 'no steer control in the Claude host; see ADR 004 section 10'}


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def digest(data):
    return 'sha256:' + sha(data)


RUN_ID = uuid.uuid4().hex[:8]
# Set by main(): a dry run never writes into the live tree, and a live run
# never reuses or appends to a previous run's private directory.
ROOT_OVERRIDE = None


def live_root():
    return Path(ROOT_OVERRIDE) if ROOT_OVERRIDE else LIVE


def private_dir(*parts):
    # The whole tree is private: the service refuses a store whose parent
    # chain is world-readable, and raw transcripts live under here.
    base = live_root()
    base.mkdir(parents=True, exist_ok=True)
    os.chmod(base, 0o700)
    case_cleanup.permit_prefix(base)
    path = base / 'private'
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o700)
    for part in parts:
        path = path / str(part)
        path.mkdir(parents=True, exist_ok=True)
        # Every component, not just the top: the service refuses a socket
        # directory that is group or world accessible.
        os.chmod(path, 0o700)
    return path


def binary_sha256():
    return sha(BINARY.read_bytes())


def preflight(dry_run):
    """Refuse a live run that cannot produce trustworthy evidence.

    M2 produced one receipt from a dirty tree; it was preserved, disclosed and
    re-run. Recording `dirty: true` was not enough — a receipt nobody can
    reproduce is not evidence, so this refuses instead of noting it.
    """
    if dry_run:
        return {'checked': False, 'reason': 'dry run: no live evidence is produced'}
    dirty = subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'],
                           capture_output=True, text=True).stdout.strip()
    if dirty:
        raise SystemExit('refusing to run live from a dirty tree:\n' + dirty)
    head = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'],
                          capture_output=True, text=True).stdout.strip()
    if not BINARY.exists():
        raise SystemExit(f'refusing to run live: no binary at {BINARY}')
    # The binary must be newer than every source it is built from. Comparing it
    # to the *commit* timestamp instead refused a correct binary, because
    # building before committing always loses that comparison — the binary held
    # exactly the committed code and was still called stale.
    sources = [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock']
    sources += [p for p in (ROOT / 'crates').rglob('*')
                if p.is_file() and p.suffix in ('.rs', '.toml')]
    newest = max(sources, key=lambda p: p.stat().st_mtime)
    built = BINARY.stat().st_mtime
    if built < newest.stat().st_mtime:
        raise SystemExit(
            f'refusing to run live: {BINARY.name} is older than '
            f'{newest.relative_to(ROOT)}; rebuild from {head[:12]} first')
    return {'checked': True, 'commit': head, 'dirty': False,
            'binary_sha256': binary_sha256(),
            'binary_newer_than_every_source': True,
            'newest_source': str(newest.relative_to(ROOT))}


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


def token_breakdown(usage):
    """The cap's measure, and its parts. Reported separately in every receipt
    so a reader can see what the single number is made of."""
    parts = {
        'input_tokens': usage.get('input_tokens') or 0,
        'output_tokens': usage.get('output_tokens') or 0,
        'cache_creation_input_tokens': usage.get('cache_creation_input_tokens') or 0,
        'cache_read_input_tokens': usage.get('cache_read_input_tokens') or 0,
    }
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
    repo = live_root() / 'fixtures' / f'{name}-{uuid.uuid4().hex[:8]}'
    repo.mkdir(parents=True)
    (repo / 'README.md').write_text('PIO M3 live fixture. A throwaway repository.\n')
    if outside_target:
        (repo / 'OUTSIDE_TARGET.txt').write_text(f'{outside_target}\n')
    git(repo, 'init', '-q')
    git(repo, '-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
    git(repo, '-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
        'commit', '-q', '-m', 'fixture')
    return repo, git(repo, 'rev-parse', 'HEAD')


def outside_marker():
    """A marker file the runner creates outside the fixture, with known
    harmless content. Never a real personal or system file."""
    directory = live_root() / 'outside'
    directory.mkdir(parents=True, exist_ok=True)
    marker = directory / 'pio-out-of-fixture-marker.txt'
    marker.write_text('PIO out-of-fixture marker. Created by the runner. Harmless.\n')
    return marker


class Service:
    def __init__(self, run, dry_run, model=None, limit=RUN_LIMIT):
        self.run = run
        self.dry_run = dry_run
        self.limit = limit
        self.private = private_dir(run, RUN_ID)
        self.store = live_root() / 'stores' / f'{run}-{RUN_ID}'
        self.store.parent.mkdir(parents=True, exist_ok=True)
        os.chmod(self.store.parent, 0o700)
        os.chmod(live_root(), 0o700)
        self.socket = private_dir(run, RUN_ID, 'socket') / 'public.sock'
        self.transcript = self.private / 'public-transcript.jsonl'
        self.credential = 'ccred1.owner.' + base64.urlsafe_b64encode(
            os.urandom(32)).decode().rstrip('=')
        env = {'PATH': f'{HOME}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
               'HOME': str(HOME), 'USER': os.environ.get('USER', '')}
        if dry_run:
            # A dry run must not read the owner's real configuration, and on a
            # CI runner there is none: the permission-mode guard then refuses
            # an absent default and the service never starts. So the dry run
            # gets its own stand-in settings, which is also the honest thing —
            # it is exercising the runner, not the owner's machine.
            config_dir = private_dir(self.run, RUN_ID, 'claude-config')
            (config_dir / 'settings.json').write_text(json.dumps(
                {'model': 'a-stand-in-model-name',
                 'permissions': {'defaultMode': PERMISSION_MODE, 'allow': ['Bash(cat)']}}))
            executable = self.private / 'fake-claude'
            executable.write_text(f"#!/bin/sh\nexec '{BINARY}' claude fake-cli \"$@\"\n")
            executable.chmod(0o755)
            env['PIO_CLAUDE_FAKE_SCENARIO'] = json.dumps(
                {'markers': str(self.private / 'markers'),
                 # The fake writes a transcript where the real harness does,
                 # under its own stand-in configuration directory.
                 'config_dir': str(config_dir)})
        else:
            config_dir = HOME / '.claude'
            executable = Path(shutil.which('claude') or '/opt/homebrew/bin/claude')
        claude = dict(executable=str(executable), env=env,
                      config_dir=str(config_dir), home=str(HOME),
                      fixture_root=str(live_root() / 'fixtures'),
                      permission_mode=PERMISSION_MODE, labeled_fake=dry_run)
        if model:
            # The service refuses a model unless the configuration names the
            # owner's dated exception, so both appear together or not at all.
            claude['model'] = model
            claude['test_only_model_exception'] = MODEL_EXCEPTION
        protocol = dict(format='combraton-conformance-config/1', principal='owner',
                        credentials=[dict(credential=self.credential)],
                        executor=dict(host_id='claude-host'))
        self.config = dict(format='pio-claude-service/1', protocol=protocol, claude=claude)
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
            [str(BINARY), 'serve-claude', '--data-dir', str(self.store),
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
        matches = sorted(self.store.glob('claude-*.events.jsonl'))
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
        format='pio-claude-live-receipt/1', run=run, dry_run=service.dry_run,
        platform=platform.platform(), started_at=started,
        commit=head, dirty=dirty, binary_sha256=binary_sha256(),
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
        usage=dict(measure='input+output+cache_creation+cache_read',
                   parts=parts, observed_total_tokens=total if usage_event else None,
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
    if receipt['permission']['matched'] is False:
        stops.append('effective permission mode did not match the requested one')
    diff = receipt.get('durable_state') or {}
    if diff.get('settings_changed'):
        stops.append("the owner's settings changed, which PIO must never cause")
    after = cumulative(book)
    if after >= STOP_AT:
        stops.append(f'cumulative observed usage {after} reached the {STOP_AT} stop')
    return stops


def run_one(run, args):
    checks = preflight(args.dry_run)
    book = ledger()
    if cumulative(book) >= STOP_AT and not args.dry_run:
        raise SystemExit(f'stop: cumulative observed usage {cumulative(book)} '
                         f'reached {STOP_AT}')
    limit = R1_LIMIT if run == 'R1' else RUN_LIMIT
    # R1 runs with the user's configuration untouched and no model passed.
    model = None if run == 'R1' else MODEL
    marker = outside_marker() if run == 'R6' else None
    repo, base = make_fixture(run, outside_target=marker)
    service = Service(run, args.dry_run, model=model, limit=limit)
    started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    extra = {}
    try:
        service.start()
        service.submit(BRIEFS[run], repo, base)
        if run == 'R7':
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
        extra['preflight'] = checks
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
    parser.add_argument('--out', type=Path, default=ROOT / 'docs/work/m3/claude-live')
    parser.add_argument('--dry-run', action='store_true',
                        help='drive the labeled fake through the same service')
    args = parser.parse_args()
    global ROOT_OVERRIDE
    # Absolute from here on: a service refuses a configuration whose paths are
    # relative, and every path below is derived from this one.
    args.out = args.out.resolve()
    if args.dry_run:
        # A dry run lives entirely under --out: it must never write into, or
        # append to, the tree a live receipt comes from.
        ROOT_OVERRIDE = args.out / 'tree'
        args.out = args.out.parent / 'claude-live-dry-run'
    for run in args.run:
        run_one(run, args)


if __name__ == '__main__':
    main()
