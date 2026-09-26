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
import case_cleanup
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
               '--verbose', '--replay-user-messages', '--permission-prompts', 'host']
CASES = [
    'turn_completes',
    'replay_acknowledges_delivery',
    'permission_denied',
    'permission_allowed',
    'out_of_fixture_request_declined_by_pio',
    'unclassifiable_request_surfaced_not_auto_allowed',
    'widening_decision_never_sent',
    'an_unattached_host_never_sees_the_request_the_harness_refuses',
    'unqualified_executable_refused',
    'missing_credential_route_refused',
    'permission_mode_not_the_configured_default_refused',
    'service_runs_under_the_product_default_when_none_is_configured',
    'service_refuses_a_stream_identity_drift_at_init',
    'service_refuses_a_permission_mode_mismatch_at_init',
    'surface_drift_refused',
    # Through the service, which is the only place these can be observed.
    'service_turn_completes',
    'service_reports_the_transcript_the_run_wrote',
    'service_records_the_configured_model_from_settings',
    'service_tells_a_harness_refusal_apart_from_a_pio_decline',
    'service_denies_a_request_nobody_answers',
    'service_records_who_decided_and_what',
    'service_refuses_an_unqualified_executable_at_start',
    'service_refuses_conformance_only_controls',
    'service_refuses_a_host_that_was_never_attached',
    'service_answers_a_request_it_will_not_act_on',
    'service_interrupt_escalates_when_the_signal_is_ignored',
    'service_reports_a_cancelled_turn_as_unknown_not_zero',
    'service_restart_reattaches_without_a_duplicate_launch',
    'service_host_lost_after_release_is_never_respawned',
    'cleanup_kills_a_harness_that_ignores_signals',
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
        # The fixtures area holds this run's workspace and could hold another
        # run's. The workspace is the boundary; its parent never is.
        self.workspace = self.fixtures / 'workspace'
        for directory in (self.fixtures, self.config_dir, self.markers, self.work,
                          self.workspace):
            directory.mkdir()
        (self.fixtures / 'README.md').write_text('PIO M3 offline fixture. No task runs here.\n')
        (self.workspace / 'README.md').write_text('PIO M3 offline workspace.\n')
        # Stands in for the user's settings: an accept-edits default, so a
        # request for anything else must be refused before a spawn.
        (self.config_dir / 'settings.json').write_text(json.dumps(
            settings if settings is not None else
            {'permissions': {'defaultMode': 'acceptEdits', 'allow': ['Bash(cat)']}}))
        self.wrapper = self.root / 'fake-claude'
        self.wrapper.write_text(f"#!/bin/sh\nexec '{BINARY}' claude fake-cli \"$@\"\n")
        self.wrapper.chmod(0o755)
        self.scenario = dict(scenario or {}, markers=str(self.markers),
                             config_dir=str(self.config_dir))
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
        self.extra_roots = []
        LIVE_CASES.append(self)

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

    def turn(self, decision=None, brief='do the fixture task', attach=True):
        """Drive one turn against the fake, answering any permission request."""
        argv = [str(self.wrapper), *STREAM_ARGS, '--permission-mode', 'acceptEdits']
        if attach:
            argv += ['--permission-prompt-tool', 'stdio']
        child = subprocess.Popen(argv, cwd=str(self.workspace), env=self.env(),
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 text=True, bufsize=1)
        sent = {'type': 'user', 'message': {'role': 'user',
                                            'content': [{'type': 'text', 'text': brief}]}}
        if attach:
            # The handshake that announces the host, before anything else.
            child.stdin.write(json.dumps({
                'type': 'control_request', 'request_id': 'req_init_pio',
                'request': {'subtype': 'initialize', 'hooks': None}}) + '\n')
            child.stdin.flush()
        child.stdin.write(json.dumps(sent) + '\n')
        child.stdin.flush()
        messages = []
        for line in child.stdout:
            if not line.strip():
                continue
            message = json.loads(line)
            if message.get('type') == 'control_response' \
                    and message.get('response', {}).get('request_id') == 'req_init_pio':
                continue
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
        if self in LIVE_CASES:
            LIVE_CASES.remove(self)
        # Kills anything still naming this store and asserts none survives,
        # so a host that refuses to exit is a failure rather than a leak.
        case_cleanup.release(self.root)
        # Anything this case made **outside** its own store, which the
        # store's release cannot reach. The out-of-fixture marker is the
        # only one, and it leaked 114 directories into `$TMPDIR` before the
        # leak check widened its roots far enough to see them:
        # `permit_prefix` says a directory *may* be removed, and nothing was
        # calling `release` on it.
        for extra in self.extra_roots:
            case_cleanup.release(extra)


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

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.',
               delivery_timeout=120):
        from public_api import command
        repo, base = self.fixture_repo(identity)
        payload = dict(brief=dict(digest=digest(brief), media_type='text/plain'),
                       workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                       timeouts=dict(delivery=delivery_timeout, execution_deadline=600))
        envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                           payload, command_id=identity)
        envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=brief.decode())}
        with self.client() as c:
            return c.call(envelope)

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

    def journal_identities(self):
        """The host and child this invocation claimed, read from the store."""
        import sqlite3
        with sqlite3.connect(f'file:{self.store}/journal.sqlite3?mode=ro', uri=True) as db:
            return [json.loads(r[0]) for r in db.execute('select state from invocations')]

    def harness_processes(self):
        """Any labeled fake still running for this case's fixture."""
        out = subprocess.check_output(['ps', '-axww', '-o', 'pid=', '-o', 'command='], text=True)
        return [line.strip() for line in out.splitlines()
                if str(self.root) in line
                and ('fake-claude' in line or 'claude host' in line)
                and 'grep' not in line]

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
        assert messages[-1]['permission_denial_count'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        assert recorded[0]['widening_fields_received'] == [], recorded

    elif name == 'permission_allowed':
        command = {'command': 'cat README.md'}
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash', 'input': command}})
        _, messages = case.turn(decision={'behavior': 'allow', 'updatedInput': command})
        assert messages[-1]['permission_denial_count'] == 0, messages[-1]
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
            [str(BINARY), 'claude', 'classify-request', '--workspace', str(case.workspace),
             '--cwd', str(case.workspace)],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        assert classification['disposition'] == 'decline', classification
        assert classification['reason'] == 'target_outside_the_fixture_workspace', classification
        assert classification['target_label'] == '<outside>', classification
        assert classification['auto_allowed'] is False, classification
        # The same escape through links, with no `..`: `self -> .`, then
        # `esc2 -> outside`. The resolver walked the rest of a path before a
        # link's target and then lost it, so since M3 this read inside the
        # fixture and would have been surfaced rather than declined (review
        # of L3, round 2, HR-1).
        os.symlink('.', Path(case.workspace) / 'self')
        os.symlink(case.root / 'outside', Path(case.workspace) / 'esc2')
        linked = dict(request, request=dict(
            request['request'], input={'file_path': f'{case.workspace}/self/esc2/secret.txt'}))
        through = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--workspace', str(case.workspace),
             '--cwd', str(case.workspace)],
            input=json.dumps(linked), capture_output=True, text=True, check=True).stdout)
        assert (through['placement'], through['disposition'], through['target_label']) == \
            ('outside_fixture', 'decline', '<outside>'), through
        encoded = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'encode-decision', '--behavior', 'deny',
             '--reason', classification['reason']],
            input=json.dumps(request), capture_output=True, text=True, check=True).stdout)
        _, messages = case.turn(decision=encoded['envelope']['response']['response'])
        assert messages[-1]['permission_denial_count'] == 1, messages[-1]
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        # The decline covers the prompt. The tool use is still recorded, because
        # a target outside the fixture is an observed effect with unresolved
        # liability, not a containment claim. ADR 004 §5.
        uses = [b for m in messages if m.get('type') == 'assistant'
                for b in m['message']['content'] if b.get('type') == 'tool_use']
        # Two: the use the harness asked about, and the one it reported having
        # made. The first was refused after the decision was transported; the
        # second was not, so it is the observed effect.
        assert len(uses) == 2, uses
        assert {u['id'] for u in uses} == {'toolu_fake_0', 'toolu_fake_ask'}, uses
        record = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'tool-uses', '--workspace', str(case.workspace),
             '--cwd', str(case.workspace)],
            input=''.join(json.dumps(m) + '\n' for m in messages),
            capture_output=True, text=True, check=True).stdout)
        assert record['out_of_fixture_effect_observed'] is True, record
        assert record['liability'] == 'unresolved', record
        assert all(u['target_label'] == '<outside>' for u in record['tool_uses']), record
        by_id = {u['tool_use_id']: u for u in record['tool_uses']}
        # Refused, so not an effect. Driven directly, so nothing can say who
        # decided; the service cases cover attribution.
        assert by_id['toolu_fake_ask']['outcome'] == 'attempted_and_denied', by_id
        assert by_id['toolu_fake_0']['outcome'] == 'performed', by_id
        # A receipt carries labels and digests, never raw paths.
        assert str(case.fixtures) not in json.dumps(record), record
        # A sibling fixture is outside too. It sits under the area fixtures are
        # created in, so a boundary drawn there would have called another run's
        # workspace contained. No traversal is needed to show it.
        sibling = case.fixtures / 'another-run' / 'notes.txt'
        sibling.parent.mkdir(exist_ok=True)
        sibling.write_text('another run\'s workspace\n')
        neighbour = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--workspace', str(case.workspace),
             '--cwd', str(case.workspace)],
            input=json.dumps({'type': 'control_request', 'request_id': 'req_2_fake',
                              'request': {'subtype': 'can_use_tool', 'tool_name': 'Read',
                                          'input': {'file_path': str(sibling)},
                                          'tool_use_id': 'toolu_fake_2'}}),
            capture_output=True, text=True, check=True).stdout)
        assert neighbour['disposition'] == 'decline', neighbour
        assert neighbour['target_label'] == '<outside>', neighbour

    elif name == 'unclassifiable_request_surfaced_not_auto_allowed':
        # A shell command names no path PIO can resolve. It is surfaced to the
        # caller, never auto-allowed, and the run stops rather than guessing.
        case = Case(out, name)
        request = {'type': 'control_request', 'request_id': 'req_1_fake',
                   'request': {'subtype': 'can_use_tool', 'tool_name': 'Bash',
                               'input': {'command': 'cat README.md'},
                               'tool_use_id': 'toolu_fake_1'}}
        classification = json.loads(subprocess.run(
            [str(BINARY), 'claude', 'classify-request', '--workspace', str(case.workspace),
             '--cwd', str(case.workspace)],
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

    elif name == 'an_unattached_host_never_sees_the_request_the_harness_refuses':
        # R3b, reproduced. Without `--permission-prompt-tool stdio` and the
        # handshake, the CLI denies anything that would prompt and answers its
        # own model. PIO is never asked and learns nothing.
        case = Case(out, name, scenario={
            'permission_request': {'tool_name': 'Bash',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        _, messages = case.turn(attach=False)
        assert 'control_request' not in kinds(messages), kinds(messages)
        refused = case.markers_of('denied_by_harness_no_host_attached')
        assert len(refused) == 1, case.markers_of('turn_received')
        assert refused[0]['attached'] is False, refused
        # The harness told its own model, in the words the real one used.
        results = [b for m in messages if m.get('type') == 'user'
                   for b in (m['message']['content'] if isinstance(m['message']['content'], list) else [])
                   if b.get('type') == 'tool_result']
        assert len(results) == 1 and results[0]['is_error'] is True, results
        assert results[0]['content'] == 'This command requires approval', results
        # And the result names the refusal, which is how a denied attempt is
        # told apart from an effect.
        assert [d['tool_name'] for d in messages[-1]['permission_denials']] == ['Bash'], messages[-1]
        # The same case with a host attached must reach PIO, or this proves
        # nothing about attachment.
        attached_case = Case(out, name + '-attached', scenario={
            'permission_request': {'tool_name': 'Bash',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        _, attached_messages = attached_case.turn(
            decision={'behavior': 'deny', 'message': 'the caller said no'})
        assert 'control_request' in kinds(attached_messages), kinds(attached_messages)
        assert attached_case.markers_of('denied_by_harness_no_host_attached') == []
        attached_case.cleanup()

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

    elif name == 'service_runs_under_the_product_default_when_none_is_configured':
        # D2. Most installations configure no `permissions.defaultMode`, and
        # admission refused every one of them. Absent is the product default,
        # measured as `default`; the flag does not accept that name, so the
        # run passes no mode flag and the `init` echo is compared with it.
        case = ServiceCase(out, name, settings={'permissions': {'allow': ['Bash(cat)']}},
                           permission_mode='default')
        status, record = case.admit()
        assert status == 0, record
        guard = record['permission_mode']
        assert guard['allowed'] is True and guard['configured'] == 'default', guard
        assert guard['configured_source'] == 'product_default', guard
        # Asking for anything else under that default is still refused.
        other = Case(out, f'{name}-acceptEdits', settings={'permissions': {}},
                     permission_mode='acceptEdits')
        status, refused = other.admit()
        assert status == 3 and refused['permission_mode']['allowed'] is False, refused
        other.cleanup()
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == {'code': 0}, view
        received = case.markers_of('turn_received')
        assert len(received) == 1 and received[0]['permission_mode_flag'] is None, received
        started = [e for e in case.host_events() if e['kind'] == 'session_started']
        assert len(started) == 1, case.host_events()
        assert started[0]['requested_permission_mode'] == 'default', started
        assert started[0]['effective_permission_mode'] == 'default', started
        assert started[0]['effective_mode_matches_requested'] is True, started

    elif name == 'service_refuses_a_stream_identity_drift_at_init':
        # D11. The pinned stream identity was enforced only by
        # re-qualification and tests; a harness whose `init` had moved ran as
        # if it had not. The fake here sends one `init` key the pinned release
        # never sent, and then asks for a permission. The host must refuse at
        # `init`, record what it saw by digest, and answer no tool use.
        case = ServiceCase(out, name, scenario={
            'init': {'key_from_a_later_release': True},
            'permission_request': {'tool_name': 'Bash', 'input': {'command': 'git tag x'}}})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        events = case.host_events()
        checked = [e for e in events if e['kind'] == 'stream_identity_checked']
        assert len(checked) == 1, [e['kind'] for e in events]
        identity = checked[0]['identity']
        assert identity['matches'] is False, identity
        assert identity['observed_sha256'] != identity['pinned_sha256'], identity
        assert identity['drift'] == [{'field': 'init_keys', 'added': ['key_from_a_later_release'],
                                      'removed': [], 'reordered': False}], identity
        refused = [e for e in events if e['kind'] == 'stream_identity_refused']
        assert len(refused) == 1, [e['kind'] for e in events]
        assert refused[0]['refusal']['reason'] == 'stream_drift', refused
        assert refused[0]['refusal']['observed_sha256'] == identity['observed_sha256'], refused
        assert refused[0]['usage'] == 'unknown', refused
        # Refused at `init`: nothing after it was acted on, and the harness
        # never got an answer to anything.
        # Not `kinds`: that name is this module's message helper, and binding
        # it here would make it local to every case.
        order = [e['kind'] for e in events]
        assert order.index('stream_identity_checked') < order.index('stream_identity_refused'), order
        for later in ('action_requested', 'request_declined_by_pio', 'control_applied',
                      'request_denied_by_default', 'turn_completed', 'host_error'):
            assert later not in order, order
        assert case.markers_of('permission_decision') == [], case.markers_of('permission_decision')
        # What the caller sees: the echo proved delivery, the run ended with no
        # exit code claimed, and usage is unknown rather than none.
        assert view['delivery'] == 'acknowledged', view
        assert view['exit'] == 'unavailable', view
        assert view['usage']['liability'] == 'unresolved', view['usage']
        assert view['usage']['observations'] == [], view['usage']
        # And why, on the exit: refused after delivery, usage unknown.
        with case.client() as c:
            stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                  'kinds': ['execution.execution']})['result']
        exits = [i['event']['payload'] for i in stream['items']
                 if 'event' in i and i['event']['type'] == 'execution.exit.observed']
        assert [(x['pio.combraton.dev/refusal']['refusal']['reason'],
                 x['pio.combraton.dev/refusal']['usage']) for x in exits] == \
            [('stream_drift', 'unknown')], exits
        # The child is stopped, not left running.
        poll(lambda: case.harness_processes(), lambda p: not p, seconds=60)
        (case.out / 'events.json').write_text(json.dumps(events, indent=2))
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_refuses_a_permission_mode_mismatch_at_init':
        # The harness reports a permission mode other than the one PIO asked
        # for. `init` arrives only after the brief, so the brief was
        # delivered. This failed through `host_error`, and the caller saw a
        # lost host: delivery ambiguous, runtime unknown, usage liability
        # `none`. It is refused the way D11 refuses a drift: the echo alone
        # is read, the child stopped before any tool use is answered, and the
        # turn ends with usage unresolved and the reason on the exit.
        case = ServiceCase(out, name, scenario={
            'init': {'permissionMode': 'bypassPermissions'},
            'permission_request': {'tool_name': 'Bash', 'input': {'command': 'git tag x'}}})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] in ('exited', 'unknown'),
                    seconds=120)
        events = case.host_events()
        order = [e['kind'] for e in events]
        refused = [e for e in events if e['kind'] == 'permission_mode_mismatch_refused']
        assert len(refused) == 1, order
        assert refused[0]['refusal'] == {'reason': 'effective_permission_mode_mismatch',
                                         'requested_permission_mode': 'acceptEdits',
                                         'effective_permission_mode': 'bypassPermissions'}, refused
        assert refused[0]['usage'] == 'unknown' and refused[0]['killed'] is True, refused
        for later in ('host_error', 'action_requested', 'request_declined_by_pio',
                      'control_applied', 'request_denied_by_default', 'turn_completed'):
            assert later not in order, order
        assert case.markers_of('permission_decision') == [], case.markers_of('permission_decision')
        # Delivered and stopped, not lost: the echo proved delivery, the run
        # exited with no exit code claimed, and usage is unknown, not none.
        assert (view['delivery'], view['runtime'], view['exit']) == \
            ('acknowledged', 'exited', 'unavailable'), view
        assert view['usage'] == {'observations': [], 'liability': 'unresolved'}, view['usage']
        with case.client() as c:
            stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                  'kinds': ['execution.execution']})['result']
        exits = [i['event']['payload'] for i in stream['items']
                 if 'event' in i and i['event']['type'] == 'execution.exit.observed']
        assert [x['pio.combraton.dev/refusal']['refusal']['reason'] for x in exits] == \
            ['effective_permission_mode_mismatch'], exits
        assert exits[0]['pio.combraton.dev/refusal']['usage'] == 'unknown', exits
        poll(lambda: case.harness_processes(), lambda p: not p, seconds=60)
        (case.out / 'events.json').write_text(json.dumps(events, indent=2))
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

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
        # The replay echo is the delivery evidence, and it reached the journal as
        # a delivery with its own evidence class rather than an inference.
        assert view['delivery'] == 'acknowledged', view
        delivery = view['deliveries'][0]
        assert delivery['evidence']['class'] == 'native_replay_echo', delivery
        # The echo returns no identifier, so it earns no proof class (D7).
        assert 'proof_class' not in delivery, delivery
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

    elif name == 'service_reports_the_transcript_the_run_wrote':
        # The harness writes a session file and a `memory` directory under a
        # slug of the session's working directory. R1 snapshotted the area
        # fixtures are created in instead, which names a directory the harness
        # never writes to, so the receipt reported that nothing had been
        # written while 194 KB sat on disk. The fake writes the same shape, so
        # this case fails if the snapshot slugs anything but the workspace.
        case = ServiceCase(out, name)
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == {'code': 0}, view
        before = [e for e in case.host_events() if e['kind'] == 'config_before']
        after = [e for e in case.host_events() if e['kind'] == 'config_after']
        assert len(before) == 1 and len(after) == 1, case.host_events()
        # Nothing before the run, and both entries after: the session file and
        # the nested directory beside it, which a non-recursive listing misses.
        assert before[0]['snapshot']['transcripts']['entry_count'] == 0, before[0]
        assert after[0]['snapshot']['transcripts']['exists'] is True, after[0]
        assert after[0]['snapshot']['transcripts']['entry_count'] == 2, after[0]
        diff = after[0]['diff']
        assert diff['new_transcript_entries'] == 2, diff
        # PIO still changed nothing of the user's own.
        assert diff['settings_changed'] is False, diff
        # The directory the harness actually used, named here and nowhere in
        # the receipt, which carries digests only.
        slug = str(case.fixtures / 'work').replace('/', '-').replace('.', '-')
        written = case.config_dir / 'projects' / slug
        assert written.is_dir(), f'the fake wrote no transcript at {written}'
        assert (written / 'memory').is_dir(), sorted(p.name for p in written.iterdir())
        assert slug not in json.dumps(after[0]), 'the snapshot named a path'

    elif name == 'service_records_the_configured_model_from_settings':
        # The model the user configured is read from their settings. R1 took it
        # from a service-configuration field that the admission list does not
        # allow, so it could only ever be absent, and the receipt said the
        # configuration named no model while it named one.
        case = ServiceCase(out, name, settings={
            'model': 'a-configured-model-name',
            'permissions': {'defaultMode': 'acceptEdits', 'allow': ['Bash(cat)']}})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        started = [e for e in case.host_events() if e['kind'] == 'session_started']
        assert len(started) == 1, case.host_events()
        assert started[0]['configured_model'] == 'a-configured-model-name', started[0]
        # Nothing was passed, so the two are distinguishable in the receipt.
        assert started[0]['requested_model'] is None, started[0]

    elif name == 'service_tells_a_harness_refusal_apart_from_a_pio_decline':
        # A host is attached and the harness refuses anyway, because its own
        # rules shadow the callback — which the pinned SDK warns about and R3
        # measured. A caller must be able to tell that apart from PIO
        # declining, so it is in the view rather than only in the events.
        case = ServiceCase(out, name, scenario={
            'deny_by_rules': True,
            'permission_request': {'tool_name': 'Bash',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
        assert view['exit'] == {'code': 0}, view
        events = case.host_events()
        # PIO declined nothing and was asked nothing.
        assert [e for e in events if e['kind'] == 'request_declined_by_pio'] == []
        assert [e for e in events if e['kind'] == 'action_requested'] == []
        assert [e for e in events if e['kind'] == 'request_denied_by_default'] == []
        # The harness was attached, and refused under its own rules anyway.
        assert case.markers_of('denied_by_harness_rules_shadowed_the_host'), case.markers_of('turn_received')
        assert [e for e in events if e['kind'] == 'host_attached'][0]['outcome']['attached'] is True
        # Visible to a caller, in the view, named for what it was.
        assert view['containment']['denied_by_harness'] == 1, view['containment']
        assert 'PIO was not asked' in view['containment']['denied_by_harness_reason'], view['containment']
        # A refused attempt is not an effect.
        record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
        assert record['denied_by_harness_count'] == 1, record
        assert record['tool_uses'][0]['outcome'] == 'attempted_and_denied', record
        assert record['liability'] == 'none_observed', record
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_denies_a_request_nobody_answers':
        # PIO decides nothing on the user's behalf except this. A request the
        # caller never answers holds the harness open forever and leaves the
        # execution unfinishable, so the default is a **single-use deny**,
        # recorded as PIO's own. The wait is the caller's declared delivery
        # timeout, not a number the host picked.
        case = ServiceCase(out, name, scenario={
            'permission_request': {'tool_name': 'Bash',
                                   'input': {'command': 'git tag pio-live-marker'}}})
        case.start()
        # Five seconds, so the case does not sit for the default two minutes.
        case.submit(delivery_timeout=5)
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        events = case.host_events()
        # The request was surfaced to the caller, who answered nothing.
        assert len([e for e in events if e['kind'] == 'action_requested']) == 1, events[-6:]
        assert [e for e in events if e['kind'] == 'control_applied'] == []
        defaulted = [e for e in events if e['kind'] == 'request_denied_by_default']
        assert len(defaulted) == 1, [e['kind'] for e in events]
        assert defaulted[0]['after_seconds'] == 5, defaulted
        # What was decided and by whom, and that it matches what the harness
        # was actually sent. Recording one decision while sending another is a
        # receipt that describes a run that did not happen.
        assert defaulted[0]['decision'] == 'deny', defaulted
        assert defaulted[0]['decided_by'] == 'pio', defaulted
        received = case.markers_of('permission_decision')
        assert [r['behavior'] for r in received] == [defaulted[0]['decision']], (
            defaulted, received)
        # A single-use deny, and nothing that widens a permission.
        assert defaulted[0]['widening_fields_sent'] == [], defaulted
        assert defaulted[0]['suggestions_acted_on'] == 0, defaulted
        assert defaulted[0]['suggestions_offered'] == 1, defaulted
        # The harness took it and the turn ended rather than hanging.
        assert view['exit'] == {'code': 0}, view
        recorded = case.markers_of('permission_decision')
        assert [r['behavior'] for r in recorded] == ['deny'], recorded
        assert recorded[0]['widening_fields_received'] == [], recorded
        # Attribution is not asserted here: the fake's permission-request path
        # emits the control request without an accompanying assistant tool-use
        # block, so there is no tool use to attribute. The real harness sends
        # both, and `a_refusal_is_attributed_to_whoever_decided_it` covers the
        # PIO-decided case directly.
        record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
        use = {u['tool_use_id']: u for u in record['tool_uses']}[
            defaulted[0]['tool_use_id']]
        assert use['decided_by'] == 'pio', use
        assert use['decision'] == 'deny', use
        assert use['outcome'] == 'declined_by_pio', use
        assert use['denied_by_harness'] is False, use
        assert record['declined_by_pio_count'] == 1, record
        assert record['denied_by_caller_count'] == 0, record
        assert record['denied_by_harness_count'] == 0, record
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'service_records_who_decided_and_what':
        # The caller's own decision, through `execution.respond_action`, deny
        # and allow. Until R3c and R4c this path existed only in the live
        # runner, so a recorded decision that disagreed with the sent one
        # would have shown up first in a live receipt.
        for decision in ('deny', 'allow', 'pio-declines'):
            # The last one targets a path outside the workspace, which PIO
            # declines itself before any caller is asked.
            outside = str(Path(tempfile.mkdtemp(prefix='pio-outside-')) / 'marker.txt')
            case_cleanup.permit_prefix(Path(outside).parent)
            Path(outside).write_text('outside the workspace\n')
            request = ({'tool_name': 'Read', 'input': {'file_path': outside}}
                       if decision == 'pio-declines'
                       else {'tool_name': 'Bash',
                             'input': {'command': 'git tag pio-live-marker'}})
            case = ServiceCase(out, f'{name}-{decision}',
                               scenario={'permission_request': request})
            # The marker is deliberately outside the fixture, so the store's
            # own release cannot reach it. `permit_prefix` alone says it *may*
            # be removed and removes nothing; this is what removes it. Without
            # it every attempt left one directory in `$TMPDIR` for ever.
            case.extra_roots.append(Path(outside).parent)
            case.start()
            case.submit()
            if decision == 'pio-declines':
                view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited')
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
                assert record['denied_by_caller_count'] == 0, record
                assert record['denied_by_harness_count'] == 0, record
                continue
            view = poll(lambda: case.inspect(),
                        lambda v: v['runtime'] in ('requires_action', 'exited'), seconds=120)
            assert view['runtime'] == 'requires_action', view
            action = view['runtime_detail']['action_id']
            answered = case.respond(action, decision, view['revision'])
            assert answered.get('result', {}).get('outcome', {}).get('state') == 'answered', answered
            view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
            assert view['exit'] == {'code': 0}, view
            events = case.host_events()
            applied = [e for e in events if e['kind'] == 'control_applied']
            assert len(applied) == 1, [e['kind'] for e in events]
            # Recorded: what, and by whom.
            assert applied[0]['decision'] == decision, applied
            assert applied[0]['decided_by'] == 'caller', applied
            # Sent: the same thing, as the harness itself saw it.
            received = case.markers_of('permission_decision')
            assert [r['behavior'] for r in received] == [decision], (applied, received)
            assert received[0]['widening_fields_received'] == [], received
            if decision == 'allow':
                # An allow echoes the original input; rewriting it would change
                # the tool call the harness decided to make.
                assert received[0]['input_echoed_unchanged'] is True, received
            # And the record that reaches the receipt says the same, by id.
            record = [e for e in events if e['kind'] == 'tool_uses'][0]['record']
            use = {u['tool_use_id']: u for u in record['tool_uses']}[
                applied[0]['tool_use_id']]
            assert use['decided_by'] == 'caller', use
            assert use['decision'] == decision, use
            assert use['outcome'] == (
                'denied_by_caller' if decision == 'deny' else 'performed'), use
            assert record['denied_by_caller_count'] == (1 if decision == 'deny' else 0), record
            assert record['declined_by_pio_count'] == 0, record
            assert record['denied_by_harness_count'] == 0, record
            # PIO decided nothing here, and nothing defaulted.
            assert [e for e in events if e['kind'] == 'request_declined_by_pio'] == []
            assert [e for e in events if e['kind'] == 'request_denied_by_default'] == []
            assert applied[0]['widening_fields_sent'] == [], applied
            assert applied[0]['suggestions_acted_on'] == 0, applied
            # No explicit cleanup: every case registers itself and the runner
            # releases them all, so a failed assertion cannot leak a daemon.

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

    elif name == 'service_refuses_conformance_only_controls':
        # D9: a launch control that exists only for the conformance
        # participant (here, fault injection into every response) must
        # never start a product service, whatever the config says.
        case = ServiceCase(out, name)
        config = json.loads(case.config_path.read_text())
        config['protocol']['faults'] = {
            'response_internal_error': [{'operation': 'execution.submit', 'times': 1}]}
        case.config_path.write_text(json.dumps(config))
        daemon = case.start(expect_ready=False)
        assert daemon.wait(timeout=120) != 0, \
            'the service started with a conformance-only control'
        stderr = (case.out / 'daemon-0.stderr').read_text()
        assert 'conformance-only' in stderr, stderr[:500]


    elif name == 'service_refuses_a_host_that_was_never_attached':
        # A pre-delivery refusal through the service, which this matrix did not
        # have. The CLI receives the permission-prompt handshake and never
        # answers it, so PIO is not the permission host — the exact shape of
        # the defect that cost four live Claude runs. It must refuse **before**
        # the brief leaves, and the refusal must be a **finished** execution:
        # the fix for that is in the shared projection, and before it a caller
        # polling the runtime could not tell a refusal from a slow start.
        case = ServiceCase(out, name, scenario={'ignore_initialize': True})
        case.start()
        case.submit()
        view = poll(lambda: case.inspect(),
                    lambda v: v['delivery'] in ('failed_before_delivery', 'not_delivered'),
                    seconds=120)
        assert view['delivery'] == 'failed_before_delivery', view
        events = case.host_events()
        attached = [e for e in events if e['kind'] == 'host_attached']
        assert len(attached) == 1, [e['kind'] for e in events]
        assert attached[0]['outcome']['attached'] is False, attached
        assert attached[0]['sent_before_delivery'] is True, attached
        error = [e for e in events if e['kind'] == 'host_error']
        assert error and 'host_not_attached' in error[0]['error'], error
        # The brief never left PIO, and the CLI was never asked for a turn.
        assert [e for e in events if e['kind'] == 'turn_start_sent'] == [], \
            'the brief was sent after a refusal'
        assert len(case.markers_of('initialize_ignored')) == 1, case.markers_of('initialize_ignored')
        assert case.markers_of('turn_received') == [], case.markers_of('turn_received')
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        assert view['exit'] == 'unavailable', view
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

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

    elif name == 'service_reports_a_cancelled_turn_as_unknown_not_zero':
        # Measured on the live R5 cancel: the harness does answer SIGINT with a
        # `result`, but it carries `terminal_reason: aborted_streaming`, an
        # empty `iterations` and every usage part zero. PIO passed that on as
        # `basis: observed, amount: 0, liability: resolved` — it told the
        # protocol a cancelled turn provably cost nothing, and the ledger
        # counted zero for a turn that had spent a session's prefix.
        case = ServiceCase(out, name, scenario={'delay_ms': 30000,
                                                'abort_on_interrupt': True})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['delivery'] == 'acknowledged')
        case.cancel()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'exited', seconds=120)
        events = case.host_events()
        assert case.markers_of('aborted_on_interrupt'), 'the fake never took the signal'
        completed = [e for e in events if e['kind'] == 'turn_completed']
        assert len(completed) == 1, events[-6:]
        # The result did arrive. That is the point: this is not the killed-child
        # case, and an empty usage block is not the same as no result.
        assert completed[0]['terminal_reason'] == 'aborted_streaming', completed
        assert completed[0]['status'] == 'failed', completed
        unknown = [e for e in events if e['kind'] == 'usage_unknown']
        assert len(unknown) == 1, [e['kind'] for e in events]
        assert unknown[0]['reason'] == 'the harness reported an empty usage block', unknown
        assert [e for e in events if e['kind'] == 'usage'] == [], 'usage was reported as zero'
        # And nothing reached the protocol as a measurement.
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

    elif name == 'service_host_lost_after_release_is_never_respawned':
        # The brief was delivered and then the host died. PIO must not decide
        # the turn's fate, and must never re-send: it reports the runtime as
        # unknown and leaves it there.
        case = ServiceCase(out, name, scenario={'delay_ms': 60000})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['delivery'] == 'acknowledged')
        state = case.journal_identities()[0]
        case.stop(case.daemons[-1])
        for identity in (state['host'], state['child']):
            if identity and identity.get('pid'):
                try:
                    os.kill(identity['pid'], 9)
                except ProcessLookupError:
                    pass
        for line in case.harness_processes():
            try:
                os.kill(int(line.split()[0]), 9)
            except (ProcessLookupError, ValueError):
                pass
        time.sleep(0.5)
        case.start()
        view = poll(lambda: case.inspect(), lambda v: v['runtime'] == 'unknown', seconds=60)
        time.sleep(1)
        # Delivery stands: the replay echo already proved it. Nothing is
        # re-sent, and no second harness is started.
        assert view['delivery'] == 'acknowledged', view
        assert view['exit'] == 'unavailable', view
        assert len(case.markers_of('turn_received')) == 1, case.markers_of('turn_received')
        assert case.harness_processes() == [], case.harness_processes()
        (case.out / 'view.json').write_text(json.dumps(view, indent=2))

    elif name == 'cleanup_kills_a_harness_that_ignores_signals':
        # The harness is spawned with the arguments a real Claude Code would
        # get, so its command line names no store. Killing only what names the
        # store left it running; the host leads its own session, so the process
        # group is what must go.
        case = ServiceCase(out, name, scenario={'ignore_interrupt': True})
        case.start()
        case.submit()
        poll(lambda: case.inspect(), lambda v: v['delivery'] == 'acknowledged')
        harness = poll(lambda: [p for p in case_cleanup.processes_under(case.root)
                                if 'fake-cli' in p[2]], bool, seconds=60)
        host = [p for p in case_cleanup.processes_under(case.root) if 'claude host' in p[2]]
        assert host and harness, (host, harness)
        # Killed from outside, the way a crash would, not through cancel.
        for pid, _, _ in host:
            os.kill(pid, 9)
        time.sleep(0.5)
        still = [p for p in case_cleanup.processes_under(case.root) if 'fake-cli' in p[2]]
        assert still, 'the harness was expected to outlive the killed host'
        # Cleanup must now take the whole group, not just what names the store.
        case_cleanup.release(case.root, remove=False)
        assert case_cleanup.processes_under(case.root) == [], \
            case_cleanup.processes_under(case.root)
        assert case_cleanup.groups_under(case.root) == set(), \
            case_cleanup.groups_under(case.root)
        for daemon in case.daemons:
            daemon.kill()
            daemon.wait(timeout=10)
        case.daemons.clear()

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
            finally:
                release_live_cases()
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
