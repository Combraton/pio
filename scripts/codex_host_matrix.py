#!/usr/bin/env python3
"""Offline Codex adapter matrix: `pio serve-codex` over the public Unix API
driving the labeled fake app-server (never a real Codex, never a live run).

Independent witnesses: the fake app-server's own marker file (spawn, received
turn input digest, approval answers), the durable host's journal and events
file read-only, and the OS process table. Every execution is labeled
`pio-fake-app-server`.
"""
import argparse
from collections import Counter
import sys
import hashlib
import json
import os
from pathlib import Path
import platform
import sqlite3
import subprocess
import tempfile
import time
import traceback
import uuid
import case_cleanup
from public_api import Client, CREDENTIAL, command

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery', 'execution.workspaces', 'execution.usage', 'execution.actions', 'execution.steering']
CASES = ['j1_turn_completes', 'approvals_reviewer_must_be_user', 'approvals_reviewer_absent_refused', 'approval_decline', 'approval_accept', 'interrupt_cancels_turn', 'steer_acknowledged', 'suppressed_ack_negative_control',
         'missing_content_refused', 'content_digest_mismatch', 'outside_fixture_refused', 'unqualified_executable_refused', 'restart_reattach_no_duplicate', 'host_lost_no_respawn', 'discovery_reports_observed_authentication',
         'widening_decisions_refused', 'deadline_stop_interrupts', 'thread_settings_broader_refused', 'permission_grant_refused',
         'lead_tool_on_the_thread', 'mcp_approval_to_the_caller', 'mcp_approval_lapses', 'other_elicitation_declined',
         'model_checked_before_turn', 'model_mismatch_refused', 'provider_mismatch_refused']
LEAD_TOOL = 'pio.combraton.dev/lead-tool'
# Owner decision for L3, 2026-09-25: the model is asked for under the dated
# exception, and the provider is checked from Codex's answer, never sent.
MODEL_CASES = {'model_checked_before_turn', 'model_mismatch_refused', 'provider_mismatch_refused'}
WITNESS = '''import json, os, sys
log = open(os.environ['PIO_WITNESS_LOG'], 'a')
def note(record):
    log.write(json.dumps(record) + '\\n')
    log.flush()
note({'event': 'started'})
for line in sys.stdin:
    message = json.loads(line)
    note({'event': 'request', 'method': message.get('method'),
          'name': (message.get('params') or {}).get('name')})
    if 'id' not in message:
        continue
    method = message.get('method')
    if method == 'initialize':
        result = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}},
                  'serverInfo': {'name': 'witness', 'version': '0'}}
    elif method == 'tools/list':
        result = {'tools': [{'name': 'start_run', 'inputSchema': {'type': 'object'}},
                            {'name': 'read_run', 'inputSchema': {'type': 'object'}}]}
    elif method == 'tools/call':
        result = {'content': [{'type': 'text', 'text': json.dumps(
            {'called': message['params']['name']})}]}
    else:
        result = {}
    sys.stdout.write(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}) + '\\n')
    sys.stdout.flush()
'''


def witness(case, pre_allowed=False):
    """An MCP server that writes down every request it receives: the only
    witness that a harness launched it. The lead tool's own spec shape."""
    script = case.root / 'witness.py'
    script.write_text(WITNESS)
    log = case.root / 'witness.jsonl'
    spec = dict(name='pio-lead', command=sys.executable, args=[str(script)],
                env=[dict(name='PIO_WITNESS_LOG', value=str(log))])
    if pre_allowed:
        spec['pre_allowed_tools'] = ['start_run', 'read_run']
    return spec, log


def witnessed(log):
    return [json.loads(l) for l in log.read_text().splitlines()] if log.exists() else []


def poll(action, predicate, seconds=20):
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
    raise AssertionError(f'bounded observation timed out; last={json.dumps(last)[:2000] if last is not None else None}')


def digest(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


class CodexClient(Client):
    def __init__(self, path, transcript=None, timeout=10):
        self.features = FEATURES
        super().__init__(path, transcript, timeout)

    def query(self, operation, payload):
        if operation == 'core.negotiate':
            payload = dict(payload, profiles=[dict(name='core', majors=[1], required=True, required_features=['core.events', 'core.capabilities', 'core.effects'], optional_features=[]),
                                             dict(name='execution', majors=[1], required=True, required_features=FEATURES, optional_features=[])])
        return super().query(operation, payload)


class Case:
    def __init__(self, out, name, scenario=None, fake=True):
        self.name = name
        self.out = out / name
        self.out.mkdir(parents=True)
        self.root = Path(tempfile.mkdtemp(prefix='pio-cx-', dir='/tmp')).resolve()
        os.chmod(self.root, 0o700)
        self.store = self.root / 'store'
        self.fixtures = self.root / 'fixtures'
        self.codex_home = self.root / 'codex-home'
        self.markers = self.root / 'markers'
        for d in (self.fixtures, self.codex_home, self.markers):
            d.mkdir()
        self.transcript = self.out / 'public-transcript.jsonl'
        self.daemons = []
        self.files = []
        self.extra_roots = []
        wrapper = self.root / 'fake-codex'
        wrapper.write_text(f"#!/bin/sh\nexec '{BINARY}' codex fake-app-server \"$@\"\n")
        wrapper.chmod(0o755)
        scenario = dict(scenario or {}, markers=str(self.markers))
        env = {'PATH': '/usr/bin:/bin', 'HOME': str(self.root), 'CODEX_HOME': str(self.codex_home), 'PIO_CODEX_FAKE_SCENARIO': json.dumps(scenario)}
        protocol = dict(format='combraton-conformance-config/1', principal='owner', credentials=[dict(credential=CREDENTIAL)], executor=dict(host_id='codex-host'))
        self.config = dict(format='pio-codex-service/1', protocol=protocol,
                           codex=dict(executable=str(wrapper), env=env, codex_home=str(self.codex_home), fixture_root=str(self.fixtures),
                                      thread=dict(sandbox='workspace-write', approvalPolicy='on-request'), labeled_fake=fake))
        self.config_path = self.root / 'service.json'
        self.config_path.write_text(json.dumps(self.config))

    def argv(self):
        return [str(BINARY), 'serve-codex', '--data-dir', str(self.store), '--config', str(self.config_path), '--socket', str(self.root / 'public.sock')]

    def start(self, expect_ready=True):
        n = len(self.daemons)
        stdout = (self.out / f'daemon-{n}.stdout').open('w')
        stderr = (self.out / f'daemon-{n}.stderr').open('w')
        self.files += [stdout, stderr]
        daemon = subprocess.Popen(self.argv(), stdout=stdout, stderr=stderr)
        self.daemons.append(daemon)
        if expect_ready:
            poll(lambda: CodexClient(self.root / 'public.sock', self.transcript).query('core.describe', {}), lambda r: 'result' in r)
        return daemon

    def stop(self, daemon):
        daemon.kill()
        daemon.wait(timeout=5)

    def client(self):
        return CodexClient(self.root / 'public.sock', self.transcript)

    def fixture(self, name='repo'):
        repo = self.fixtures / name
        repo.mkdir()
        (repo / 'README.md').write_text('fixture\n')
        run = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True, capture_output=True, text=True)
        run('init', '-q')
        run('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
        run('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'commit', '-q', '-m', 'fixture')
        return repo, run('rev-parse', 'HEAD').stdout.strip()

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.', content=True, repository=None, tamper=False, deadline=600, delivery=120, extensions=None):
        repo, base = self.fixture(identity) if repository is None else (repository, 'unused')
        # Live submits carry a 2 minute delivery timeout and 10 minute deadline.
        # The delivery timeout is also how long an approval may wait for its
        # caller before PIO declines it once (owner decision, 2026-09-25).
        payload = dict(brief=dict(digest=digest(brief), media_type='text/plain'),
                       workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                       timeouts=dict(delivery=delivery, execution_deadline=deadline))
        envelope = command('execution.submit', dict(kind='execution.execution', id=identity), payload, command_id=identity)
        if content:
            text = brief.decode() + (' tampered' if tamper else '')
            envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=text)}
        if extensions:
            envelope['extensions'] = dict(envelope.get('extensions') or {}, **extensions)
        with self.client() as c:
            return c.call(envelope), repo

    def configure(self, **codex):
        """Change the service's codex settings before it starts."""
        self.config['codex'].update(codex)
        self.config_path.write_text(json.dumps(self.config))

    def execution_command(self, operation, identity, payload, command_id, content_bytes=None, media_type=None):
        with self.client() as c:
            revision = c.query('execution.inspect', {'execution': identity})['result']['revision']
            envelope = command(operation, dict(kind='execution.execution', id=identity), payload, command_id=command_id, revision=revision)
            if content_bytes is not None:
                envelope['extensions'] = {CONTENT: dict(media_type=media_type, text=content_bytes.decode())}
            return c.call(envelope)

    def inspect(self, identity='work'):
        with self.client() as c:
            return c.query('execution.inspect', {'execution': identity})

    def markers_records(self):
        path = self.markers / 'fake-app-server.jsonl'
        return [json.loads(l) for l in path.read_text().splitlines()] if path.exists() else []

    def journal(self):
        with sqlite3.connect(f'file:{self.store}/journal.sqlite3?mode=ro', uri=True) as db:
            return [json.loads(r[0]) for r in db.execute('select record from journal order by sequence')], \
                   [json.loads(r[0]) for r in db.execute('select state from invocations')]

    def app_server_processes(self):
        table = subprocess.check_output(['ps', '-axww', '-o', 'pid=', '-o', 'command='], text=True)
        return [l for l in table.splitlines() if str(self.root) in l and ('fake-app-server' in l or 'codex host' in l)]

    def close(self):
        try:
            if self.transcript.exists():
                check = subprocess.run([str(BINARY), 'check-transcript', str(self.transcript)], capture_output=True, text=True)
                (self.out / 'schema-validation.txt').write_text(check.stdout + check.stderr)
                assert check.returncode == 0, check.stderr
            if (self.store / 'journal.sqlite3').exists():
                journal, invocations = self.journal()
                (self.out / 'journal.json').write_text(json.dumps(journal, indent=2) + '\n')
                (self.out / 'invocations.json').write_text(json.dumps(invocations, indent=2) + '\n')
            for f in list(self.store.glob('codex-*.events.jsonl')) + list(self.store.glob('codex-*.controls.jsonl')) + [self.markers / 'fake-app-server.jsonl']:
                if f.exists():
                    (self.out / f.name).write_bytes(f.read_bytes())
        finally:
            for line in self.app_server_processes():
                try:
                    os.kill(int(line.split()[0]), 9)
                except (ProcessLookupError, ValueError):
                    pass
            for d in self.daemons:
                if d.poll() is None:
                    self.stop(d)
            for f in self.files:
                f.close()
            # Kill anything still naming this store, assert none survives, and
            # remove it. A retained store per attempt is a leak; a store removed
            # under a live host is worse.
            case_cleanup.release(self.root)
            for extra in self.extra_roots:
                case_cleanup.release(extra)


def exited(case, identity='work', seconds=30):
    return poll(lambda: case.inspect(identity)['result'], lambda v: v['runtime'] == 'exited', seconds)


def events_of(case, kind):
    records = []
    for f in case.store.glob('codex-*.events.jsonl'):
        records += [json.loads(l) for l in f.read_text().splitlines() if json.loads(l)['kind'] == kind]
    return records


NATIVE = 'pio.combraton.dev/native-declines'


def answered_once(case):
    """What the fake received, by request id, and that no request was
    answered twice: the wire, not PIO's own record of it (review of L3,
    CH-3)."""
    markers = case.markers_records()
    twice = [m for m in markers if m['kind'] == 'second_response']
    assert twice == [], twice
    return [m for m in markers if m['kind'] == 'response_received']


def carried_declines(case, identity='work'):
    """The run's native declines as a caller reads them: off the stream, on
    the run's own exit event, and nowhere else (review of L3, CH-2/F1)."""
    with case.client() as c:
        result = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                              'kinds': ['execution.execution']})['result']
    exits = [i['event'] for i in result['items'] if 'event' in i
             and i['event']['type'] == 'execution.exit.observed'
             and i['event']['subject']['id'] == identity]
    assert len(exits) == 1, exits
    assert NATIVE in exits[0]['payload'], exits[0]
    return exits[0]['payload'][NATIVE]


def model_case(case, name):
    """The thread's model and provider, from Codex's own answer, before its
    first turn: a mismatch in either ends the run with no turn sent."""
    response, _ = case.submit()
    assert response['result']['outcome']['admission'] == 'admitted', response
    final = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'exited', 60)
    checked = events_of(case, 'model_checked')
    assert len(checked) == 1, checked
    checked = checked[0]
    assert checked['requested_model'] == 'gpt-5.6-terra' and \
        checked['expected_model_provider'] == 'openai', checked
    received = [m for m in case.markers_records() if m['kind'] == 'turn_received']
    if name == 'model_checked_before_turn':
        assert checked['matches'] is True and checked['model'] == 'gpt-5.6-terra' and \
            checked['model_provider'] == 'openai', checked
        order = [json.loads(l)['kind'] for f in case.store.glob('codex-*.events.jsonl')
                 for l in f.read_text().splitlines()]
        assert order.index('model_checked') < order.index('turn_start_sent'), order
        assert len(received) == 1 and final['delivery'] == 'acknowledged', final
        return dict(outcome='pass', checked=checked, before_turn=True)
    assert checked['matches'] is False, checked
    assert received == [], 'a turn reached Codex on a thread whose model did not match'
    invocations = poll(lambda: case.journal()[1],
                       lambda i: i and i[0]['phase'] == 'known_not_released')
    assert 'thread_model_mismatch' in str(invocations[0]['receipt']['reason']), invocations[0]
    return dict(outcome='pass', checked=checked, turns_sent=0, delivery=final['delivery'])


def lead_tool_case(case, name):
    """The lead tool on its own thread, and what Codex asks before calling
    it: a single `accept`, a `decline`, or one decline of PIO's when nobody
    answers. Never `persist`."""
    spec, log = witness(case, pre_allowed=name == 'lead_tool_on_the_thread')
    response, _ = case.submit(extensions={LEAD_TOOL: spec},
                              delivery=2 if name == 'mcp_approval_lapses' else 120)
    assert response['result']['outcome']['admission'] == 'admitted', response
    if name == 'lead_tool_on_the_thread':
        exited(case)
        # A second run, with no tool, gets none.
        other, _ = case.submit(identity='plain')
        assert other['result']['outcome']['admission'] == 'admitted', other
        exited(case, 'plain')
        sent = sorted((e['names'], e['pre_allowed_tools']) for e in events_of(case, 'mcp_servers_sent'))
        assert sent == [([], None), (['pio-lead'], ['read_run', 'start_run'])], sent
        # Exactly the lead's two tools, and no server-wide default, as the
        # host read them back from the request it sent, and as the fake
        # received them (review of L3, CH-1).
        want = {'pio-lead': {'tools': {'read_run': {'approval_mode': 'approve'},
                                       'start_run': {'approval_mode': 'approve'}},
                             'default_tools_approval_mode': None}}
        by_json = lambda found: sorted(found, key=lambda s: json.dumps(s, sort_keys=True))
        servers = [e['servers'] for e in events_of(case, 'mcp_servers_sent')]
        assert by_json(servers) == by_json([{}, want]), servers
        received = [m['servers'] for m in case.markers_records()
                    if m['kind'] == 'thread_config_received']
        assert by_json(received) == by_json([{}, want]), received
        seen = witnessed(log)
        assert [e.get('method') for e in seen if e['event'] == 'request'] == \
            ['initialize', 'notifications/initialized', 'tools/list', 'tools/call', 'tools/call'], seen
        assert events_of(case, 'action_requested') == [], 'Codex asked about a pre-allowed tool'
        return dict(outcome='pass', sent=sent, launches=len([e for e in seen if e['event'] == 'started']))
    # Either way it settles: surfaced to the caller or, wrongly, declined.
    poll(lambda: case.inspect()['result'],
         lambda v: v['runtime'] in ('requires_action', 'exited'), 30)
    requested = events_of(case, 'action_requested')
    assert requested, ('the MCP tool-call approval was not surfaced: '
                       f"{events_of(case, 'native_request_declined')}")
    requested = requested[0]
    assert requested['method'] == 'mcpServer/elicitation/request' and \
        requested['approval_kind'] == 'mcp_tool_call' and requested['server'] == 'pio-lead', requested
    assert requested['persist_offered'] == ['session', 'always'], requested
    assert requested['answer_deadline_seconds'] == (2 if name == 'mcp_approval_lapses' else 120), requested
    if name == 'mcp_approval_lapses':
        # Two seconds of delivery timeout, then PIO's decline, or nothing.
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and not events_of(case, 'request_denied_by_default'):
            time.sleep(0.2)
        denied = events_of(case, 'request_denied_by_default')
        assert denied, 'nobody answered and nothing lapsed: the approval waits for ever'
        final = exited(case, seconds=30)
        assert [(d['decision'], d['decided_by'], d['sent']) for d in denied] == \
            [('decline', 'pio', {'action': 'decline'})], denied
        # On the wire: one answer to the one request, and it is a decline.
        wire = answered_once(case)
        assert [(m['count'], m['result']) for m in wire] == [(1, {'action': 'decline'})], wire
        assert [a['state'] for a in final['actions']] == ['answered'], final['actions']
        calls = [e for e in witnessed(log) if e.get('method') == 'tools/call']
        assert calls == [], 'the tool ran although nobody allowed it'
        return dict(outcome='pass', lapsed=denied[0], tool_calls=0)
    sent = []
    for decision in ('accept', 'decline'):
        view = poll(lambda: case.inspect()['result'],
                    lambda v: any(a['state'] == 'pending' for a in v.get('actions') or []), 30)
        action = next(a for a in view['actions'] if a['state'] == 'pending')
        body = json.dumps({'decision': decision}).encode()
        answer = case.execution_command('execution.respond_action', 'work',
                                        dict(action_id=action['action_id'],
                                             response=dict(digest=digest(body), media_type='application/json')),
                                        f"answer-{action['action_id']}", body, 'application/json')
        assert answer['result']['outcome']['state'] == 'answered', answer
    final = exited(case)
    applied = events_of(case, 'control_applied')
    assert [a['sent'] for a in applied] == [{'action': 'accept', 'content': {}}, {'action': 'decline'}], applied
    # On the wire, as the fake received them: each answered once, and the
    # accept remembers nothing (no `_meta.persist`).
    wire = answered_once(case)
    assert [m['result'] for m in wire] == [{'action': 'accept', 'content': {}}, {'action': 'decline'}], wire
    read = [(m['action'], m['persist']) for m in case.markers_records()
            if m['kind'] == 'mcp_approval_answered']
    assert read == [('accept', None), ('decline', None)], read
    calls = [e['name'] for e in witnessed(log) if e.get('method') == 'tools/call']
    assert calls == ['start_run'], calls
    return dict(outcome='pass', sent=[a['sent'] for a in applied], tool_calls=calls, exit=final['exit'])


def run_case(out, name):
    scenario = dict(
        approvals_reviewer_must_be_user={'approval': 'command', 'delay_ms': 100,
                                        'approvals_reviewer': 'guardian_subagent'},
        approvals_reviewer_absent_refused={'approval': 'command', 'delay_ms': 100,
                                           'approvals_reviewer': None},
        # Asked from outside the fixture: surfaced, and classified so.
        approval_decline={'approval': 'command', 'approval_kind': 'writeStdin', 'delay_ms': 100,
                          'approval_cwd': '/'},
        approval_accept={'approval': 'command', 'delay_ms': 100},
        interrupt_cancels_turn={'delay_ms': 60000},
        steer_acknowledged={'delay_ms': 60000},
        suppressed_ack_negative_control={'ack_turn': False, 'delay_ms': 300},
        restart_reattach_no_duplicate={'delay_ms': 4000},
        host_lost_no_respawn={'delay_ms': 60000},
        widening_decisions_refused={'approval': 'command', 'approval_kind': 'absent', 'delay_ms': 100},
        permission_grant_refused={'approval': 'permissions', 'delay_ms': 100},
        deadline_stop_interrupts={'delay_ms': 60000},
        lead_tool_on_the_thread={'lead': {'calls': [{'tool': 'start_run', 'arguments': {}},
                                                    {'tool': 'read_run', 'arguments': {}}]}},
        mcp_approval_to_the_caller={'lead': {'calls': [{'tool': 'start_run', 'arguments': {}},
                                                       {'tool': 'read_run', 'arguments': {}}]}},
        mcp_approval_lapses={'lead': {'calls': [{'tool': 'start_run', 'arguments': {}}]}},
        other_elicitation_declined={'approval': 'elicitation', 'delay_ms': 100},
        model_checked_before_turn={'model_provider': 'openai', 'delay_ms': 100},
        model_mismatch_refused={'model_provider': 'openai', 'model_reported': 'another-model'},
        provider_mismatch_refused={'delay_ms': 100},
    ).get(name, {'delay_ms': 100})
    case = Case(out, name, scenario, fake=(name != 'unqualified_executable_refused'))
    if name in MODEL_CASES:
        case.configure(thread=dict(case.config['codex']['thread'], model='gpt-5.6-terra'),
                       test_only_model_exception='owner-2026-09-19-m2-fixture-runs',
                       expected_model_provider='openai')
    try:
        if name == 'unqualified_executable_refused':
            daemon = case.start(expect_ready=False)
            code = daemon.wait(timeout=60)
            stderr = (case.out / 'daemon-0.stderr').read_text()
            assert code == 2 and 'codex_not_qualified' in stderr, (code, stderr)
            assert case.markers_records() == [] and not (case.root / 'public.sock').exists()
            qualification = json.loads((case.store / 'qualification.json').read_text())
            return dict(outcome='pass', exit=code, refusals=qualification['refusals'], app_server_spawned=False, native_work=False)
        case.start()
        if name == 'discovery_reports_observed_authentication':
            with case.client() as c:
                before = c.query('execution.discovery.list', {})['result']['installations'][0]
            assert before['authentication'] == 'unknown' and before['usable'] is False and before['adapter_recognized'] == 'no', before
            case.submit()
            exited(case)
            with case.client() as c:
                after = c.query('execution.discovery.list', {})['result']['installations'][0]
            assert after['authentication'] == 'authenticated' and after['reachable'] == 'yes' and after['last_verified'], after
            assert after['usable'] is False, 'a labeled fake is never usable Codex'
            return dict(outcome='pass', before=before, after=after)
        if name in ('missing_content_refused', 'outside_fixture_refused'):
            if name == 'outside_fixture_refused':
                outside = Path(tempfile.mkdtemp(prefix='pio-cx-outside-', dir='/tmp'))
                # Deliberately outside the fixture root, so it is this case's
                # to remove; the store's own release cannot reach it.
                case.extra_roots.append(outside)
                response, _ = case.submit(repository=outside)
            else:
                response, _ = case.submit(content=False)
            outcome = response['result']['outcome']
            assert outcome['admission'] == 'refused' and outcome['reason'] == 'capability_unavailable', response
            time.sleep(0.5)
            assert case.markers_records() == [] and case.journal()[1] == [], 'no app-server, no host admission'
            return dict(outcome='pass', admission=outcome, spawn_markers=0, invocations=0)
        if name == 'content_digest_mismatch':
            response, _ = case.submit(tamper=True)
            data = response['error']['data']
            assert data['code'] == 'invalid_envelope' and data['details']['path'] == '/extensions/pio.combraton.dev~1content', response
            assert case.markers_records() == []
            return dict(outcome='pass', refusal=data, spawn_markers=0)

        if name == 'thread_settings_broader_refused':
            (case.codex_home / 'config.toml').write_text('sandbox_mode = "read-only"\n')
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = poll(lambda: case.inspect()['result'], lambda v: v['delivery'] == 'failed_before_delivery')
            invocations = poll(lambda: case.journal()[1], lambda i: i and i[0]['phase'] == 'known_not_released')
            guard = events_of(case, 'thread_settings_guard')[0]['guard']
            assert guard['allowed'] is False and guard['broader_than_configured'] == [{'setting': 'sandbox_mode', 'requested': 'workspace-write', 'configured': 'read-only'}], guard
            assert case.markers_records() == [] and events_of(case, 'spawned') == [], 'refused before any app-server starts'
            assert 'thread_settings_refused' in invocations[0]['receipt']['reason']
            # A refusal before delivery is a **finished** execution. The fix is
            # in the shared projection, so it holds here too: before it, a
            # caller polling the runtime could not tell a refusal from a slow
            # start, and a live OpenCode run waited at `preparing` until it was
            # stopped by hand. `exit` stays `unavailable`: no exit code was
            # observed and none is claimed.
            final = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'exited')
            assert final['exit'] == 'unavailable', final
            return dict(outcome='pass', guard=guard, delivery=final['delivery'],
                        runtime=final['runtime'], exit=final['exit'], app_server_spawned=False)
        if name in MODEL_CASES:
            return model_case(case, name)
        if name in ('lead_tool_on_the_thread', 'mcp_approval_to_the_caller', 'mcp_approval_lapses'):
            return lead_tool_case(case, name)
        if name == 'other_elicitation_declined':
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            # Either way it settles: declined (exited) or, wrongly, surfaced.
            poll(lambda: case.inspect()['result'],
                 lambda v: v['runtime'] in ('exited', 'requires_action'), 30)
            assert events_of(case, 'action_requested') == [], 'surfaced an elicitation that asks for a login'
            final = exited(case)
            declined = events_of(case, 'native_request_declined')
            assert [d['method'] for d in declined] == ['mcpServer/elicitation/request'], declined
            # What was asked, by whom, and the true reason; never the URL.
            assert (declined[0]['server'], declined[0]['mode'], declined[0]['approval_kind']) \
                == ('someone', 'url', None), declined
            assert 'does not recognise as a tool-call approval' in declined[0]['reason'], declined
            assert 'example.invalid' not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['method'], d['server'], d['mode'], d['decided_by']) for d in carried] == \
                [('mcpServer/elicitation/request', 'someone', 'url', 'pio')], carried
            refused = [m for m in case.markers_records() if m['kind'] == 'approval_refused']
            assert len(refused) == 1, case.markers_records()
            return dict(outcome='pass', declined=declined[0]['method'], carried=carried,
                        actions=0, exit=final['exit'])
        brief = b'Fixture task: reply with one line.'
        response, repo = case.submit(brief=brief, deadline=3 if name == 'deadline_stop_interrupts' else 600)
        assert response['result']['outcome']['admission'] == 'admitted', response
        if name == 'restart_reattach_no_duplicate':
            view = poll(lambda: case.inspect()['result'], lambda v: v['delivery'] == 'acknowledged')
            before = case.journal()[1][0]
            case.stop(case.daemons[-1])
            case.start()
            final = exited(case)
            after = case.journal()[1][0]
            received = [m for m in case.markers_records() if m['kind'] == 'turn_received']
            spawned = [m for m in case.markers_records() if m['kind'] == 'spawned']
            assert len(received) == 1 and len(spawned) == 1, case.markers_records()
            assert after['host'] == before['host'] and after['child'] == before['child'] and after['invocation_id'] == before['invocation_id']
            assert final['delivery'] == 'acknowledged' and final['exit'] == {'code': 0}
            assert final['host']['generation'] > view['host']['generation']
            return dict(outcome='pass', spawn_markers=len(spawned), turn_received=len(received), host_identity=after['host'], child_identity=after['child'],
                        generations=dict(before=view['host']['generation'], after=final['host']['generation']), recovery=final['recovery'])
        if name == 'host_lost_no_respawn':
            poll(lambda: case.inspect()['result'], lambda v: v['delivery'] == 'acknowledged')
            state = case.journal()[1][0]
            case.stop(case.daemons[-1])
            for identity in (state['host'], state['child']):
                os.kill(identity['pid'], 9)
            for line in case.app_server_processes():
                os.kill(int(line.split()[0]), 9)
            time.sleep(0.3)
            case.start()
            view = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'unknown', 30)
            time.sleep(1)
            spawned = [m for m in case.markers_records() if m['kind'] == 'spawned']
            received = [m for m in case.markers_records() if m['kind'] == 'turn_received']
            assert len(spawned) == 1 and len(received) == 1, case.markers_records()
            assert view['delivery'] == 'acknowledged' and view['exit'] == 'unavailable', view
            assert case.app_server_processes() == []
            return dict(outcome='pass', runtime=view['runtime'], delivery=view['delivery'], spawn_markers=1, turn_received=1, recovery=view['recovery'], respawned=False)
        if name in ('approvals_reviewer_must_be_user', 'approvals_reviewer_absent_refused'):
            # Owner decision, 2026-09-22: approvals from led runs come to the
            # person. The app-server's own enum is
            # `user | auto_review | guardian_subagent`, and two of the three
            # send them somewhere else. PIO never sets the field, and every
            # run asserts the harness's answer — a field that is never read
            # is not a check, so this case makes the fake answer otherwise.
            # The second case has it answer **nothing**: the field is
            # required in 0.155.1's response, and silence is not `user`.
            answered = None if name == 'approvals_reviewer_absent_refused' else 'guardian_subagent'
            final = poll(lambda: case.inspect()['result'],
                         lambda v: v['runtime'] == 'exited', 60)
            started = events_of(case, 'thread_started')
            assert [e['approvals_reviewer'] for e in started] == [answered], started
            assert final['exit'] == 'unavailable', final
            # The host commits its failure phase just after the exit
            # observation, so `exited` on the view can arrive while the
            # receipt is still null. Read it once it is there; reading it
            # at once failed one run in twenty.
            invocations = poll(lambda: case.journal()[1],
                               lambda i: i and i[0]['phase'] == 'known_not_released')
            reason = str(invocations[0]['receipt']['reason'])
            assert 'approvals_reviewer_not_user' in reason, invocations
            return dict(outcome='pass', reviewer=answered,
                        refused='approvals_reviewer_not_user')

        if name == 'widening_decisions_refused':
            waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
            action = waiting['runtime_detail']['action_id']
            # `kind` is optional, and absent means a command approval.
            assert [e['approval_kind'] for e in events_of(case, 'action_requested')] == ['command']
            refused = []
            for n, decision in enumerate(['acceptForSession', {'acceptWithExecpolicyAmendment': {'execpolicy_amendment': ['echo', 'fixture']}},
                                          {'applyNetworkPolicyAmendment': {'network_policy_amendment': {'host': 'example.com', 'action': 'allow'}}}]):
                body = json.dumps({'decision': decision}).encode()
                answer = case.execution_command('execution.respond_action', 'work', dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')), f'widen-{n}', body, 'application/json')
                data = answer['error']['data']
                assert data['code'] == 'invalid_envelope' and data['details']['path'] == '/payload/response', answer
                refused.append(dict(decision=decision, code=data['code']))
            time.sleep(0.5)
            assert not list(case.store.glob('codex-*.controls.jsonl')), 'no control reached the host'
            assert [m for m in case.markers_records() if m['kind'] == 'approval_answered'] == []
            assert case.inspect()['result']['actions'][0]['state'] == 'pending'
            body = json.dumps({'decision': 'decline'}).encode()
            case.execution_command('execution.respond_action', 'work', dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')), 'decline', body, 'application/json')
            exited(case)
            answers = [m['decision'] for m in case.markers_records() if m['kind'] == 'approval_answered']
            assert answers == ['decline'], answers
            assert [m['result'] for m in answered_once(case)] == [{'decision': 'decline'}]
            return dict(outcome='pass', refused=refused, native_answers=answers)
        if name == 'deadline_stop_interrupts':
            final = exited(case, seconds=60)
            sent = [e for e in events_of(case, 'control_sent') if e['control_id'].endswith('.deadline-stop')]
            responses = [e for e in events_of(case, 'control_response') if e['control_id'].endswith('.deadline-stop')]
            assert len(sent) == 1 and sent[0]['method'] == 'turn/interrupt' and responses and responses[0]['error'] is None, (sent, responses)
            assert [e['status'] for e in events_of(case, 'turn_completed')] == ['interrupted']
            assert [e['code'] for e in events_of(case, 'app_server_exited')] == [0], 'app-server exited cleanly, not killed'
            assert 'cancellation' not in final and final['exit'] == {'code': 0}, final
            invocations = poll(lambda: case.journal()[1], lambda i: i and i[0]['phase'] == 'completed')
            with sqlite3.connect(f'file:{case.store}/journal.sqlite3?mode=ro', uri=True) as db:
                record = json.loads(db.execute("select value from protocol_projection where key='execution/work'").fetchone()[0])
            stop = record['codex']['deadline_stop']
            assert stop['request'] == 'turn_interrupt_acknowledged' and stop['outcome'] == 'interrupted', stop
            assert 'execution_deadline' in record['timeouts_passed']
            assert invocations[0]['receipt']['turn_status'] == 'interrupted'
            return dict(outcome='pass', deadline_stop=stop, turn_status='interrupted', app_server_exit=0, host_killed=False)
        if name == 'permission_grant_refused':
            # Surfacing a permission grant as an answerable action would stall
            # this case at requires_action until the wait expired. Watch for
            # either outcome so the failure names the defect instead of timing
            # out with a view dump.
            final = poll(lambda: case.inspect()['result'],
                         lambda v: v['runtime'] in ('exited', 'requires_action'), 30)
            assert final['runtime'] == 'exited', 'permission_grant_surfaced_as_action'
            declined = events_of(case, 'native_request_declined')
            assert [e['method'] for e in declined] == ['item/permissions/requestApproval'], declined
            assert 'widen' in declined[0]['reason']
            # The kinds of permission asked for, never the paths.
            assert declined[0]['permission_kinds'] == ['filesystem'], declined
            assert str(case.root) not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['method'], d['permission_kinds']) for d in carried] == \
                [('item/permissions/requestApproval', ['filesystem'])], carried
            # No action is ever surfaced, so nobody can answer a permission
            # grant. The view omits `actions` entirely when there are none.
            assert final.get('actions', []) == [] and events_of(case, 'action_requested') == [], final
            refusals = [m for m in case.markers_records() if m['kind'] == 'approval_refused']
            assert [m['code'] for m in refusals] == [-32000], refusals
            assert [m for m in case.markers_records() if m['kind'] == 'approval_answered'] == []
            return dict(outcome='pass', declined=declined[0]['method'], actions=0, native_refusal_code=-32000, exit=final['exit'])
        if name == 'suppressed_ack_negative_control':
            final = exited(case)
            delivery = final['deliveries'][0]
            assert final['delivery'] != 'acknowledged' and 'proof_class' not in delivery, final
            assert events_of(case, 'turn_acknowledged') == [] and len(events_of(case, 'turn_completed')) == 1
            return dict(outcome='pass', control='suppressed_native_turn_ack', property='acknowledged_requires_native_turn_ack', observed_delivery=final['delivery'], turn_completed_without_ack=True)
        if name in ('approval_decline', 'approval_accept'):
            decision = 'decline' if name == 'approval_decline' else 'accept'
            waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
            action = waiting['runtime_detail']['action_id']
            assert waiting['actions'][0]['state'] == 'pending'
            response_bytes = json.dumps({'decision': decision}).encode()
            answered = case.execution_command('execution.respond_action', 'work', dict(action_id=action, response=dict(digest=digest(response_bytes), media_type='application/json')), 'answer', response_bytes, 'application/json')
            assert answered['result']['outcome']['state'] == 'answered', answered
            effect = answered['result']['outcome']['response_effect']
            final = exited(case)
            answers = [m for m in case.markers_records() if m['kind'] == 'approval_answered']
            assert [a['decision'] for a in answers] == [decision], answers
            assert [m['result'] for m in answered_once(case)] == [{'decision': decision}]
            items = [e for e in events_of(case, 'item_completed') if e['item_id'] == 'item-approval']
            assert items and items[0]['status'] == ('completed' if decision == 'accept' else 'declined'), items
            # The decision records which kind of command approval it answered.
            requested = events_of(case, 'action_requested')
            expected_kind = 'writeStdin' if decision == 'decline' else 'command'
            assert [e['approval_kind'] for e in requested] == [expected_kind], requested
            # Where the command would run, classified against the workspace:
            # a label and a digest, never the path (review of L3, CH-6/F6).
            placement = requested[0]['classification']
            expected = ('outside_fixture', '<outside>') if decision == 'decline' \
                else ('inside_fixture', '<fixture>/')
            assert (placement['subject'], placement['placement'], placement['target_label']) \
                == ('cwd', *expected), placement
            assert str(case.root) not in json.dumps(placement), placement
            assert requested[0]['network_approval'] is False, requested
            with case.client() as c:
                stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                      'kinds': ['execution.execution']})['result']
            approval = [i['event']['payload']['pio.combraton.dev/approval'] for i in stream['items']
                        if 'event' in i and 'pio.combraton.dev/approval' in i['event']['payload']]
            assert [(a['classification']['placement'], a['reason'], a['network_approval'])
                    for a in approval] == [(expected[0], 'labeled fake approval request', False)], approval
            with case.client() as c:
                record = c.query('core.effects.get', {'effect': effect})['result']
            assert record['status'] == 'succeeded' and record['observations'][-1]['evidence']['class'] == 'native_request_resolved', record
            wrong = case.execution_command('execution.respond_action', 'work', dict(action_id=action, response=dict(digest=digest(response_bytes), media_type='application/json')), 'answer-again', response_bytes, 'application/json')
            assert wrong['error']['data']['code'] == 'not_found', wrong
            return dict(outcome='pass', action=action, decision=decision, native_answers=answers, item_status=items[0]['status'], response_effect=record['status'], repeat_answer=wrong['error']['data']['code'])
        if name == 'interrupt_cancels_turn':
            poll(lambda: case.inspect()['result'], lambda v: v['delivery'] == 'acknowledged')
            cancel = case.execution_command('execution.cancel', 'work', {}, 'cancel')
            assert 'result' in cancel, cancel
            final = exited(case)
            assert final['cancellation']['outcome'] == 'cancelled', final
            assert [e['status'] for e in events_of(case, 'turn_completed')] == ['interrupted']
            return dict(outcome='pass', cancellation=final['cancellation'], turn_status='interrupted')
        if name == 'steer_acknowledged':
            poll(lambda: case.inspect()['result'], lambda v: v['delivery'] == 'acknowledged')
            message = b'Also mention the fixture name.'
            steer = case.execution_command('execution.steer', 'work', dict(message=dict(digest=digest(message), media_type='text/plain')), 'steer', message, 'text/plain')
            assert steer['result']['outcome']['request'] == 'recorded', steer
            view = poll(lambda: case.inspect()['result'], lambda v: v.get('steering') and v['steering'][0]['delivery'] == 'acknowledged')
            entry = view['steering'][0]
            assert entry['proof_class'] == 'provider_ack_id' and entry['behavior'] == 'not_observed', entry
            case.execution_command('execution.cancel', 'work', {}, 'cancel')
            exited(case)
            return dict(outcome='pass', steering=entry)
        # j1_turn_completes
        final = exited(case)
        delivery = final['deliveries'][0]
        assert final['delivery'] == 'acknowledged' and delivery['proof_class'] == 'provider_ack_id', final
        assert final['exit'] == {'code': 0} and final['usage']['observations'][0]['amount'] == 42 and final['usage']['liability'] == 'resolved', final
        received = [m for m in case.markers_records() if m['kind'] == 'turn_received']
        assert len(received) == 1 and 'sha256:' + received[0]['input_sha256'] == digest(brief), received
        with case.client() as c:
            output = c.query('execution.output.read', {'execution': 'work', 'offset': 0})['result']
        import base64
        text = base64.b64decode(output['data_base64']).decode()
        assert 'fake agent reply' in text, text
        # Everything a caller must see is written before the event that makes
        # the view `exited`, so a reader that sees `exited` finds it. The
        # other order passed this case only by winning a race (review 47).
        kinds = [json.loads(l)['kind'] for f in case.store.glob('codex-*.events.jsonl')
                 for l in f.read_text().splitlines()]
        assert kinds.index('config_after') < kinds.index('app_server_exited'), kinds
        diff = events_of(case, 'config_after')[0]['diff']
        assert [p['location'] for p in diff['projects_added']] == ['fixture'] and diff['other_changes'] is False, diff
        thread = events_of(case, 'thread_started')[0]
        assert thread['sandbox']['type'] == 'workspaceWrite' and thread['model_provider'] == 'pio-fake'
        # The host commits its completed phase just after the exit observation.
        invocations = poll(lambda: case.journal()[1], lambda i: len(i) == 1 and i[0]['phase'] == 'completed')
        journal = case.journal()[0]
        assert 'Fixture task' not in json.dumps(journal), 'brief bytes must not enter the journal'
        return dict(outcome='pass', delivery=delivery, exit=final['exit'], usage=final['usage'], received_input_matches_brief=True, output_bytes=len(text),
                    config_diff=diff, thread=dict(sandbox=thread['sandbox']['type'], model=thread['model'], provider=thread['model_provider']),
                    source=final['deliveries'][0]['evidence']['source'])
    finally:
        case.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=1)
    parser.add_argument('--case', choices=CASES)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=False)
    results = []
    for name in ([args.case] if args.case else CASES):
        for repetition in range(1, args.repetitions + 1):
            result = dict(case=name, repetition=repetition, source='pio-fake-app-server', real_codex=False, attempted=1)
            try:
                result.update(run_case(args.out / f'rep-{repetition}', name))
            except Exception as error:
                # The line that raised, not only the exception: `IndexError:
                # list index out of range` named no line, and a flake on CI
                # went undiagnosed until a reviewer reproduced it (review 47).
                frame = traceback.extract_tb(error.__traceback__)[-1]
                result.update(outcome='harness_or_assertion_failure', reason=f'{type(error).__name__}: {error}',
                              failed_at=f'{Path(frame.filename).name}:{frame.lineno} in {frame.name}: {frame.line}')
            results.append(result)
            print(name, repetition, result['outcome'], result.get('reason', ''), result.get('failed_at', ''), flush=True)
    counts = dict(Counter(r['outcome'] for r in results))
    report = dict(format='pio-codex-host-matrix/1', source='pio-fake-app-server', real_codex=False, live_tokens=0,
                  head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)), platform=platform.platform(),
                  binary_sha256=hashlib.sha256(BINARY.read_bytes()).hexdigest(), attempted=len(results), repetitions=args.repetitions, counts=counts, results=results)
    (args.out / 'matrix.json').write_text(json.dumps(report, indent=2) + '\n')
    assert set(counts) <= {'pass', 'expected_property_failure'}, counts


if __name__ == '__main__':
    main()
