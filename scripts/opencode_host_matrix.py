#!/usr/bin/env python3
"""Offline OpenCode adapter matrix, driven against the labeled fake ACP server.

Never a real OpenCode, never a live run, no model call at any point, and
**never anywhere near the owner's running service**: the fake is a child of
this process on stdio, and every case records the owner's `serve --service`
process before and after and fails if it moved.

The case that matters most is the owner's rule of 2026-09-20: refuse unless
the session's reported provider and model equal the requested ones. It is
proven here the only way it can be — by showing that when the session reports
a different model, **no prompt is ever sent**.

Each case runs three times; a case passes only when all three agree.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import queue
import shutil
import subprocess
import tempfile
import hashlib
import threading
import time


def digest(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


def poll(action, predicate, seconds=60):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            last = action()
            if predicate(last):
                return last
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.05)
    raise AssertionError(f'bounded observation timed out; last={json.dumps(last)[:1200]}')

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery',
            'execution.workspaces', 'execution.usage', 'execution.actions']
REQUESTED = 'minimax-coding-plan/MiniMax-M2.7-highspeed'
DOWNGRADE = 'opencode/nemotron-3.5-lightning-free'
CASES = [
    'session_reports_the_requested_model',
    'silent_downgrade_refused_before_any_prompt',
    'wrong_model_from_the_right_provider_refused',
    'a_session_reporting_nothing_is_refused',
    'excluded_provider_refused_at_admission',
    'model_without_the_dated_exception_refused',
    'credential_variable_refused_at_admission',
    'unqualified_executable_refused',
    'forbidden_flags_are_never_passed',
    'an_always_allow_option_is_offered_and_never_taken',
    # Through the service, which is the only place delivery and usage land.
    'service_turn_completes',
    'service_refuses_a_downgraded_session_before_any_prompt',
]


def owner_service():
    out = subprocess.run(['ps', '-Ao', 'pid,lstart,command'], capture_output=True, text=True).stdout
    return sorted(l.strip() for l in out.splitlines()
                  if 'serve --service' in l and 'opencode' in l and 'grep' not in l)


class Acp:
    """One ACP session against the labeled fake, on stdio."""

    def __init__(self, case, extra_args=()):
        self.case = case
        self.child = subprocess.Popen(
            [str(case.wrapper), 'acp', *extra_args], cwd=str(case.fixtures), env=case.env(),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1)
        self.queue = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.counter = 0

    def _read(self):
        for line in self.child.stdout:
            self.queue.put(line)

    def call(self, method, params, seconds=30):
        self.counter += 1
        self.child.stdin.write(json.dumps(
            {'jsonrpc': '2.0', 'id': self.counter, 'method': method, 'params': params}) + '\n')
        self.child.stdin.flush()
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                message = json.loads(self.queue.get(timeout=1))
            except (queue.Empty, ValueError):
                continue
            if message.get('id') == self.counter:
                return message
            # An agent-initiated request, such as a permission prompt.
            if message.get('method') == 'session/request_permission':
                self.permission = message
                if self.case.decision is not None:
                    self.child.stdin.write(json.dumps(
                        {'jsonrpc': '2.0', 'id': message['id'],
                         'result': {'outcome': self.case.decision}}) + '\n')
                    self.child.stdin.flush()
        raise AssertionError(f'no response to {method}')

    def close(self):
        self.child.kill()
        self.child.wait(timeout=10)


# Every case registers itself, so a failing assertion cannot leak a daemon or
# its store. Cleanup on the success path alone left six services running,
# parented to init, during this adapter's development.
LIVE_CASES = []


def release_live_cases():
    while LIVE_CASES:
        case = LIVE_CASES.pop()
        try:
            case.cleanup()
        except Exception:
            pass


class Case:
    def __init__(self, out, name, scenario=None, model=None, exception=True,
                 labeled_fake=True, env_extra=None, executable=None):
        self.name = name
        self.out = out / name
        self.out.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='pio-oc-', dir='/tmp')).resolve()
        os.chmod(self.root, 0o700)
        self.fixtures = self.root / 'fixtures'
        self.config_dir = self.root / 'opencode-config'
        self.markers = self.root / 'markers'
        self.work = self.root / 'work'
        for directory in (self.fixtures, self.config_dir, self.markers, self.work):
            directory.mkdir()
        (self.fixtures / 'README.md').write_text('PIO M3b offline fixture. No task runs here.\n')
        self.wrapper = self.root / 'fake-opencode'
        self.wrapper.write_text(f"#!/bin/sh\nexec '{BINARY}' opencode fake-acp \"$@\"\n")
        self.wrapper.chmod(0o755)
        self.scenario = dict(scenario or {}, markers=str(self.markers))
        self.decision = None
        self.owner_before = owner_service()
        env = {'PATH': '/usr/bin:/bin', 'HOME': str(self.root),
               'USER': os.environ.get('USER', 'pio')}
        env.update(env_extra or {})
        config = {'opencode': {
            'executable': str(executable or self.wrapper),
            'env': dict(env, PIO_OPENCODE_FAKE_SCENARIO=json.dumps(self.scenario))
                   if labeled_fake else env,
            'config_dir': str(self.config_dir), 'home': str(self.root),
            'fixture_root': str(self.fixtures), 'labeled_fake': labeled_fake}}
        if model is not None:
            config['opencode']['model'] = model
        if exception:
            config['opencode']['test_only_model_exception'] = \
                'owner-2026-09-20-m3b-opencode-fixture-runs'
        self.config_path = self.root / 'service.json'
        self.config_path.write_text(json.dumps(config))
        LIVE_CASES.append(self)

    def env(self):
        return {'PATH': '/usr/bin:/bin', 'HOME': str(self.root),
                'USER': os.environ.get('USER', 'pio'),
                'PIO_OPENCODE_FAKE_SCENARIO': json.dumps(self.scenario)}

    def admit(self):
        result = subprocess.run(
            [str(BINARY), 'opencode', 'service-admit', '--config', str(self.config_path),
             '--work', str(self.work)],
            capture_output=True, text=True, env=self.env(), timeout=180)
        record = json.loads(result.stdout) if result.stdout.strip() else {}
        (self.out / 'admission.json').write_text(json.dumps(record, indent=2, sort_keys=True))
        return result.returncode, record

    def markers_of(self, event):
        path = self.markers / 'fake-opencode-acp.jsonl'
        if not path.exists():
            return []
        return [json.loads(l) for l in path.read_text().splitlines()
                if l.strip() and json.loads(l)['event'] == event]

    def guard(self, session):
        """PIO's own decision about the session it was given."""
        result = subprocess.run(
            [str(BINARY), 'opencode', 'session-guard', '--requested', REQUESTED],
            input=json.dumps(session), capture_output=True, text=True)
        assert result.returncode in (0, 3), result.stderr
        return json.loads(result.stdout)

    def finish(self):
        assert owner_service() == self.owner_before, \
            "the owner's OpenCode service moved during this case"

    def cleanup(self):
        if self in LIVE_CASES:
            LIVE_CASES.remove(self)
        shutil.rmtree(self.root, ignore_errors=True)


class ServiceCase(Case):
    """Driven through `pio serve-opencode` over the public Unix API."""

    def __init__(self, out, name, **kw):
        super().__init__(out, name, **kw)
        from public_api import Client, CREDENTIAL

        class OpenCodeClient(Client):
            def __init__(self, path, transcript=None, timeout=10):
                self.features = FEATURES
                super().__init__(path, transcript, timeout)

            def query(self, operation, payload):
                if operation == 'core.negotiate':
                    payload = dict(payload, profiles=[
                        dict(name='core', majors=[1], required=True,
                             required_features=['core.events', 'core.capabilities', 'core.effects'],
                             optional_features=[]),
                        dict(name='execution', majors=[1], required=True,
                             required_features=FEATURES, optional_features=[])])
                return super().query(operation, payload)

        self._client = OpenCodeClient
        self.store = self.root / 'store'
        self.socket = self.root / 'public.sock'
        self.transcript = self.out / 'public-transcript.jsonl'
        self.daemons, self.files = [], []
        config = json.loads(self.config_path.read_text())
        config['format'] = 'pio-opencode-service/1'
        config['protocol'] = dict(
            format='combraton-conformance-config/1', principal='owner',
            credentials=[dict(credential=CREDENTIAL)],
            executor=dict(host_id='opencode-host'))
        self.config_path.write_text(json.dumps(config))

    def client(self):
        return self._client(self.socket, self.transcript)

    def start(self, expect_ready=True):
        n = len(self.daemons)
        stdout = (self.out / f'daemon-{n}.stdout').open('w')
        stderr = (self.out / f'daemon-{n}.stderr').open('w')
        self.files += [stdout, stderr]
        daemon = subprocess.Popen(
            [str(BINARY), 'serve-opencode', '--data-dir', str(self.store),
             '--config', str(self.config_path), '--socket', str(self.socket)],
            stdout=stdout, stderr=stderr, env=self.env())
        self.daemons.append(daemon)
        if expect_ready:
            poll(lambda: self.client().query('core.describe', {}), lambda r: 'result' in r)
        return daemon

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.'):
        from public_api import command
        repo = self.fixtures / identity
        repo.mkdir(exist_ok=True)
        (repo / 'README.md').write_text('fixture\n')
        git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True,
                                        capture_output=True, text=True)
        git('init', '-q')
        git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
        git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
            'commit', '-q', '-m', 'fixture')
        base = git('rev-parse', 'HEAD').stdout.strip()
        payload = dict(brief=dict(digest=digest(brief), media_type='text/plain'),
                       workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                       timeouts=dict(delivery=120, execution_deadline=600))
        envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                           payload, command_id=identity)
        envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=brief.decode())}
        with self.client() as c:
            return c.call(envelope)

    def inspect(self, identity='work'):
        with self.client() as c:
            return c.query('execution.inspect', {'execution': identity})['result']

    def cleanup(self):
        for daemon in self.daemons:
            daemon.kill()
            daemon.wait(timeout=10)
        for handle in self.files:
            handle.close()
        super().cleanup()


def new_session(case, extra_args=()):
    """Everything up to, and not including, a prompt."""
    acp = Acp(case, extra_args)
    acp.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {}})
    created = acp.call('session/new', {'cwd': str(case.fixtures), 'mcpServers': []})
    return acp, created['result']


def run_case(out, name):
    if name == 'session_reports_the_requested_model':
        case = Case(out, name, model=REQUESTED)
        acp, session = new_session(case)
        guard = case.guard(session)
        assert guard['allowed'] is True, guard
        assert guard['checked_before_delivery'] is True, guard
        acp.call('session/prompt', {'sessionId': session['sessionId'],
                                    'prompt': [{'type': 'text', 'text': 'fixture task'}]})
        assert len(case.markers_of('prompt_received')) == 1, case.markers_of('prompt_received')
        acp.close()

    elif name == 'silent_downgrade_refused_before_any_prompt':
        # The owner's rule. A missing route does not refuse — it substitutes a
        # free third-party model — so the only safe answer is to compare what
        # the session reports against what was requested, and stop.
        case = Case(out, name, scenario={'model': DOWNGRADE}, model=REQUESTED)
        acp, session = new_session(case)
        guard = case.guard(session)
        assert guard['allowed'] is False, guard
        assert guard['reported_provider'] == 'opencode', guard
        reasons = [u['reason'] for u in guard['unresolved']]
        assert "the session's model is not the requested one" in reasons, guard
        assert "the session's provider is not the requested one" in reasons, guard
        # The proof: PIO stops here, so the fixture content never leaves.
        assert case.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        assert len(case.markers_of('session_created')) == 1, case.markers_of('session_created')
        acp.close()
        (case.out / 'guard.json').write_text(json.dumps(guard, indent=2))

    elif name == 'wrong_model_from_the_right_provider_refused':
        case = Case(out, name, scenario={'model': 'minimax-coding-plan/MiniMax-M3'}, model=REQUESTED)
        acp, session = new_session(case)
        guard = case.guard(session)
        assert guard['allowed'] is False, guard
        assert guard['reported_provider'] == 'minimax-coding-plan', guard
        assert case.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        acp.close()

    elif name == 'a_session_reporting_nothing_is_refused':
        case = Case(out, name, model=REQUESTED)
        for session in ({}, {'sessionId': 'x', 'configOptions': []}):
            guard = case.guard(session)
            assert guard['allowed'] is False, guard

    elif name == 'excluded_provider_refused_at_admission':
        # Owner decision: no PIO run uses this provider for any purpose.
        case = Case(out, name, model='juspay-grid/glm-latest')
        status, record = case.admit()
        assert status == 3, record
        assert 'provider_excluded_by_the_owner' in [r['reason'] for r in record['refusals']], record
        assert record['session_started'] is False, record

    elif name == 'model_without_the_dated_exception_refused':
        case = Case(out, name, model=REQUESTED, exception=False)
        status, record = case.admit()
        assert status == 3, record
        assert 'model_requires_the_dated_test_only_exception' in \
            [r['reason'] for r in record['refusals']], record

    elif name == 'credential_variable_refused_at_admission':
        for variable in ('MINIMAX_API_KEY', 'OPENCODE_PASSWORD', 'ANTHROPIC_AUTH_TOKEN'):
            case = Case(out, f'{name}-{variable}', model=REQUESTED,
                        env_extra={variable: 'not-a-real-secret'})
            status, record = case.admit()
            assert status == 3, record
            assert 'env_carries_a_credential_variable' in \
                [r['reason'] for r in record['refusals']], record
            case.finish()
            case.cleanup()
        case = Case(out, name, model=REQUESTED)

    elif name == 'unqualified_executable_refused':
        case = Case(out, name, model=REQUESTED, labeled_fake=False)
        status, record = case.admit()
        assert status == 3, record
        assert record['qualification']['qualified'] is False, record
        assert record['session_started'] is False, record

    elif name == 'forbidden_flags_are_never_passed':
        # `--auto` approves everything not explicitly denied, and the owner has
        # no deny rules. `--server` would reach the owner's own service.
        case = Case(out, name, model=REQUESTED)
        acp, session = new_session(case)
        acp.call('session/prompt', {'sessionId': session['sessionId'],
                                    'prompt': [{'type': 'text', 'text': 'fixture task'}]})
        assert case.markers_of('forbidden_flag') == [], case.markers_of('forbidden_flag')
        acp.close()
        # The detector works, so its silence above is evidence.
        control = Case(out, f'{name}-control', model=REQUESTED)
        detector, _ = new_session(control, extra_args=('--auto',))
        assert [m['flags'] for m in control.markers_of('forbidden_flag')] == [['--auto']], \
            control.markers_of('forbidden_flag')
        detector.close()
        control.finish()
        control.cleanup()

    elif name == 'an_always_allow_option_is_offered_and_never_taken':
        case = Case(out, name, model=REQUESTED, scenario={
            'permission_request': {'title': 'run a command', 'kind': 'execute',
                                   'input': {'command': 'touch marker.txt'}}})
        case.decision = {'outcome': 'selected', 'optionId': 'allow'}
        acp, session = new_session(case)
        acp.call('session/prompt', {'sessionId': session['sessionId'],
                                    'prompt': [{'type': 'text', 'text': 'fixture task'}]})
        decided = case.markers_of('permission_decision')
        assert len(decided) == 1, decided
        # The harness offers an always-allow option on every request; taking it
        # would widen a permission beyond the request that prompted it.
        assert decided[0]['always_option_offered'] is True, decided
        assert decided[0]['always_option_taken'] is False, decided
        assert decided[0]['widening_fields_received'] == [], decided
        acp.close()

    elif name == 'service_turn_completes':
        case = ServiceCase(out, name, model=REQUESTED)
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['delivery'] == 'acknowledged', view
        delivery = view['deliveries'][0]
        assert delivery['evidence']['class'] == 'native_session_update', delivery
        # This harness returns no acknowledgment identifier, so no proof class
        # is claimed. ADR 005 section 7.
        assert 'proof_class' not in delivery or delivery['proof_class'] is None, delivery
        assert view['usage']['liability'] == 'resolved', view['usage']
        assert [o['measure'] for o in view['usage']['observations']] == \
            ['opencode.tokens.total'], view['usage']
        assert len(case.markers_of('prompt_received')) == 1, case.markers_of('prompt_received')
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_refuses_a_downgraded_session_before_any_prompt':
        # The owner's rule, through the durable host: the session reports a
        # different provider, so the host refuses and the brief never leaves.
        case = ServiceCase(out, name, scenario={'model': DOWNGRADE}, model=REQUESTED)
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(),
                    lambda v: v['delivery'] in ('failed_before_delivery', 'not_delivered'))
        assert view['delivery'] == 'failed_before_delivery', view
        assert case.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        assert len(case.markers_of('session_created')) == 1, case.markers_of('session_created')
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    else:
        raise AssertionError(f'unknown case {name}')
    case.finish()
    case.cleanup()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--case', action='append')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    selected = args.case or CASES
    results, failures = [], []
    for name in selected:
        for repetition in range(args.repetitions):
            try:
                run_case(args.out / f'{name}-{repetition}', name)
                results.append((name, repetition, 'pass', None))
            except Exception as error:
                results.append((name, repetition, 'fail', repr(error)))
                failures.append((name, repetition, repr(error)))
            finally:
                release_live_cases()
    summary = dict(format='pio-opencode-host-matrix/1', platform=platform.platform(),
                   harness='pio-fake-opencode-acp', model_calls=0, live_run=False,
                   owner_service_touched=False,
                   cases=len(selected), repetitions=args.repetitions,
                   attempts=len(results), failures=len(failures),
                   results=[dict(case=c, repetition=r, status=s, error=e)
                            for c, r, s, e in results])
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    for name, repetition, error in failures:
        print(f'FAIL {name}#{repetition}: {error}')
    print(f'{len(selected)} cases x {args.repetitions} = {len(results)} attempts, '
          f'{len(failures)} failures')
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
