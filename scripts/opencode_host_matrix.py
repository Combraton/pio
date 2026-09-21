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
import case_cleanup
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
    'the_exception_covers_the_provider_and_nothing_else',
    'model_without_the_dated_exception_refused',
    'credential_variable_refused_at_admission',
    'unqualified_executable_refused',
    'forbidden_flags_are_never_passed',
    'a_new_session_does_not_start_on_the_requested_model',
    'an_always_allow_option_is_offered_and_never_taken',
    'an_unattached_host_never_sees_the_request_the_harness_refuses',
    # Through the service, which is the only place delivery and usage land.
    'service_turn_completes',
    'service_measures_where_the_harness_reports_usage',
    'a_decision_is_selected_by_kind_never_by_id',
    'a_decision_whose_kind_is_not_offered_is_refused',
    'service_refuses_a_selection_that_did_not_take',
    'service_asks_for_the_narrower_mode_and_checks_it_took',
    'service_stops_when_the_model_selection_is_refused',
    'service_refuses_a_downgraded_session_before_any_prompt',
    'service_tells_a_harness_refusal_apart_from_a_pio_decline',
    'service_records_who_decided_and_what',
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
                 labeled_fake=True, env_extra=None, executable=None, mode=None):
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
        if mode is not None:
            config['opencode']['mode'] = mode
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
        # Kills anything still naming this store and asserts none survives,
        # so a host that refuses to exit is a failure rather than a leak.
        case_cleanup.release(self.root)


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

    def respond(self, action_id, decision, revision, identity='work'):
        """Answer one surfaced permission request as the caller would."""
        from public_api import command
        body = json.dumps({'decision': decision}).encode()
        envelope = command('execution.respond_action',
                           dict(kind='execution.execution', id=identity),
                           dict(action_id=action_id,
                                response=dict(digest=digest(body),
                                              media_type='application/json')),
                           command_id=f'{identity}.answer', revision=revision)
        envelope['extensions'] = {CONTENT: dict(media_type='application/json',
                                                text=body.decode())}
        with self.client() as c:
            return c.call(envelope)

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.',
               delivery_timeout=120):
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
                       timeouts=dict(delivery=delivery_timeout, execution_deadline=600))
        envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                           payload, command_id=identity)
        envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=brief.decode())}
        with self.client() as c:
            return c.call(envelope)

    def inspect(self, identity='work'):
        with self.client() as c:
            return c.query('execution.inspect', {'execution': identity})['result']

    def host_events(self):
        """The durable host's own append-only events file, read-only."""
        matches = sorted(self.store.glob('opencode-*.events.jsonl'))
        if not matches:
            return []
        return [json.loads(line) for line in matches[0].read_text().splitlines()
                if line.strip()]

    def cleanup(self):
        for daemon in self.daemons:
            daemon.kill()
            daemon.wait(timeout=10)
        for handle in self.files:
            handle.close()
        super().cleanup()


def new_session(case, extra_args=(), select=None):
    """Everything up to, and not including, a prompt.

    Including the model selection, because **a new session does not start on
    the client's model**. Measured against 2.0.11: `session/new` reports the
    harness's own default and only `session/set_config_option` moves it. The
    host does the same, and what comes back here is the harness's own report
    **after** the selection — which is what the guard is run against.
    """
    acp = Acp(case, extra_args)
    acp.call('initialize', {'protocolVersion': 1, 'clientCapabilities': {}})
    created = acp.call('session/new', {'cwd': str(case.fixtures), 'mcpServers': []})
    session = created['result']
    if select is not False:
        answer = acp.call('session/set_config_option',
                          {'sessionId': session['sessionId'], 'configId': 'model',
                           'value': select or REQUESTED})
        if answer.get('result'):
            session = dict(session, configOptions=answer['result']['configOptions'])
    return acp, session


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

    elif name == 'the_exception_covers_the_provider_and_nothing_else':
        # Owner decision, 2026-09-21: the dated exception was widened from one
        # model id to the provider, so a run may use whichever model on the
        # owner's plan suits the evidence. It is an allowlist of exactly one
        # provider, not a relaxation.
        for model in ('minimax-coding-plan/MiniMax-M3',
                      'minimax-coding-plan/MiniMax-M2.5-highspeed',
                      'minimax-coding-plan/MiniMax-M2'):
            case = Case(out, f'{name}-{model.split("/")[-1]}', model=model)
            status, record = case.admit()
            assert status == 0, (model, record)
            assert record['refusals'] == [], (model, record)
            case.finish()
            case.cleanup()
        # Everything else is still refused, and the record says which rule.
        # `opencode/` matters most: it is what a silently downgraded session
        # reports, so the admission refuses what the session guard would.
        for model, reason in (
                ('opencode/deepseek-v4.1-flash', 'provider_not_covered_by_the_exception'),
                ('opencode/jev-1.13-free', 'provider_not_covered_by_the_exception'),
                ('anthropic/claude-sonnet-5', 'provider_not_covered_by_the_exception'),
                ('juspay-grid/glm-latest', 'provider_excluded_by_the_owner')):
            case = Case(out, f'{name}-{model.split("/")[0]}-refused', model=model)
            status, record = case.admit()
            assert status == 3, (model, record)
            assert reason in [r['reason'] for r in record['refusals']], (model, record)
            assert record['session_started'] is False, (model, record)
            case.finish()
            case.cleanup()
        case = Case(out, name, model=REQUESTED)

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

    elif name == 'a_new_session_does_not_start_on_the_requested_model':
        # The case that would have caught the first live refusal. The host had
        # only ever *checked* the model and never *set* it, and the fake hid
        # that by echoing back whatever was requested.
        case = Case(out, name, model=REQUESTED)
        acp, unselected = new_session(case, select=False)
        current = {o['id']: o['currentValue'] for o in unselected['configOptions']}
        assert current['model'] == 'opencode/deepseek-v4.1-flash', current
        assert current['model'] != REQUESTED, current
        # A client that only looks is refused, which is what happened live.
        looked = case.guard(unselected)
        assert looked['allowed'] is False, looked
        # Selecting it, the way the harness actually allows, and the answer is
        # the harness's own updated report rather than a bare acknowledgement.
        answer = acp.call('session/set_config_option',
                          {'sessionId': unselected['sessionId'], 'configId': 'model',
                           'value': REQUESTED})
        assert answer.get('error') is None, answer
        selected = dict(unselected, configOptions=answer['result']['configOptions'])
        assert case.guard(selected)['allowed'] is True, selected
        set_markers = case.markers_of('config_option_set')
        assert len(set_markers) == 1, set_markers
        assert set_markers[0]['config_id'] == 'model', set_markers
        assert set_markers[0]['requested_value'] == REQUESTED, set_markers
        acp.close()

    elif name == 'an_always_allow_option_is_offered_and_never_taken':
        case = Case(out, name, model=REQUESTED, scenario={
            'permission_request': {'title': 'run a command', 'kind': 'execute',
                                   'input': {'command': 'touch marker.txt'}}})
        case.decision = {'outcome': 'selected', 'optionId': 'opt_1'}
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

    elif name == 'an_unattached_host_never_sees_the_request_the_harness_refuses':
        # The same property the Claude matrix asserts under this name, and the
        # same reason: a fake that always asks cannot show a harness that only
        # asks a client that is there. ACP declares no permission capability,
        # so the handshake is the whole of attachment here.
        case = Case(out, name, model=REQUESTED, scenario={
            'permission_request': {'title': 'run a command', 'kind': 'execute',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        acp = Acp(case)
        # No `initialize`: straight to a session and a prompt.
        created = acp.call('session/new', {'cwd': str(case.fixtures), 'mcpServers': []})
        acp.call('session/prompt', {'sessionId': created['result']['sessionId'],
                                    'prompt': [{'type': 'text', 'text': 'fixture task'}]})
        refused = case.markers_of('denied_by_harness_no_host_attached')
        assert len(refused) == 1, case.markers_of('prompt_received')
        assert refused[0]['attached'] is False, refused
        # Nobody was asked.
        assert case.markers_of('permission_decision') == [], case.markers_of('permission_decision')
        acp.close()
        # The control: the same scenario with the handshake does reach a client.
        attached = Case(out, name + '-attached', model=REQUESTED, scenario={
            'permission_request': {'title': 'run a command', 'kind': 'execute',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        attached.decision = {'outcome': 'selected', 'optionId': 'opt_3'}
        acp2, session2 = new_session(attached)
        acp2.call('session/prompt', {'sessionId': session2['sessionId'],
                                     'prompt': [{'type': 'text', 'text': 'fixture task'}]})
        assert len(attached.markers_of('permission_decision')) == 1, attached.markers_of('permission_decision')
        assert attached.markers_of('denied_by_harness_no_host_attached') == []
        acp2.close()

    elif name == 'service_tells_a_harness_refusal_apart_from_a_pio_decline':
        # A client is attached and the harness refuses anyway under its own
        # rules. A caller must be able to tell that from PIO declining, so it
        # is in the view and not only in the events.
        case = ServiceCase(out, name, model=REQUESTED, scenario={
            'decide_by_rules': True,
            'permission_request': {'title': 'run a command', 'kind': 'execute',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == {'code': 0}, view
        events = case.host_events()
        assert case.markers_of('denied_by_harness_rules_shadowed_the_host'), case.markers_of('prompt_received')
        # PIO declined nothing and was asked nothing.
        assert [e for e in events if e['kind'] == 'request_declined_by_pio'] == []
        assert [e for e in events if e['kind'] == 'action_requested'] == []
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_records_who_decided_and_what':
        # The same four situations the Claude matrix covers, asserted on the
        # record that reaches the receipt rather than on the events. Fix 2 of
        # the field audit had no test until this one.
        requests = {
            'deny': {'title': 'run a command', 'kind': 'execute',
                     'input': {'command': 'git tag pio-live-marker'}},
            'allow': {'title': 'run a command', 'kind': 'execute',
                      'input': {'command': 'git tag pio-live-marker'}},
            'nobody-answers': {'title': 'run a command', 'kind': 'execute',
                               'input': {'command': 'git tag pio-live-marker'}},
            # Outside the workspace, so PIO declines it before any caller is
            # asked and the classifier's decision is the one recorded.
            'pio-declines': {'title': 'read a file', 'kind': 'read',
                             'input': {'file_path': '/etc/hosts'}},
        }
        for situation, request in requests.items():
            case = ServiceCase(out, f'{name}-{situation}', model=REQUESTED,
                               scenario={'permission_request': request})
            case.start()
            # Five seconds for the unanswered one, so the case does not sit for
            # the default two minutes.
            case.submit(delivery_timeout=5 if situation == 'nobody-answers' else 120)
            if situation == 'pio-declines':
                view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited',
                            seconds=120)
                events = case.host_events()
                declined = [e for e in events if e['kind'] == 'request_declined_by_pio']
                assert len(declined) == 1, [e['kind'] for e in events]
                assert declined[0]['decided_by'] == 'pio', declined
                assert declined[0]['decision'] == 'deny', declined
                record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
                use = {u['tool_use_id']: u for u in record['tool_uses']}[
                    declined[0]['tool_use_id']]
                assert use['decided_by'] == 'pio', use
                assert use['decision'] == 'deny', use
                assert use['outcome'] == 'declined_by_pio', use
                assert use['placement'] == 'outside_fixture', use
                # PIO declined it, so nothing happened outside the workspace.
                assert record['out_of_fixture_effect_observed'] is False, record
                assert record['declined_by_pio_count'] == 1, record
                assert record['denied_by_harness_count'] == 0, record
                continue
            if situation == 'nobody-answers':
                view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited',
                            seconds=120)
                events = case.host_events()
                defaulted = [e for e in events if e['kind'] == 'request_denied_by_default']
                assert len(defaulted) == 1, [e['kind'] for e in events]
                assert defaulted[0]['after_seconds'] == 5, defaulted
                assert defaulted[0]['decided_by'] == 'pio', defaulted
                assert defaulted[0]['option_id'] == 'opt_3', defaulted
                assert defaulted[0]['option_kind'] == 'reject_once', defaulted
                assert defaulted[0]['widening_fields_sent'] == [], defaulted
                expected, decision = 'pio', 'deny'
                action_id = defaulted[0]['tool_use_id']
            else:
                view = poll(lambda: case.inspect(),
                            lambda v: v['runtime'] in ('requires_action', 'exited'),
                            seconds=120)
                assert view['runtime'] == 'requires_action', view
                answered = case.respond(view['runtime_detail']['action_id'],
                                        situation, view['revision'])
                assert answered.get('result', {}).get('outcome', {}).get(
                    'state') == 'answered', answered
                view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited',
                            seconds=120)
                events = case.host_events()
                applied = [e for e in events if e['kind'] == 'control_applied']
                assert len(applied) == 1, [e['kind'] for e in events]
                assert applied[0]['decided_by'] == 'caller', applied
                assert applied[0]['always_option_taken'] is False, applied
                expected, decision = 'caller', situation
                action_id = applied[0]['tool_use_id']
            record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
            use = {u['tool_use_id']: u for u in record['tool_uses']}[action_id]
            assert use['decided_by'] == expected, (situation, use)
            assert use['decision'] == decision, (situation, use)
            assert use['outcome'] == ('performed' if decision == 'allow'
                                      else f'denied_by_{expected}'.replace(
                                          'denied_by_pio', 'declined_by_pio')), (situation, use)
            assert record['denied_by_harness_count'] == 0, (situation, record)

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
        # The measure itself, on the host's own record. Found where the
        # harness put it — `usage`, not `_meta.usage` — and summed from
        # whatever counters are there rather than from a list written in
        # advance. Both of those were wrong until a live turn said so: one
        # read a path that was never there, the other dropped `thoughtTokens`.
        usage = [e for e in case.host_events() if e['kind'] == 'usage']
        assert len(usage) == 1, [e['kind'] for e in case.host_events()]
        assert usage[0]['path'] == 'usage', usage
        assert sorted(usage[0]['parts']) == ['inputTokens', 'outputTokens',
                                             'thoughtTokens'], usage
        # The harness's own total is kept beside PIO's sum, never added to it.
        assert usage[0]['parts_sum_matches_reported_total'] is True, usage
        assert usage[0]['total']['totalTokens'] == usage[0]['reported_total'], usage
        assert len(case.markers_of('prompt_received')) == 1, case.markers_of('prompt_received')
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_measures_where_the_harness_reports_usage':
        # R1's question, asked offline of a harness whose answer is already
        # known. This cannot say what OpenCode does — only a live run can —
        # but it can show that the census is capable of either answer, which
        # a lookup at the one place the Claude adapter assumed was not.
        seen = {}
        for label, scenario in (('at-turn-end', {'message_chunks': 3}),
                                ('as-it-goes', {'message_chunks': 3,
                                                'usage_on_updates': True})):
            case = ServiceCase(out, f'{name}-{label}', model=REQUESTED,
                               scenario=dict(scenario, usage_total=480))
            case.start()
            case.submit()
            poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
            census = [e for e in case.host_events() if e['kind'] == 'usage_granularity']
            assert len(census) == 1, [e['kind'] for e in case.host_events()]
            seen[label] = census[0]
            case.finish()
            case.cleanup()
        end, going = seen['at-turn-end'], seen['as-it-goes']
        # Every update is counted by its own kind, whether or not it carries
        # usage, so the census says what the turn was made of.
        assert end['session_update_kinds']['agent_message_chunk'] == 3, end
        assert going['session_update_kinds']['agent_message_chunk'] == 3, going
        # The difference the census exists to report. Both report at the end;
        # one also reports on the way, and the count is what says so.
        assert end['session_update_kinds'].get('usage_update') == 1, end
        assert end['usage_bearing_update_kinds'] == {'usage_update': 1}, end
        assert end['report_count'] == 2, end
        assert going['session_update_kinds'].get('usage_update') == 3, going
        assert going['usage_bearing_update_kinds'] == {'usage_update': 3}, going
        assert going['report_count'] == 4, going
        assert end['reported_during_turn'] is True, end
        assert going['reported_during_turn'] is True, going
        assert end['reported_at_turn_end'] is True, end
        assert going['reported_at_turn_end'] is True, going
        # **The property R1 bought.** `usage_update` names none of its fields
        # `usage` or `*tokens` — it sends `used`, `size` and `cost` — so a
        # census searching field names alone misses the one update kind that
        # actually carries usage. It is found by its own kind, and the whole
        # update is recorded.
        updates = [r for r in end['reports'] if r['where'] == 'session/update']
        assert [r['matched_by'] for r in updates] == ['the update kind'], updates
        assert updates[0]['paths'] == [], updates
        assert updates[0]['values'][0]['used'] == 480, updates
        assert updates[0]['values'][0]['cost'] == {'amount': 0, 'currency': 'USD'}, updates
        # And the turn result's own usage, found where the harness put it —
        # `usage`, not `_meta.usage` — with every counter summed. Reading one
        # expected path and two expected counters recorded a live turn that
        # cost 7,910 tokens as unknown.
        result = [r for r in end['reports'] if r['where'] == 'session/prompt result']
        assert [r['paths'] for r in result] == [['usage']], result
        assert 'thoughtTokens' in result[0]['values'][0], result
        case = Case(out, name, model=REQUESTED)

    elif name == 'a_decision_is_selected_by_kind_never_by_id':
        # Option ids are the agent's to invent; the meaning is in `kind`. The
        # fake's ids are `opt_1`, `opt_2`, `opt_3`, so a host that hard-codes
        # `allow` or `reject` selects nothing and fails here rather than in
        # front of the owner's harness.
        for decision, kind, option_id in (('deny', 'reject_once', 'opt_3'),
                                          ('allow', 'allow_once', 'opt_1')):
            case = ServiceCase(out, f'{name}-{decision}', model=REQUESTED, scenario={
                'permission_request': {'title': 'run a command', 'kind': 'execute',
                                       'input': {'command': 'git tag pio-live-marker'}}})
            case.start()
            case.submit()
            view = poll(lambda: case.inspect(),
                        lambda v: v['runtime'] in ('requires_action', 'exited'), seconds=120)
            assert view['runtime'] == 'requires_action', view
            case.respond(view['runtime_detail']['action_id'], decision, view['revision'])
            poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
            events = case.host_events()
            applied = [e for e in events if e['kind'] == 'control_applied']
            assert len(applied) == 1, [e['kind'] for e in events]
            assert applied[0]['applied'] is True, applied
            assert applied[0]['option_kind'] == kind, applied
            assert applied[0]['option_id'] == option_id, applied
            assert applied[0]['always_option_taken'] is False, applied
            # The option list as the agent offered it: the measurement ADR 005
            # promises, and the only one PIO has until a live request arrives.
            observed = [e for e in events if e['kind'] == 'permission_options_observed']
            assert len(observed) == 1, [e['kind'] for e in events]
            assert observed[0]['option_ids'] == ['opt_1', 'opt_2', 'opt_3'], observed
            assert observed[0]['option_kinds'] == ['allow_once', 'allow_always',
                                                   'reject_once'], observed
            # No id is its own kind, so nothing above can pass by hard-coding.
            assert not set(observed[0]['option_ids']) & set(observed[0]['option_kinds'])
            # And the harness saw the kind PIO meant.
            decided = case.markers_of('permission_decision')
            assert len(decided) == 1, decided
            assert decided[0]['option_kind'] == kind, decided
            assert decided[0]['always_option_taken'] is False, decided
            case.finish()
            case.cleanup()
        case = Case(out, name, model=REQUESTED)

    elif name == 'a_decision_whose_kind_is_not_offered_is_refused':
        # PIO never approximates a decision. An allow it cannot express
        # single-use is not upgraded to the always option sitting next to it;
        # it is refused, and the refusal is recorded as PIO's, not the
        # caller's, because the caller did not ask for a refusal.
        for decision, omit in (('allow', ['allow_once']), ('deny', ['reject_once'])):
            case = ServiceCase(out, f'{name}-{decision}', model=REQUESTED, scenario={
                'permission_request': {'title': 'run a command', 'kind': 'execute',
                                       'omit_option_kinds': omit,
                                       'input': {'command': 'git tag pio-live-marker'}}})
            case.start()
            case.submit()
            view = poll(lambda: case.inspect(),
                        lambda v: v['runtime'] in ('requires_action', 'exited'), seconds=120)
            assert view['runtime'] == 'requires_action', view
            case.respond(view['runtime_detail']['action_id'], decision, view['revision'])
            poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
            events = case.host_events()
            refused = [e for e in events if e['kind'] == 'option_kind_not_offered']
            assert len(refused) == 1, [e['kind'] for e in events]
            assert refused[0]['required_kind'] == omit[0], refused
            assert refused[0]['requested_decision'] == decision, refused
            assert refused[0]['outcome_sent'] == 'cancelled', refused
            applied = [e for e in events if e['kind'] == 'control_applied'][0]
            assert applied['applied'] is False, applied
            assert applied['option_id'] is None, applied
            assert applied['decision'] == 'deny', applied
            assert applied['decided_by'] == 'pio', applied
            # The harness took no option at all: nothing was selected, and in
            # particular not the always option it was still offering.
            decided = case.markers_of('permission_decision')
            assert len(decided) == 1, decided
            assert decided[0]['outcome'] == 'cancelled', decided
            assert decided[0]['option_kind'] is None, decided
            assert decided[0]['always_option_taken'] is False, decided
            # The always option was there the whole time, in both cases, and
            # was still not taken: that is the point of the refusal.
            assert decided[0]['always_option_offered'] is True, decided
            # And the record that reaches the receipt says PIO refused it.
            record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
            use = {u['tool_use_id']: u for u in record['tool_uses']}[applied['tool_use_id']]
            assert use['decided_by'] == 'pio', use
            assert use['decision'] == 'deny', use
            assert use['outcome'] == 'declined_by_pio', use
            case.finish()
            case.cleanup()
        case = Case(out, name, model=REQUESTED)

    elif name == 'service_refuses_a_selection_that_did_not_take':
        # A harness that answers the selection without an error and changes
        # nothing. PIO must not read a clean response as evidence that the
        # selection took: the guard runs on the harness's own report, not on
        # the absence of an error.
        case = ServiceCase(out, name, model=REQUESTED,
                           scenario={'ignore_model_selection': True})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(),
                    lambda v: v['delivery'] in ('failed_before_delivery', 'not_delivered'))
        assert view['delivery'] == 'failed_before_delivery', view
        # A refusal is a finished execution. Before this the runtime sat at
        # `preparing` for ever and a caller could not tell it from a slow
        # start; the first live run hung there.
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == 'unavailable', view
        events = case.host_events()
        selected = [e for e in events if e['kind'] == 'model_selected']
        assert len(selected) == 1, [e['kind'] for e in events]
        assert selected[0]['error'] is None, selected
        created = [e for e in events if e['kind'] == 'session_created'][0]
        assert created['model_selected_by_pio'] is True, created
        assert created['model_on_creation'] == 'opencode/deepseek-v4.1-flash', created
        assert created['reported_model'] == 'opencode/deepseek-v4.1-flash', created
        assert created['model_matches_requested'] is False, created
        assert case.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_asks_for_the_narrower_mode_and_checks_it_took':
        # R2 measured this harness executing a shell command with no request
        # reaching PIO at all. `mode` is the only per-session lever that does
        # not edit the owner's configuration, and `plan` is the narrower of
        # the two. Narrowing is allowed; widening is refused at admission.
        case = ServiceCase(out, name, model=REQUESTED, mode='plan')
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        events = case.host_events()
        selected = [e for e in events if e['kind'] == 'mode_selected']
        assert len(selected) == 1, [e['kind'] for e in events]
        assert selected[0]['requested_mode'] == 'plan', selected
        assert selected[0]['reported_mode'] == 'plan', selected
        assert selected[0]['mode_matches_requested'] is True, selected
        created = [e for e in events if e['kind'] == 'session_created'][0]
        assert created['requested_mode'] == 'plan', created
        assert created['reported_mode'] == 'plan', created
        assert case.markers_of('config_option_set')[-1]['current_mode'] == 'plan', \
            case.markers_of('config_option_set')
        case.finish()
        case.cleanup()

        # A harness that accepts the narrower posture and stays on the one it
        # had. PIO reads the report back, not the absence of an error, so this
        # is a refusal with the brief still inside it.
        ignored = ServiceCase(out, f'{name}-ignored', model=REQUESTED, mode='plan',
                              scenario={'ignore_mode_selection': True})
        ignored.start()
        ignored.submit()
        view = poll(lambda: ignored.inspect(),
                    lambda v: v['delivery'] in ('failed_before_delivery', 'not_delivered'))
        assert view['delivery'] == 'failed_before_delivery', view
        events = ignored.host_events()
        assert [e for e in events if e['kind'] == 'mode_selected'][0][
            'mode_matches_requested'] is False, events
        error = [e for e in events if e['kind'] == 'host_error']
        assert error and 'mode_not_applied' in error[0]['error'], error
        assert ignored.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        view = poll(lambda: ignored.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == 'unavailable', view
        ignored.finish()
        ignored.cleanup()

        # And widening is refused before anything starts: `build` is the
        # harness's own default, so asking for it could only ever loosen a
        # session that was already narrower.
        for mode in ('build', 'bypass'):
            wide = Case(out, f'{name}-{mode}-refused', model=REQUESTED, mode=mode)
            status, record = wide.admit()
            assert status == 3, (mode, record)
            assert 'mode_is_not_a_narrowing_one' in [r['reason'] for r in record['refusals']], record
            wide.finish()
            wide.cleanup()
        case = Case(out, name, model=REQUESTED)

    elif name == 'service_stops_when_the_model_selection_is_refused':
        # The harness refuses the selection outright. PIO stops with the brief
        # still inside it rather than running on whatever model it got.
        case = ServiceCase(out, name, model=REQUESTED,
                           scenario={'refuse_model_selection': True})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(),
                    lambda v: v['delivery'] in ('failed_before_delivery', 'not_delivered'))
        assert view['delivery'] == 'failed_before_delivery', view
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == 'unavailable', view
        events = case.host_events()
        selected = [e for e in events if e['kind'] == 'model_selected']
        assert len(selected) == 1, [e['kind'] for e in events]
        assert selected[0]['error'] is not None, selected
        error = [e for e in events if e['kind'] == 'host_error']
        assert error and 'model_selection_refused' in error[0]['error'], error
        # Nothing reached the guard, and nothing reached the harness.
        assert [e for e in events if e['kind'] == 'session_created'] == []
        assert case.markers_of('prompt_received') == [], 'a prompt was sent after a refusal'
        assert len(case.markers_of('model_selection_refused')) == 1, \
            case.markers_of('model_selection_refused')
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
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == 'unavailable', view
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
