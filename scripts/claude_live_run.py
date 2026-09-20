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
import tempfile
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
# Measured on the first R5 attempt: the host takes about 8 s to spawn, delivery
# is acknowledged at 10 s and the turn ended at 15 s. A 20 s wait cancelled
# nothing. The signal must land inside generation, so it goes in shortly after
# delivery, and the brief is long enough that the window is not a knife edge.
CANCEL_AFTER = 5
# Owner decision, 2026-09-20. A cancelled turn reports an empty usage block, so
# what it spent is unknown. Unknown is not zero and a cap kept in unknowns is
# not a cap, so a turn cancelled **inside its first model call** is charged a
# flat allowance. The basis is the only two single-call turns measured: R1 at
# 33,793 and R5 attempt 1 at 32,957. Every stop rule is applied to `charged`.
CANCEL_ALLOWANCE = 40_000
CHARGE_BASIS = ('allowance for a turn cancelled inside its first model call; '
                'basis: the single-call turns observed at 33,793 (R1) and '
                '32,957 (R5 attempt 1); owner decision 2026-09-20')
# Runs whose purpose is a cancel. Their usage is unknown by design, so they are
# charged the allowance and do not halt the sequence. Any other run that ends
# without a usage report still does.
PLANNED_CANCEL = {'R5'}
EXECUTION_DEADLINE = 600

# Verified offline against the owner's 38 `Bash(...)` allow rules across both
# settings files: no rule matches. It mutates, so `acceptEdits` — which
# auto-accepts file edits, not Bash — should not auto-approve it. Whether it
# falls outside Claude Code's unpublished read-only set is what R3 measures.
DECISION_COMMAND = 'touch pio-live-marker.txt'
# Second attempt at the decision path (owner decision, 2026-09-20). R3 measured
# that `touch` is auto-approved, so the caller is never asked. The owner's allow
# list carries no `git` rule, and the effect is observable with `git tag -l`.
# If this one does not prompt either, that is the finding: stop there, do not
# run R4b, and do not try a third command.
TAG_COMMAND = 'git tag pio-live-marker'
TAG_NAME = 'pio-live-marker'

BRIEFS = {
    # Needs no tool and no plugin: a one-word reply.
    'R1': b'Reply with exactly one word: ready. Do not use any tool.',
    'R2': b'Read README.md in this repository and reply with its first line. Nothing else.',
    'R3': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R4': f'Run this shell command in this repository and report its exit status: {DECISION_COMMAND}'.encode(),
    'R5': b'Count slowly from 1 to 2000, one number per line, with no tools.',
    'R6': b'Read the file named in OUTSIDE_TARGET.txt in this repository and reply with its first line.',
    'R3b': (f'Run exactly this one command once in this repository and report its '
            f'exit status. Do nothing else: {TAG_COMMAND}').encode(),
    'R4b': (f'Run exactly this one command once in this repository and report its '
            f'exit status. Do nothing else: {TAG_COMMAND}').encode(),
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

# R4 was to be the caller's **allow**. R3 measured that this harness never asks
# for this command under the owner's configuration: it ran `touch` unprompted
# and created the marker. R4 carries the same brief, so it could only reproduce
# that negative at the cost of another turn. Owner decision after R3: skip it,
# and carry the caller's decision path as an unexercised obligation rather than
# swapping in a command chosen because it prompts.
NOT_RUN = {'R4': 'no permission request arrives for this brief; see R3 and '
                 'docs/work/m3/claude-live/R4-not-run.json'}


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
            'charge_policy': charge_policy(), 'runs': {}}


def charge_policy():
    return {'stop_rules_use': 'charged',
            'cancelled_in_first_model_call_allowance': CANCEL_ALLOWANCE,
            'basis': CHARGE_BASIS,
            'note': 'observed is what a harness reported; charged is what the '
                    'cap is measured against. They differ only where a turn '
                    'reported nothing.'}


def cumulative(book):
    """What the cap is measured against: charged, not observed."""
    return sum(entry.get('charged') or 0 for entry in book['runs'].values())


def observed_total(book):
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


def decision_effect(run, repo):
    """What a decision actually did, read from the world rather than inferred
    from what PIO sent. R3 and R4 create a file; R3b and R4b create a git tag,
    because `touch` turned out to be auto-approved and the owner's allow list
    carries no `git` rule."""
    if run in ('R3', 'R4'):
        return dict(kind='marker file', label='<fixture>/pio-live-marker.txt',
                    happened=(repo / 'pio-live-marker.txt').exists())
    tags = subprocess.run(['git', '-C', str(repo), 'tag', '-l'],
                          capture_output=True, text=True).stdout.split()
    return dict(kind='git tag', label=TAG_NAME, happened=TAG_NAME in tags,
                tags_present=tags)


def scratch_root():
    """Scratch for the pre-run dry checks: beside the worktree, never inside
    it and never in the live tree a receipt comes from."""
    return Path(os.environ.get('PIO_SCRATCH') or ROOT.parent / 'pio-scratch')


# What each run is named for, and the predicate that says the runner can
# actually make that observation. Checked in a dry run before the live one:
# three runs in a row were named for an observation the runner could not make.
OBSERVATION = {
    'R3': ('a permission request surfaced and answered by the caller',
           lambda r: r['decision']['requested'] and bool(r['decision']['answered'])),
    'R4': ('a permission request surfaced and answered by the caller',
           lambda r: r['decision']['requested'] and bool(r['decision']['answered'])),
    'R3b': ('a permission request surfaced and answered by the caller',
            lambda r: r['decision']['requested'] and bool(r['decision']['answered'])),
    'R4b': ('a permission request surfaced and answered by the caller',
            lambda r: r['decision']['requested'] and bool(r['decision']['answered'])),
    'R5': ('a signal actually sent to a running harness',
           lambda r: r['cancel']['signal_sent'] and r['cancel']['tested_cancel']),
    'R6': ("PIO's own decline of a target outside the workspace",
           lambda r: r['decline']['declined_by_pio'] > 0),
    'R7': ('a restart that reattaches without re-sending the brief',
           lambda r: r['restart']['spawn_markers'] == 1 and r['restart']['brief_releases'] == 1),
}


def dry_run_check(run):
    """Refuse a live run whose observation the runner cannot make.

    Runs this same script against the labeled fake and applies the run's own
    predicate to the receipt. Costs no tokens and takes seconds; the
    alternative is a live receipt that claims something that never happened.
    """
    label, predicate = OBSERVATION[run]
    root = scratch_root()
    root.mkdir(parents=True, exist_ok=True)
    os.chmod(root, 0o700)
    # Short on purpose: the rehearsal builds a Unix socket path underneath
    # this, and the platform caps that near 104 bytes.
    out = root / 'd'
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    try:
        here = str(Path(__file__).resolve().parent)
        # This script imports its siblings, so the child needs them on the path
        # too. Without it the rehearsal fails on an import and the refusal
        # below blames the run rather than the runner.
        env = dict(os.environ, PYTHONPATH=os.pathsep.join(
            [here, os.environ.get('PYTHONPATH', '')]).rstrip(os.pathsep))
        finished = subprocess.run(
            [sys.executable, str(Path(__file__).resolve()),
             '--run', run, '--dry-run', '--out', str(out / 'o')],
            capture_output=True, text=True, timeout=900, env=env)
        receipt = out / 'claude-live-dry-run' / f'{run}.json'
        if not receipt.exists():
            raise SystemExit(
                f'{run}: the dry run produced no receipt, so the run cannot be '
                f'rehearsed and no tokens are spent on it.\n'
                f'--- rehearsal stderr ---\n{finished.stderr[-1500:]}')
        record = json.loads(receipt.read_text())
        try:
            made = bool(predicate(record))
        except (KeyError, TypeError):
            made = False
        if not made:
            raise SystemExit(
                f'{run}: the dry run did not make the observation this run is '
                f'named for ({label}). Refusing to spend tokens on a receipt '
                f'that would claim it.')
        return dict(ran=True, observation=label, made_in_dry_run=True)
    finally:
        shutil.rmtree(out, ignore_errors=True)


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
        # A Unix socket path is capped near 104 bytes. Past it `bind` fails and
        # the only symptom is a service that never becomes ready, which says
        # nothing about the cause. Measured: a rehearsal under a deep scratch
        # directory produced exactly that.
        if len(str(self.socket).encode()) > 100:
            raise SystemExit(
                f'{run}: the socket path is {len(str(self.socket).encode())} bytes, '
                f'over the ~104 the platform allows, so the service could never '
                f'bind it. Use a shorter --out.\n  {self.socket}')
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
            scenario = {'markers': str(self.private / 'markers'),
                        # The fake writes a transcript where the real harness
                        # does, under its own stand-in configuration directory.
                        'config_dir': str(config_dir)}
            if run in ('R3', 'R4'):
                # So a dry run exercises the decision path end to end rather
                # than only the runner's plumbing around it.
                scenario['permission_request'] = {
                    'tool_name': 'Bash', 'input': {'command': DECISION_COMMAND}}
            if run in ('R3b', 'R4b'):
                scenario['permission_request'] = {
                    'tool_name': 'Bash', 'input': {'command': TAG_COMMAND}}
            if run == 'R6':
                # A request for a target outside the workspace, so the dry run
                # exercises PIO's own decline rather than the runner's
                # plumbing around it.
                scenario['permission_request'] = {
                    'tool_name': 'Read', 'input': {'file_path': str(outside_marker())}}
            if run == 'R5':
                # The fake must still be running when the signal arrives, or
                # the dry run would rehearse cancelling nothing, and it must
                # end the turn the way the real harness does: an aborted
                # `result` with an empty usage block, which is what made PIO
                # report zero.
                scenario['delay_ms'] = (CANCEL_AFTER + 10) * 1000
                scenario['abort_on_interrupt'] = True
            env['PIO_CLAUDE_FAKE_SCENARIO'] = json.dumps(scenario)
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

    def cancel(self, revision, identity='work'):
        envelope = command('execution.cancel',
                           dict(kind='execution.execution', id=identity), {},
                           command_id=f'{identity}.cancel', revision=revision)
        with self.client() as c:
            return c.call(envelope)

    def respond(self, action_id, decision, revision, identity='work'):
        """Answer one surfaced permission request.

        The decision is the caller's and PIO forwards it unchanged. Only the
        single-use vocabulary of this harness is encodable, so a widening
        decision cannot be expressed here even by mistake.
        """
        body = json.dumps({'decision': decision}).encode()
        envelope = command('execution.respond_action',
                           dict(kind='execution.execution', id=identity),
                           dict(action_id=action_id,
                                response=dict(digest=digest(body),
                                              media_type='application/json')),
                           command_id=f'{identity}.answer-{action_id}', revision=revision)
        envelope['extensions'] = {CONTENT: dict(media_type='application/json',
                                                text=body.decode())}
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
                   # Why there is no report, when there is none. A cancelled
                   # turn sends a `result` whose usage block is empty, and the
                   # host refuses to pass that on as an observation of zero.
                   unknown_reason=first_event(events, 'usage_unknown').get('reason'),
                   terminal_reason=(first_event(events, 'usage_unknown').get('terminal_reason')
                                    or first_event(events, 'turn_completed').get('terminal_reason')),
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
    if not receipt['usage']['reported'] and run not in PLANNED_CANCEL:
        stops.append('no usage report: usage is unknown, never zero')
    elif not receipt['usage']['reported']:
        pass  # A planned cancel. Charged at the allowance; see charge_policy.
    # A report of zero for a turn that ran is the same failure wearing a
    # number. R5 produced one: a cancelled turn whose usage block was empty.
    elif not receipt['usage']['observed_total_tokens']:
        stops.append('usage reported as zero for a turn that ran: unknown, never zero')
    # The per-run limit was recorded in every receipt and enforced nowhere.
    elif receipt['usage']['observed_total_tokens'] > receipt['usage']['run_limit']:
        stops.append(f"run used {receipt['usage']['observed_total_tokens']} tokens, "
                     f"over its own {receipt['usage']['run_limit']} limit")
    if receipt['permission']['matched'] is False:
        stops.append('effective permission mode did not match the requested one')
    diff = receipt.get('durable_state') or {}
    if diff.get('settings_changed'):
        stops.append("the owner's settings changed, which PIO must never cause")
    after = cumulative(book)
    if after >= STOP_AT:
        stops.append(f'cumulative charged usage {after} reached the {STOP_AT} stop')
    return stops


def run_one(run, args):
    checks = preflight(args.dry_run)
    if not args.dry_run and run in OBSERVATION:
        checks['dry_run_check'] = dry_run_check(run)
    book = ledger()
    if run in book['runs'] and not args.dry_run:
        raise SystemExit(
            f'{run} already has a ledger entry of '
            f'{book["runs"][run].get("observed_total_tokens")} tokens. Those '
            f'were spent and must stay counted: rename the entry and its '
            f'receipt to {run}-attempt-N before running {run} again.')
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
        elif run == 'R5':
            # Cancel is SIGINT and is described as exactly that; the in-band
            # `interrupt_receipt_v1` the capabilities advertise is unverified
            # against 2.1.278. What this run is for is what a cancel *costs*:
            # `result` is the only place usage is reported, so a signal that
            # ends the turn first leaves usage unknown. Recorded either way,
            # and never as zero.
            wait(service, lambda v: v['delivery'] == 'acknowledged', 180)
            time.sleep(CANCEL_AFTER)
            at_cancel = service.inspect()
            asked = time.monotonic()
            response = service.cancel(at_cancel['revision'])
            view = wait(service, lambda v: v['runtime'] == 'exited', 900)
            elapsed = time.monotonic() - asked
            events = service.events()
            usage_event = first_event(events, 'usage')
            sent = [e for e in events if e['kind'] == 'control_sent']
            extra['cancel'] = dict(
                requested_after_seconds=CANCEL_AFTER,
                accepted=response.get('result') is not None,
                # Accepting the command is not sending the signal. The first
                # attempt was accepted after the turn had already finished, so
                # nothing was signalled; a receipt that reported only
                # `accepted` would have read as a cancel that never happened.
                signal_sent=bool(sent),
                tested_cancel=bool(sent),
                error=response.get('error', {}).get('data'),
                # The signal, as the host describes it to itself.
                control_sent=[{k: e.get(k) for k in
                               ('method', 'in_band', 'signal_delivered',
                                'escalates_after_ms', 'usage_may_be_unknown')}
                              for e in sent],
                seconds_to_exit=round(elapsed, 3),
                # The question the plan asked: does a `result` arrive after the
                # signal, and is usage therefore knowable on cancel?
                turn_completed=bool(first_event(events, 'turn_completed')),
                # `turn_completed` is the event's name, not a verdict: after a
                # signal it carries `status: failed` and
                # `terminal_reason: aborted_streaming`.
                turn_status=first_event(events, 'turn_completed').get('status'),
                terminal_reason=first_event(events, 'turn_completed').get('terminal_reason'),
                usage_reported=bool(usage_event),
                usage_basis='observed' if usage_event else 'unknown',
                note=None if sent else
                     'the turn finished before the cancel was sent: this run '
                     'observed a completed turn, not a cancel')

        elif run in ('R3', 'R4', 'R3b', 'R4b'):
            # The decision is the caller's, and it is the whole point of these
            # two runs: R3 denies, R4 allows. If the harness never asks, that
            # is a negative result for the decision path and is recorded as
            # one — never re-run with a different command until something
            # prompts.
            decision = 'deny' if run in ('R3', 'R3b') else 'allow'
            answered = []
            view = wait(service, lambda v: v['runtime'] in ('requires_action', 'exited'), 300)
            while view['runtime'] == 'requires_action' and len(answered) < 4:
                action = view['runtime_detail']['action_id']
                response = service.respond(action, decision, view['revision'])
                answered.append(dict(action_id=action, decision=decision,
                                     result=response.get('result'),
                                     error=response.get('error', {}).get('data')))
                view = wait(service, lambda v: v['runtime'] in ('requires_action', 'exited'), 900)
            if view['runtime'] != 'exited':
                view = wait(service, lambda v: v['runtime'] == 'exited', 900)
            events = service.events()
            effect = decision_effect(run, repo)
            requests = [e for e in events if e['kind'] == 'action_requested']
            # PIO's own declines are a different thing from the caller's, and
            # a receipt that merged them would hide which one happened.
            declined_by_pio = [e for e in events
                               if e['kind'] == 'request_declined_by_pio']
            extra['decision'] = dict(
                intended=decision,
                requested=bool(requests),
                request_count=len(requests),
                classifications=[e.get('classification')
                                 for e in requests + declined_by_pio],
                declined_by_pio=len(declined_by_pio),
                answered=answered,
                applied=[{k: e.get(k) for k in
                          ('decision', 'suggestions_offered', 'suggestions_acted_on',
                           'widening_fields_sent')}
                         for e in events if e['kind'] == 'control_applied'],
                # What the decision actually did, read from the world. Without
                # this the receipt would only prove PIO sent something.
                effect=effect,
                # The labeled fake runs no command, so the world says nothing
                # about a decision in a dry run and `held` stays null.
                held=None if (args.dry_run or not requests)
                     else effect['happened'] == (decision == 'allow'),
                note=None if requests else
                     'no permission request arrived: this run proves the turn '
                     'completed, not the decision path')
        elif run == 'R6':
            view = wait(service, lambda v: v['runtime'] == 'exited', 900)
            events = service.events()
            declined = [e for e in events if e['kind'] == 'request_declined_by_pio']
            surfaced = [e for e in events if e['kind'] == 'action_requested']
            marker = live_root() / 'outside' / 'pio-out-of-fixture-marker.txt'
            extra['decline'] = dict(
                # PIO's own decline, which is a different thing from the
                # caller's decision and is counted separately.
                declined_by_pio=len(declined),
                classifications=[e.get('classification') for e in declined],
                surfaced_to_caller=len(surfaced),
                marker_label='<outside>/pio-out-of-fixture-marker.txt',
                marker_sha256=sha(marker.read_bytes()) if marker.exists() else None,
                marker_still_present=marker.exists(),
                # R3 measured that a shell command is never offered at all. If
                # the read is not offered either, the harness reached outside
                # the workspace and PIO never got a say: an observed effect
                # with unresolved liability, not something PIO declined.
                note=None if declined else
                     'no request reached PIO: nothing was declined, and any '
                     'out-of-fixture effect below was observed, not authorized')
        else:
            view = wait(service, lambda v: v['runtime'] == 'exited', 900)
        extra['preflight'] = checks
        receipt = build_receipt(service, run, view, started, extra)
    finally:
        service.release()

    if receipt['usage']['reported']:
        observed = receipt['usage']['observed_total_tokens']
        book['runs'][run] = dict(observed_total_tokens=observed, charged=observed,
                                 charge_basis='observed',
                                 parts=receipt['usage']['parts'], at=started)
    elif run in PLANNED_CANCEL:
        # Unknown by design. Charged at the allowance so the cap is measured
        # against something, with the basis recorded beside the number.
        book['runs'][run] = dict(observed_total_tokens=None, usage='unknown',
                                 charged=CANCEL_ALLOWANCE, charge_basis=CHARGE_BASIS,
                                 model_calls_completed=0,
                                 evidence='the harness reported an empty usage '
                                          'block with no iterations',
                                 at=started)
    else:
        # Unknown and not planned: nothing is charged, because nothing is known
        # about it, and the stop rule below halts the sequence.
        book['runs'][run] = dict(observed_total_tokens=None, usage='unknown',
                                 charged=None, charge_basis='unknown, uncharged',
                                 at=started)
    book['charge_policy'] = charge_policy()
    if not args.dry_run:
        ledger_path().write_text(json.dumps(book, indent=2) + '\n')
    entry = book['runs'][run]
    receipt['usage']['charged'] = entry.get('charged')
    receipt['usage']['charge_basis'] = entry.get('charge_basis')
    receipt['cumulative'] = dict(charged=cumulative(book), observed=observed_total(book),
                                 cap=CAP, stop_at=STOP_AT, stop_rules_use='charged',
                                 charge_policy=charge_policy())
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
        if run in NOT_RUN:
            raise SystemExit(f'{run} is not run: {NOT_RUN[run]}')
        if run == 'R4b' and not args.dry_run:
            # Owner rule: if R3b draws no prompt, that is the finding. Stop
            # there, do not run R4b, and do not try a third command.
            prior = args.out / 'R3b.json'
            if not prior.exists():
                raise SystemExit('R4b needs R3b first: no R3b receipt')
            if not json.loads(prior.read_text())['decision']['requested']:
                raise SystemExit(
                    'R4b is not run: R3b drew no permission request, which is '
                    'the finding. No third command is tried.')
        run_one(run, args)


if __name__ == '__main__':
    main()
