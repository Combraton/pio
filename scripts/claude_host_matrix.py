#!/usr/bin/env python3
"""Offline Claude adapter matrix: the pre-spawn admission decisions and the
stream shapes, driven against the labeled fake CLI. Never a real Claude Code,
never a live run, and no model call at any point.

Independent witnesses: the fake CLI's own marker file (turn received, the
permission decision it was given, whether that decision carried a widening
field), the admission record the service produces before any stream starts,
and the process exit status. Every execution is labeled `pio-fake-claude-cli`.

Each case runs three times; a case passes only when all three agree.
"""
import argparse
import json
import os
import hashlib
import time
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery',
            'execution.workspaces', 'execution.usage', 'execution.actions']
STREAM_ARGS = ['--print', '--input-format', 'stream-json', '--output-format', 'stream-json',
               '--verbose', '--replay-user-messages']
CASES = [
    'turn_completes',
    'replay_acknowledges_delivery',
    'permission_denied',
    'permission_allowed',
    'out_of_fixture_request_declined_by_pio',
    'unclassifiable_request_surfaced_not_auto_allowed',
    'widening_decision_never_sent',
    'unqualified_executable_refused',
    'missing_credential_route_refused',
    'permission_mode_not_the_configured_default_refused',
    'surface_drift_refused',
    # Through the service, which is the only place these can be observed.
    'service_turn_completes',
    'service_refuses_an_unqualified_executable_at_start',
    'service_answers_a_request_it_will_not_act_on',
    'service_interrupt_escalates_when_the_signal_is_ignored',
    'service_restart_reattaches_without_a_duplicate_launch',
]


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
    raise AssertionError(f'bounded observation timed out; last={json.dumps(last)[:1500]}')


class Case:
    """One offline case: a fixture workspace, a labeled fake, and a settings
    file standing in for the user's own."""

    def __init__(self, out, name, scenario=None, settings=None, permission_mode='acceptEdits',
                 labeled_fake=True, executable=None):
        self.name = name
        self.out = out / name
        self.out.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='pio-cl-', dir='/tmp')).resolve()
        os.chmod(self.root, 0o700)
        self.fixtures = self.root / 'fixtures'
        self.config_dir = self.root / 'claude-config'
        self.markers = self.root / 'markers'
        self.work = self.root / 'work'
        for directory in (self.fixtures, self.config_dir, self.markers, self.work):
            directory.mkdir()
        (self.fixtures / 'README.md').write_text('PIO M3 offline fixture. No task runs here.\n')
        # Stands in for the user's settings: an accept-edits default, so a
        # request for anything else must be refused before a spawn.
        (self.config_dir / 'settings.json').write_text(json.dumps(
            settings if settings is not None else
            {'permissions': {'defaultMode': 'acceptEdits', 'allow': ['Bash(cat)']}}))
        self.wrapper = self.root / 'fake-claude'
        self.wrapper.write_text(f"#!/bin/sh\nexec '{BINARY}' claude fake-cli \"$@\"\n")
        self.wrapper.chmod(0o755)
        self.scenario = dict(scenario or {}, markers=str(self.markers))
        self.config_path = self.root / 'service.json'
        self.config_path.write_text(json.dumps({'claude': {
            'executable': str(executable or self.wrapper),
            'env': {'PATH': '/usr/bin:/bin', 'HOME': str(self.root), 'USER': os.environ.get('USER', 'pio')},
            'config_dir': str(self.config_dir), 'home': str(self.root),
            'fixture_root': str(self.fixtures),
            'permission_mode': permission_mode, 'labeled_fake': labeled_fake}}))
        if labeled_fake:
            # The fake's scenario travels in the service environment, which the
            # admission check accepts only because `labeled_fake` is true.
            config = json.loads(self.config_path.read_text())
            config['claude']['env']['PIO_CLAUDE_FAKE_SCENARIO'] = json.dumps(self.scenario)
            self.config_path.write_text(json.dumps(config))

    def env(self):
        return {'PATH': '/usr/bin:/bin', 'HOME': str(self.root),
                'USER': os.environ.get('USER', 'pio'),
                'PIO_CLAUDE_FAKE_SCENARIO': json.dumps(self.scenario)}

    def admit(self):
        """The decisions a service makes before it spawns anything for a turn."""
        result = subprocess.run(
            [str(BINARY), 'claude', 'service-admit', '--config', str(self.config_path),
             '--work', str(self.work)],
            capture_output=True, text=True, env=self.env(), timeout=120)
        record = json.loads(result.stdout) if result.stdout.strip() else {}
        (self.out / 'admission.json').write_text(json.dumps(record, indent=2, sort_keys=True))
        return result.returncode, record

    def turn(self, decision=None, brief='do the fixture task'):
        """Drive one turn against the fake, answering any permission request."""
        argv = [str(self.wrapper), *STREAM_ARGS, '--permission-mode', 'acceptEdits']
        child = subprocess.Popen(argv, cwd=str(self.fixtures), env=self.env(),
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 text=True, bufsize=1)
        sent = {'type': 'user', 'message': {'role': 'user',
                                            'content': [{'type': 'text', 'text': brief}]}}
        child.stdin.write(json.dumps(sent) + '\n')
        child.stdin.flush()
        messages = []
        for line in child.stdout:
            if not line.strip():
                continue
            message = json.loads(line)
            messages.append(message)
            if message.get('type') == 'control_request' and decision is not None:
                child.stdin.write(json.dumps({'type': 'control_response', 'response': {
                    'subtype': 'success', 'request_id': message['request_id'],
                    'response': decision}}) + '\n')
                child.stdin.flush()
        child.stdin.close()
        child.wait(timeout=30)
        (self.out / 'transcript.jsonl').write_text(
            ''.join(json.dumps(m) + '\n' for m in messages))
        return sent, messages

    def markers_of(self, event):
        path = self.markers / 'fake-claude-cli.jsonl'
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines()
                if line.strip() and json.loads(line)['event'] == event]

    def cleanup(self):
        shutil.rmtree(self.root, ignore_errors=True)


class ServiceCase(Case):
    """A case driven through `pio serve-claude` over the public Unix API, which
    is the only way journal, delivery and usage behaviour can be observed."""

    def __init__(self, out, name, **kw):
        super().__init__(out, name, **kw)
        from public_api import Client, CREDENTIAL

        class ClaudeClient(Client):
            """Negotiates the execution features this adapter actually uses;
            the base client's list does not carry workspaces or actions."""

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

        self._client = ClaudeClient
        self.store = self.root / 'store'
        self.socket = self.root / 'public.sock'
        self.transcript = self.out / 'public-transcript.jsonl'
        self.daemons = []
        self.files = []
        config = json.loads(self.config_path.read_text())
        config['format'] = 'pio-claude-service/1'
        config['protocol'] = dict(
            format='combraton-conformance-config/1', principal='owner',
            credentials=[dict(credential=CREDENTIAL)],
            executor=dict(host_id='claude-host'))
        self.config_path.write_text(json.dumps(config))

    def client(self):
        return self._client(self.socket, self.transcript)

    def start(self, expect_ready=True):
        n = len(self.daemons)
        stdout = (self.out / f'daemon-{n}.stdout').open('w')
        stderr = (self.out / f'daemon-{n}.stderr').open('w')
        self.files += [stdout, stderr]
        daemon = subprocess.Popen(
            [str(BINARY), 'serve-claude', '--data-dir', str(self.store),
             '--config', str(self.config_path), '--socket', str(self.socket)],
            stdout=stdout, stderr=stderr, env=self.env())
        self.daemons.append(daemon)
        if expect_ready:
            poll(lambda: self.client().query('core.describe', {}), lambda r: 'result' in r)
        return daemon

    def fixture_repo(self, name='repo'):
        repo = self.fixtures / name
        repo.mkdir(exist_ok=True)
        (repo / 'README.md').write_text('fixture\n')
        git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True,
                                        capture_output=True, text=True)
        git('init', '-q')
        git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
        git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
            'commit', '-q', '-m', 'fixture')
        return repo, git('rev-parse', 'HEAD').stdout.strip()

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.'):
        from public_api import command
        repo, base = self.fixture_repo(identity)
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

    def stop(self, daemon):
        daemon.kill()
        daemon.wait(timeout=10)

    def cancel(self, identity='work'):
        from public_api import command
        with self.client() as c:
            revision = c.query('execution.inspect', {'execution': identity})['result']['revision']
            return c.call(command('execution.cancel',
                                  dict(kind='execution.execution', id=identity), {},
                                  command_id=f'{identity}.cancel', revision=revision))

    def host_events(self):
        """The durable host's own append-only events file, read-only."""
        matches = sorted(self.store.glob('claude-*.events.jsonl'))
        if not matches:
            return []
        return [json.loads(line) for line in matches[0].read_text().splitlines() if line.strip()]

    def cleanup(self):
        for daemon in self.daemons:
            daemon.kill()
            daemon.wait(timeout=10)
        for handle in self.files:
            handle.close()
        super().cleanup()


def kinds(messages):
    return [m['type'] + ('/' + m['subtype'] if m.get('subtype') else '') for m in messages]


def run_case(out, name):
    """Each case asserts a named property and returns nothing; an assertion
    failure is the report."""
    if name == 'turn_completes':
        case = Case(out, name)
        sent, messages = case.turn()
        assert kinds(messages) == ['system/init', 'user', 'assistant', 'result/success'], kinds(messages)
        assert messages[-1]['is_error'] is False, messages[-1]
        assert case.markers_of('turn_complete'), 'the fake never recorded a completed turn'

    elif name == 'replay_acknowledges_delivery':
        case = Case(out, name)
        sent, messages = case.turn(brief='a brief that must come back byte for byte')
        replay = [m for m in messages if m.get('isReplay')]
        assert len(replay) == 1, kinds(messages)
        # The delivery proof is an exact echo, not a receipt the harness invented.
        assert replay[0]['message'] == sent['message'], replay[0]

    elif name == 'permission_denied':
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': {'command': 'ls /etc'}}})
        _, messages = case.turn(decision={'behavior': 'deny', 'message': 'outside the fixture'})
        assert 'control_request' in kinds(messages), kinds(messages)
        assert messages[-1]['permission_denials'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        assert recorded[0]['widening_fields_received'] == [], recorded

    elif name == 'permission_allowed':
        command = {'command': 'cat README.md'}
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': command}})
        _, messages = case.turn(decision={'behavior': 'allow', 'updatedInput': command})
        assert messages[-1]['permission_denials'] == 0, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['allow'], recorded
        # An allow must echo the input unchanged: rewriting it would alter the
        # tool call the user's harness decided to make.
        assert recorded[0]['input_echoed_unchanged'] is True, recorded

    elif name == 'out_of_fixture_request_declined_by_pio':
        # PIO classifies the request and encodes the decision. Until the service
        # binding lands the script only *transports* that decision to the fake;
        # it does not choose it. The traversal is the point: a prefix test would
        # have called this target contained.
        case = Case(out, name)
        (case.root / 'outside').mkdir(exist_ok=True)
        (case.root / 'outside' / 'secret.txt').write_text('not yours\n')
        # Written the way a prefix test gets *wrong*: an absolute path that
        # begins with the fixture and then climbs out of it. A relative
        # `../outside/...` would be declined even by the broken classifier, so
        # it would not prove anything.
        escape = f'{case.fixtures}/../outside/secret.txt'
        case.scenario = dict(case.scenario,
                             permission_request={'tool_name': 'Read',
                                                 'input': {'file_path': escape}},
                             tool_uses=[{'name': 'Read', 'input': {'file_path': escape}}])
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Read',
                               'input': {'file_path': escape},
                               'tool_use_id': 'toolu_fake_1'}}
        classification = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        assert classification['disposition'] == 'decline', classification
        assert classification['reason'] == 'target_outside_the_fixture_workspace', classification
        assert classification['target_label'] == '<outside>', classification
        assert classification['auto_allowed'] is False, classification
        encoded = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'encode-decision', '--behavior', 'deny',
             '--reason', classification['reason']],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        _, messages = case.turn(decision=encoded['envelope']['response']['response'])
        assert messages[-1]['permission_denials'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        # The decline covers the prompt. The tool use is still recorded, because
        # a target outside the fixture is an observed effect with unresolved
        # liability, not a containment claim. ADR 004 §5.
        uses = [b for m in messages if m.get('type') == 'assistant'
                for b in m['message']['content'] if b.get('type') == 'tool_use']
        assert len(uses) == 1, uses
        record = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'tool-uses', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=''.join(json.dumps(m) + '\n' for m in messages),
            capture_output=True, text=True, check=True).stdout)
        assert record['out_of_fixture_effect_observed'] is True, record
        assert record['liability'] == 'unresolved', record
        assert record['tool_uses'][0]['target_label'] == '<outside>', record
        # A receipt carries labels and digests, never raw paths.
        assert str(case.fixtures) not in json.dumps(record), record

    elif name == 'unclassifiable_request_surfaced_not_auto_allowed':
        # A shell command names no path PIO can resolve. It is surfaced to the
        # caller, never auto-allowed, and the run stops rather than guessing.
        case = Case(out, name)
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Bash',
                               'input': {'command': 'cat README.md'},
                               'tool_use_id': 'toolu_fake_1'}}
        classification = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--fixture', str(case.fixtures),
             '--cwd', str(case.fixtures)],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        assert classification['disposition'] == 'surface_as_action', classification
        assert classification['placement'] == 'not_classifiable', classification
        assert classification['auto_allowed'] is False, classification
        assert classification['target_label'] is None, classification

    elif name == 'widening_decision_never_sent':
        command = {'command': 'cat README.md'}
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': command}})
        # The harness offers a rule update in every request. The decision PIO
        # forwards is built by the adapter, which cannot encode one.
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Bash', 'input': command,
                               'permission_suggestions': [{'type': 'addRules',
                                                           'destination': 'userSettings'}]}}
        encoded = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'encode-decision', '--behavior', 'allow'],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        decision = encoded['envelope']['response']['response']
        assert encoded['suggestions_offered'] == 1 and encoded['suggestions_acted_on'] == 0, encoded
        _, messages = case.turn(decision=decision)
        recorded = case.markers_of('permission_decision')
        # The fake would have recorded a widening field had one arrived; this
        # case is only evidence because that detector is proven to work.
        assert recorded[0]['widening_fields_received'] == [], recorded
        assert recorded[0]['input_echoed_unchanged'] is True, recorded

    elif name == 'unqualified_executable_refused':
        # A real (non-fake) configuration pointed at an executable that is not
        # the qualified Claude Code.
        case = Case(out, name, labeled_fake=False)
        status, record = case.admit()
        assert status == 3, record
        reasons = [r['reason'] for r in record['refusals']]
        assert 'claude_not_qualified' in reasons, record
        assert record['stream_spawned'] is False, record

    elif name == 'missing_credential_route_refused':
        case = Case(out, name, scenario={'route': None})
        status, record = case.admit()
        assert status == 3, record
        reasons = [r['reason'] for r in record['refusals']]
        assert reasons == ['missing_credential_route'], record
        assert record['credential_route']['observed']['loggedIn'] is False, record
        assert record['stream_spawned'] is False, record

    elif name == 'permission_mode_not_the_configured_default_refused':
        for requested in ('bypassPermissions', 'plan', 'dontAsk', 'default'):
            case = Case(out, f'{name}-{requested}', permission_mode=requested)
            status, record = case.admit()
            assert status == 3, record
            reasons = [r['reason'] for r in record['refusals']]
            assert 'permission_mode_refused' in reasons, record
            assert record['permission_mode']['configured'] == 'acceptEdits', record
            assert record['stream_spawned'] is False, record
            case.cleanup()
        # The configured default itself is admitted.
        case = Case(out, name)
        status, record = case.admit()
        assert status == 0, record
        assert record['permission_mode']['allowed'] is True, record

    elif name == 'surface_drift_refused':
        # A genuine drift, not merely an unqualified executable: pin the fake's
        # own surface as the baseline, then move one help and show the refusal
        # names the command that changed.
        case = Case(out, name, scenario={'help_suffix': ' drifted'})
        baseline = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'surface-identity', '--executable', str(case.wrapper),
             '--work', str(case.work / 'baseline'), '--fake-scenario', '{}'],
            capture_output=True, text=True, check=True, env=case.env()).stdout)
        baseline_path = case.root / 'baseline-surface.json'
        baseline_path.write_text(json.dumps(baseline))

        def qualify(scenario):
            work = case.work / f'q{abs(hash(scenario))}'
            result = subprocess.run(
                [str(BINARY), 'claude', 'qualify', '--executable', str(case.wrapper),
                 '--work', str(work), '--expected', str(baseline_path),
                 '--fake-scenario', scenario],
                capture_output=True, text=True, env=case.env())
            return result.returncode, json.loads(result.stdout)

        status, unchanged = qualify('{}')
        assert status == 0 and unchanged['qualified'] is True, unchanged
        assert unchanged['surface']['drift_count'] == 0, unchanged

        status, drifted = qualify('{"help_suffix":" drifted"}')
        assert status == 3, drifted
        assert drifted['qualified'] is False, drifted
        assert drifted['refusals'] == [{'reason': 'surface_drift', 'commands': 8}], drifted
        # Every command's help moved, and each is named.
        assert {d['change'] for d in drifted['surface']['drift']} == {'changed'}, drifted
        (case.out / 'drift.json').write_text(json.dumps(drifted, indent=2))

    elif name == 'service_turn_completes':
        case = ServiceCase(out, name)
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        # The replay echo is the delivery proof, and it reached the journal as
        # a delivery with its own evidence class rather than an inference.
        assert view['delivery'] == 'acknowledged', view
        delivery = view['deliveries'][0]
        assert delivery['evidence']['class'] == 'native_replay_echo', delivery
        assert delivery['proof_class'] == 'provider_ack_id', delivery
        assert delivery['evidence']['source'] == 'pio-fake-claude-cli/host', delivery
        assert view['exit'] == {'code': 0}, view
        # Containment is recorded on every execution, not only when something
        # went wrong, and usage came from the harness's own report.
        assert view['containment'] == {
            'mechanism': 'harness_permission_rules_only',
            'os_sandbox_observed': False}, view
        assert view['usage']['liability'] == 'resolved', view['usage']
        observations = view['usage']['observations']
        assert [o['measure'] for o in observations] == ['claude.tokens.total'], observations
        assert observations[0]['amount'] > 0 and observations[0]['basis'] == 'observed', observations
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_refuses_an_unqualified_executable_at_start':
        # A real configuration pointed at something that is not the qualified
        # Claude Code: the service must refuse to start at all.
        case = ServiceCase(out, name, labeled_fake=False)
        daemon = case.start(expect_ready=False)
        assert daemon.wait(timeout=120) != 0, 'the service started on an unqualified executable'
        stderr = (case.out / 'daemon-0.stderr').read_text()
        assert 'claude_not_admitted' in stderr, stderr[:500]
        assert 'claude_not_qualified' in stderr, stderr[:500]
        admission = json.loads((case.store / 'claude-admission.json').read_text())
        assert admission['stream_spawned'] is False, admission


    elif name == 'service_answers_a_request_it_will_not_act_on':
        # PIO answers nothing on the user's behalf — but it must answer, or a
        # real harness waits forever on a request nobody will decide.
        case = ServiceCase(out, name, scenario={'foreign_control_request': 'mcp_message'})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        answered = case.markers_of('foreign_control_request')
        assert len(answered) == 1, answered
        assert answered[0]['answered'] is True, 'the harness was left waiting'
        assert answered[0]['response_subtype'] == 'error', answered
        assert 'no user is attached' in (answered[0]['error'] or ''), answered
        declined = [e for e in case.host_events() if e['kind'] == 'native_request_declined']
        assert [d['error_response_sent'] for d in declined] == [True], declined

    elif name == 'service_interrupt_escalates_when_the_signal_is_ignored':
        # A harness that ignores the interrupt must not hold the host open.
        case = ServiceCase(out, name, scenario={'ignore_interrupt': True})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['delivery'] == 'acknowledged')
        case.cancel()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        escalated = [e for e in case.host_events() if e['kind'] == 'interrupt_escalated']
        assert len(escalated) == 1, case.host_events()[-6:]
        assert escalated[0]['from'] == 'SIGINT' and escalated[0]['to'] == 'SIGKILL', escalated
        assert escalated[0]['killed'] is True, escalated
        assert escalated[0]['usage'] == 'unknown', escalated
        # A killed child sends no result, so usage is unknown, never zero.
        assert view['usage']['liability'] == 'unresolved', view['usage']
        assert view['usage'].get('observations', []) == [], view['usage']
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_restart_reattaches_without_a_duplicate_launch':
        # The conversation lives in the host that owns the child's pipes, so a
        # restarted daemon reattaches and never re-sends the brief.
        case = ServiceCase(out, name, scenario={'delay_ms': 4000})
        case.start()
        case.submit()
        before = poll(lambda: case.inspect(), lambda v: v['delivery'] == 'acknowledged')
        case.stop(case.daemons[-1])
        case.start()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        received = case.markers_of('turn_received')
        completed = case.markers_of('turn_complete')
        assert len(received) == 1, f'the brief was delivered {len(received)} times'
        assert len(completed) == 1, completed
        assert view['delivery'] == 'acknowledged' and view['exit'] == {'code': 0}, view
        assert view['host']['generation'] > before['host']['generation'], (before, view)
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    else:
        raise AssertionError(f'unknown case {name}')
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
            out = args.out / f'{name}-{repetition}'
            try:
                run_case(out, name)
                results.append((name, repetition, 'pass', None))
            except Exception as error:
                results.append((name, repetition, 'fail', repr(error)))
                failures.append((name, repetition, repr(error)))
    summary = dict(format='pio-claude-host-matrix/1', platform=platform.platform(),
                   harness='pio-fake-claude-cli', model_calls=0, live_run=False,
                   cases=len(selected), repetitions=args.repetitions,
                   attempts=len(results), failures=len(failures),
                   results=[dict(case=c, repetition=r, status=s, error=e) for c, r, s, e in results])
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    for name, repetition, error in failures:
        print(f'FAIL {name}#{repetition}: {error}')
    print(f"{len(selected)} cases x {args.repetitions} = {len(results)} attempts, "
          f"{len(failures)} failures")
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
