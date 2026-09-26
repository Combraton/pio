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
         'model_checked_before_turn', 'model_mismatch_refused', 'provider_mismatch_refused',
         'command_approval_lapses', 'form_elicitation_declined', 'approval_cwd_through_a_link',
         'approval_cwd_past_path_max', 'subagent_thread_attributed',
         'subagent_interrupted_with_the_run', 'features_off_decision_sent',
         'early_declines_in_every_wait', 'early_decline_then_exit', 'file_change_no_root',
         'file_change_root_inside',
         'early_elicitation_declined', 'user_input_declined', 'network_and_unplaced_approval',
         'file_change_grant_root', 'continuation_interrupted', 'subagent_turn_after_interrupt',
         'url_elicitation_with_approval_kind_declined']
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

    def submit(self, identity='work', brief=b'Fixture task: reply with one line.', content=True, repository=None, tamper=False, deadline=600, delivery=120, extensions=None, base='unused'):
        repo, base = self.fixture(identity) if repository is None else (repository, base)
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
# Codex's unmetered, default-on features off per launch, as the host must
# send them (`pio_codex::features_off`, rust-v0.157.0; review of L3, round 4).
FEATURES_OFF = {'agents.enabled': False, 'features.multi_agent': False,
                'features.multi_agent_v2': False, 'features.memories': False,
                'features.memory_tool': False, 'features.goals': False,
                'web_search': 'disabled', 'features.image_generation': False}
OTHER_THREADS = 'pio.combraton.dev/other-threads'


def answered_once(case):
    """What the fake received, by request id, and that no request was
    answered twice: the wire, not PIO's own record of it (review of L3,
    CH-3)."""
    markers = case.markers_records()
    twice = [m for m in markers if m['kind'] == 'second_response']
    assert twice == [], twice
    return [m for m in markers if m['kind'] == 'response_received']


def carried_declines(case, identity='work', key=NATIVE):
    """The run's native declines as a caller reads them: off the stream, on
    the run's own exit event, and nowhere else (review of L3, CH-2/F1). Or,
    with `key`, another list the exit carries."""
    with case.client() as c:
        result = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                              'kinds': ['execution.execution']})['result']
    exits = [i['event'] for i in result['items'] if 'event' in i
             and i['event']['type'] == 'execution.exit.observed'
             and i['event']['subject']['id'] == identity]
    assert len(exits) == 1, exits
    assert key in exits[0]['payload'], exits[0]
    return exits[0]['payload'][key]


def subagent_case(case, name):
    """A run whose thread spawns a sub-agent, as Codex 0.157.0 sends it: the
    parent's `subAgentActivity` item, then the agent's own thread on the same
    connection, its turn, its usage, a command approval it asks and its own
    words (review of L3, round 3, SPEND-2). None of it is the run's: the
    agent's turn/completed does not end the run's turn, its words are not
    the run's output, its request is declined by PIO and never put to the
    caller, and its usage is added to the run's, which is the sum over the
    run's threads. The exit carries the thread, how it appeared and its
    usage. Interrupting the run interrupts the agent's turn too."""
    if name == 'features_off_decision_sent':
        # The rehearsal's own token, beside a labeled fake: every thread's
        # config turns Codex's unmetered features off, by the keys
        # `pio_codex::features_off` gives, read back from the request and
        # witnessed by the fake, and no agent is spawned and no memory
        # pipeline runs (review of L3, round 4, U1).
        case.configure(features_off_decision='rehearsal-only-features-off')
    case.start()
    response, _ = case.submit()
    assert response['result']['outcome']['admission'] == 'admitted', response
    sub = 'fake-sub-thread-1'
    if name == 'features_off_decision_sent':
        final = exited(case)
        sent = events_of(case, 'features_off_sent')
        assert [(e['decision'], e['sent']) for e in sent] == [
            ('rehearsal-only-features-off', FEATURES_OFF)], sent
        received = [m for m in case.markers_records() if m['kind'] == 'thread_config_received']
        assert [(m['agents_enabled'], m['multi_agent'], m['multi_agent_v2'], m['features_off'])
                for m in received] == [(False, False, False, FEATURES_OFF)], received
        kinds = [m['kind'] for m in case.markers_records()]
        assert 'spawn_not_offered' in kinds and 'sub_agent_spawned' not in kinds, kinds
        assert 'memory_pipeline_not_started' in kinds, kinds
        assert 'continuation_not_started' in kinds and 'continuation_started' not in kinds, kinds
        assert events_of(case, 'continuation_started') == []
        assert not (case.codex_home / 'memories').exists(), 'the memory pipeline ran'
        assert events_of(case, 'other_thread') == [] and carried_declines(case, key=OTHER_THREADS) == []
        assert final['exit'] == {'code': 0}, final
        return dict(outcome='pass', sent=sent[0]['sent'])
    if name in ('subagent_interrupted_with_the_run', 'subagent_turn_after_interrupt'):
        poll(lambda: events_of(case, 'other_thread_turn'),
             lambda turns: any(e['state'] == 'started' for e in turns))
        case.execution_command('execution.cancel', 'work', {}, 'cancel-sub')
        final = exited(case)
        order = [json.loads(l) for f in case.store.glob('codex-*.events.jsonl')
                 for l in f.read_text().splitlines()]
        sent = [e for e in order if e['kind'] == 'control_sent'
                and e.get('method') == 'turn/interrupt']
        own = [e for e in sent if not e.get('thread_id')]
        subs = [e for e in sent if e.get('thread_id') == sub]
        # Interrupted **with** the run: under the cancel's own control id,
        # before the run's own turn ended, never only afterwards as the
        # host's 'run-ended' sweep (review of L3, round 4, R4-HC-1).
        assert len(own) == 1 and own[0]['control_id'] != 'run-ended', sent
        assert subs and subs[0]['control_id'] == own[0]['control_id'], sent
        own_end = next(i for i, e in enumerate(order) if e['kind'] == 'turn_completed')
        assert order.index(subs[0]) < own_end, [e['kind'] for e in order]
        interrupted = [m for m in case.markers_records() if m['kind'] == 'sub_agent_interrupted']
        turns = events_of(case, 'other_thread_turn')
        assert final['cancellation'].get('outcome') == 'cancelled', final
        if name == 'subagent_interrupted_with_the_run':
            assert [m['thread'] for m in interrupted] == [sub], interrupted
            assert [e['state'] for e in turns] == ['started', 'interrupted'], turns
            return dict(outcome='pass', interrupted=[sub], control=subs[0]['control_id'])
        # Set going again on the same thread after the cancel: its new turn
        # is interrupted too, by the sweep after the run's own turn, since
        # what was interrupted is a thread's turn, not the thread.
        assert [(m['thread'], m['turn']) for m in interrupted] == [
            (sub, 'fake-sub-turn-1'), (sub, 'fake-sub-turn-1-again')], interrupted
        assert [(e['control_id'], e['turn_id']) for e in subs] == [
            (own[0]['control_id'], 'fake-sub-turn-1'), ('run-ended', 'fake-sub-turn-1-again')], subs
        assert [(e['turn_id'], e['state']) for e in turns] == [
            ('fake-sub-turn-1', 'started'), ('fake-sub-turn-1', 'interrupted'),
            ('fake-sub-turn-1-again', 'started'), ('fake-sub-turn-1-again', 'interrupted')], turns
        return dict(outcome='pass', interrupted=[e['turn_id'] for e in subs])
    final = exited(case)
    assert final['exit'] == {'code': 0}, final
    # The run's own turn ended the run, once, and the agent's did not.
    completed = events_of(case, 'turn_completed')
    own_turn = events_of(case, 'turn_acknowledged')[0]['turn_id']
    assert [e['turn_id'] for e in completed] == [own_turn], completed
    assert [e['state'] for e in events_of(case, 'other_thread_turn')] == ['started', 'completed']
    # How it appeared: named by the run's own thread, before it said anything.
    seen = events_of(case, 'other_thread')
    assert [(e['thread_id'], e['how']['by'], e['how']['item_type']) for e in seen] == \
        [(sub, "named by this run's own thread", 'subAgentActivity')], seen
    # Its request: declined by PIO, recorded as another thread's, never an action.
    assert events_of(case, 'action_requested') == [], 'surfaced a sub-agent request'
    declined = events_of(case, 'native_request_declined')
    assert [(d['method'], d['thread_id'], d['reason']) for d in declined] == [
        ('item/commandExecution/requestApproval', sub,
         'declined by PIO: a request from a thread this run did not start')], declined
    asked = [m for m in case.markers_records() if m['kind'] == 'sub_agent_asked']
    assert len(asked) == 1 and asked[0]['refused'], asked
    # Its usage, apart and summed: 5,000 and 10,000 on its thread, 42 on the run's.
    usage = events_of(case, 'usage')
    assert [(e['own_thread'], e['total']['totalTokens'], e['run_total']) for e in usage] == [
        (False, 5000, 5000), (False, 10000, 10000), (True, 42, 10042)], usage
    assert final['usage']['observations'][0]['amount'] == 10042, final['usage']
    # Its words are not the run's.
    with case.client() as c:
        output = c.query('execution.output.read', {'execution': 'work', 'offset': 0})['result']
    import base64
    text = base64.b64decode(output['data_base64']).decode()
    assert 'fake agent reply' in text and 'SENTINEL-sub' not in text, text
    carried = carried_declines(case, key=OTHER_THREADS)
    assert [(o['thread_id'], o['how']['by'], o['usage_total']) for o in carried] == \
        [(sub, "named by this run's own thread", 10000)], carried
    return dict(outcome='pass', other_threads=carried, run_total=10042)


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


def nested_dirs(dir_fd, names):
    """Make `names` as nested directories, each relative to the one before by
    its descriptor, so no call names a long path; the deepest's descriptor."""
    for name in names:
        os.mkdir(name, dir_fd=dir_fd)
        inner = os.open(name, os.O_RDONLY | os.O_DIRECTORY, dir_fd=dir_fd)
        os.close(dir_fd)
        dir_fd = inner
    return dir_fd


def past_path_max(repo, outside):
    """`L1/L2/esc` under `repo`: two links, each under PATH_MAX and together
    past it, the second inside the directory the first leads to, and `esc`
    out to `outside`. The kernel follows it one component at a time; a
    resolver that builds the whole path cannot read the second link
    (ENAMETOOLONG), which it took for "not a link" until round 3 of the
    review of L3 (R3-HC-1)."""
    path_max = os.pathconf('/', 'PC_PATH_MAX')
    per_link = (path_max - 64) // 251
    first = [f'a{n:02}' + 'd' * 247 for n in range(per_link)]
    second = [f'b{n:02}' + 'd' * 247 for n in range(per_link)]
    os.mkdir(repo / 't')
    a = nested_dirs(os.open(repo / 't', os.O_RDONLY | os.O_DIRECTORY), first)
    os.symlink('t/' + '/'.join(first), repo / 'L1')
    b = nested_dirs(os.dup(a), second)
    os.symlink('/'.join(second), 'L2', dir_fd=a)
    os.symlink(str(outside), 'esc', dir_fd=b)
    os.close(a)
    os.close(b)
    assert len(str(repo)) + 2 * per_link * 251 > path_max
    return repo / 'L1' / 'L2' / 'esc'


def file_change_case(case, name):
    """A file-change approval with no root, and one whose root is inside the
    fixture (review of L3, round 3, R3-HC-6): the first says it asks for
    none and carries no placement; the second places its root inside, by
    label and digest. The relay leaves both to the owner."""
    import l3_desk_relay
    repo, base = case.fixture('work')
    scenario = json.loads(case.config['codex']['env']['PIO_CODEX_FAKE_SCENARIO'])
    if name == 'file_change_root_inside':
        (repo / 'src').mkdir(exist_ok=True)
        scenario['approval_grant_root'] = str(repo / 'src')
    case.config['codex']['env']['PIO_CODEX_FAKE_SCENARIO'] = json.dumps(scenario)
    case.config_path.write_text(json.dumps(case.config))
    case.start()
    response, _ = case.submit(repository=repo, base=base)
    assert response['result']['outcome']['admission'] == 'admitted', response
    waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
    requested = events_of(case, 'action_requested')[0]
    assert requested['method'] == 'item/fileChange/requestApproval', requested
    if name == 'file_change_no_root':
        assert requested['grant_root_requested'] is False, requested
        assert 'classification' not in requested, requested
    else:
        assert requested['grant_root_requested'] is True, requested
        placement = requested['classification']
        assert (placement['subject'], placement['placement'], placement['target_label']) == \
            ('grant_root', 'inside_fixture', '<fixture>/src'), placement
        assert str(repo) not in json.dumps(placement), placement
    with case.client() as c:
        stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                              'kinds': ['execution.execution']})['result']
    approval = next(i['event']['payload']['pio.combraton.dev/approval'] for i in stream['items']
                    if 'event' in i and 'pio.combraton.dev/approval' in i['event']['payload'])
    assert approval['grant_root_requested'] is (name == 'file_change_root_inside'), approval
    assert l3_desk_relay.decided_in_advance(dict(run='L3.beta', approval=approval)) is None
    action = waiting['runtime_detail']['action_id']
    body = json.dumps({'decision': 'decline'}).encode()
    case.execution_command('execution.respond_action', 'work',
                           dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')),
                           'decline', body, 'application/json')
    exited(case)
    return dict(outcome='pass', grant_root=requested['grant_root_requested'])


def link_case(case, name):
    """A command approval whose working directory is inside the fixture by
    its spelling and outside it on disk: through `self -> .` and then
    `esc2 -> <outside>`, with no `..` for any lexical check to catch. Codex
    joins a command's workdir without canonicalizing it, so this is what
    reaches PIO (review of L3, round 2, HR-1). It must read outside_fixture,
    and the L3 relay must not answer it. Or through two links whose
    resolution is longer than PATH_MAX, which must read not_classifiable
    (round 3, R3-HC-1)."""
    import l3_desk_relay
    repo, base = case.fixture('work')
    outside = Path(tempfile.mkdtemp(prefix='pio-cx-outside-', dir='/tmp')).resolve()
    case.extra_roots.append(outside)
    if name == 'approval_cwd_past_path_max':
        cwd = past_path_max(repo, outside)
        expected = ('not_classifiable', None)
    else:
        os.symlink('.', repo / 'self')
        os.symlink(outside, repo / 'esc2')
        cwd = repo / 'self' / 'esc2'
        expected = ('outside_fixture', '<outside>')
    # The kernel lands outside either way.
    assert os.path.samefile(cwd, outside), cwd
    scenario = json.loads(case.config['codex']['env']['PIO_CODEX_FAKE_SCENARIO'])
    scenario['approval_cwd'] = str(cwd)
    case.config['codex']['env']['PIO_CODEX_FAKE_SCENARIO'] = json.dumps(scenario)
    case.config_path.write_text(json.dumps(case.config))
    case.start()
    response, _ = case.submit(repository=repo, base=base)
    assert response['result']['outcome']['admission'] == 'admitted', response
    waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
    placement = events_of(case, 'action_requested')[0]['classification']
    assert (placement['placement'], placement['target_label']) == expected, placement
    assert str(outside) not in json.dumps(placement) and str(repo) not in json.dumps(placement), placement
    with case.client() as c:
        stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                              'kinds': ['execution.execution']})['result']
    approval = next(i['event']['payload']['pio.combraton.dev/approval'] for i in stream['items']
                    if 'event' in i and 'pio.combraton.dev/approval' in i['event']['payload'])
    assert approval['classification']['placement'] == expected[0], approval
    # The relay, given this very payload for a command it would otherwise
    # answer, leaves it to the owner; the same item placed inside is answered.
    item = dict(run='L3.beta', approval=dict(approval, command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'"))
    assert l3_desk_relay.decided_in_advance(item) is None, item
    inside = dict(item, approval=dict(item['approval'], classification=dict(
        approval['classification'], placement='inside_fixture')))
    assert l3_desk_relay.decided_in_advance(inside), inside
    action = waiting['runtime_detail']['action_id']
    body = json.dumps({'decision': 'decline'}).encode()
    case.execution_command('execution.respond_action', 'work',
                           dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')),
                           'decline', body, 'application/json')
    exited(case)
    return dict(outcome='pass', placement=placement['placement'], label=placement['target_label'],
                relay_answered=False)


def run_case(out, name):
    scenario = dict(
        approvals_reviewer_must_be_user={'approval': 'command', 'delay_ms': 100,
                                        'approvals_reviewer': 'guardian_subagent'},
        approvals_reviewer_absent_refused={'approval': 'command', 'delay_ms': 100,
                                           'approvals_reviewer': None},
        # Asked from outside the fixture: surfaced, and classified so.
        approval_decline={'approval': 'command', 'approval_kind': 'writeStdin', 'delay_ms': 100,
                          'approval_cwd': '/'},
        # A reason where Codex gives one; the measured command approvals (M2 R5,
        # R6) gave none, which the other cases send.
        approval_accept={'approval': 'command', 'delay_ms': 100,
                         'approval_reason': 'labeled fake approval request'},
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
        url_elicitation_with_approval_kind_declined={'approval': 'elicitation', 'delay_ms': 100,
                                                     'elicitation_meta_kind': 'mcp_tool_call'},
        command_approval_lapses={'approval': 'command', 'delay_ms': 100},
        form_elicitation_declined={'approval': 'elicitation', 'elicitation_mode': 'form',
                                   'delay_ms': 100},
        approval_cwd_through_a_link={'approval': 'command', 'delay_ms': 100},
        approval_cwd_past_path_max={'approval': 'command', 'delay_ms': 100},
        subagent_thread_attributed={'spawn_agent': True, 'subagent_steps': 2, 'subagent_step': 5000,
                                    'subagent_step_ms': 300, 'subagent_asks': True,
                                    'delay_ms': 4000, 'usage_total': 42},
        features_off_decision_sent={'spawn_agent': True, 'memory_pipeline': True,
                                    'continue_after_turn': True, 'delay_ms': 500},
        continuation_interrupted={'continue_after_turn': True, 'continuation_step_ms': 60000,
                                  'delay_ms': 100},
        early_declines_in_every_wait={'elicit_during': ['initialize', 'account/read', 'thread/start'],
                                      'delay_ms': 100},
        early_decline_then_exit={'elicit_during': ['account/read'], 'exit_after_early': True},
        subagent_interrupted_with_the_run={'spawn_agent': True, 'subagent_steps': 2,
                                           'subagent_step_ms': 60000, 'delay_ms': 60000},
        subagent_turn_after_interrupt={'spawn_agent': True, 'subagent_steps': 2,
                                       'subagent_step_ms': 60000, 'subagent_restarts': True,
                                       'delay_ms': 60000},
        early_elicitation_declined={'elicit_during_thread_start': True, 'delay_ms': 100},
        user_input_declined={'approval': 'user_input', 'delay_ms': 100},
        file_change_grant_root={'approval': 'fileChange', 'delay_ms': 100, 'approval_grant_root': '/'},
        file_change_no_root={'approval': 'fileChange', 'delay_ms': 100},
        file_change_root_inside={'approval': 'fileChange', 'delay_ms': 100},
        network_and_unplaced_approval={'approval': 'command', 'delay_ms': 100,
                                       'approval_network': True, 'approval_no_cwd': True},
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
        if name in ('approval_cwd_through_a_link', 'approval_cwd_past_path_max'):
            return link_case(case, name)
        if name in ('file_change_no_root', 'file_change_root_inside'):
            return file_change_case(case, name)
        if name in ('subagent_thread_attributed', 'subagent_interrupted_with_the_run',
                    'features_off_decision_sent', 'subagent_turn_after_interrupt'):
            return subagent_case(case, name)
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
        if name == 'file_change_grant_root':
            # A file change that asks for writes under a root: the request says
            # so, and where the root lands, by label and digest (review of L3,
            # round 2, HR-8); the L3 relay leaves every file change to the owner.
            import l3_desk_relay
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
            requested = events_of(case, 'action_requested')[0]
            assert requested['method'] == 'item/fileChange/requestApproval', requested
            assert requested['grant_root_requested'] is True, requested
            placement = requested['classification']
            assert (placement['subject'], placement['placement'], placement['target_label']) == \
                ('grant_root', 'outside_fixture', '<outside>'), placement
            with case.client() as c:
                stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                      'kinds': ['execution.execution']})['result']
            approval = next(i['event']['payload']['pio.combraton.dev/approval'] for i in stream['items']
                            if 'event' in i and 'pio.combraton.dev/approval' in i['event']['payload'])
            assert approval['grant_root_requested'] is True and \
                approval['classification']['subject'] == 'grant_root', approval
            assert l3_desk_relay.decided_in_advance(dict(run='L3.beta', approval=approval)) is None
            action = waiting['runtime_detail']['action_id']
            body = json.dumps({'decision': 'decline'}).encode()
            case.execution_command('execution.respond_action', 'work',
                                   dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')),
                                   'decline', body, 'application/json')
            exited(case)
            return dict(outcome='pass', grant_root='outside_fixture')
        if name == 'network_and_unplaced_approval':
            # A command approval that asks for the network and names no cwd:
            # the host says both, and the L3 relay answers neither, from the
            # payload the host produced (review of L3, round 2, HR-3).
            import l3_desk_relay
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            waiting = poll(lambda: case.inspect()['result'], lambda v: v['runtime'] == 'requires_action')
            requested = events_of(case, 'action_requested')[0]
            assert requested['network_approval'] is True, requested
            assert (requested['classification']['placement'], requested['classification']['target_label']) \
                == ('not_classifiable', None), requested
            # 0.157.0's network ask names no command and no cwd (review of L3,
            # round 4, R4-HC-5): the relay's exact-command check refuses it by
            # itself, whatever the network flag says.
            assert requested['command'] is None and requested['cwd_digest'] is None, requested
            with case.client() as c:
                stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                      'kinds': ['execution.execution']})['result']
            approval = next(i['event']['payload']['pio.combraton.dev/approval'] for i in stream['items']
                            if 'event' in i and 'pio.combraton.dev/approval' in i['event']['payload'])
            assert approval['network_approval'] is True and \
                approval['classification']['placement'] == 'not_classifiable', approval
            item = lambda **change: dict(run='L3.beta', approval=dict(
                approval, command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'", **change))
            inside = dict(approval['classification'], placement='inside_fixture')
            assert l3_desk_relay.decided_in_advance(item()) is None
            assert l3_desk_relay.decided_in_advance(item(classification=inside)) is None, 'network'
            assert l3_desk_relay.decided_in_advance(item(network_approval=False)) is None, 'unplaced'
            assert l3_desk_relay.decided_in_advance(item(classification=inside, network_approval=False))
            action = waiting['runtime_detail']['action_id']
            body = json.dumps({'decision': 'decline'}).encode()
            case.execution_command('execution.respond_action', 'work',
                                   dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')),
                                   'decline', body, 'application/json')
            exited(case)
            return dict(outcome='pass', network_approval=True, placement='not_classifiable')
        if name == 'continuation_interrupted':
            # A turn Codex starts by itself on the run's own thread once the
            # run's turn has ended, as a goal's continuation does (review of
            # L3, round 4, SPEND-9): the host, still reading that thread for
            # its grace period, records it, interrupts it under control
            # `continuation`, waits for its end, and the exit carries it. The
            # run's own turn status stands.
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = exited(case)
            order = [json.loads(l) for f in case.store.glob('codex-*.events.jsonl')
                     for l in f.read_text().splitlines()]
            kinds = [e['kind'] for e in order]
            own = events_of(case, 'turn_acknowledged')[0]['turn_id']
            started = events_of(case, 'continuation_started')
            assert [e['turn_id'] for e in started] == [f'{own}-continued'], started
            assert kinds.index('continuation_started') > kinds.index('turn_completed'), kinds
            sent = [e for e in events_of(case, 'control_sent') if e['control_id'] == 'continuation']
            assert [(e['method'], e['turn_id']) for e in sent] == \
                [('turn/interrupt', f'{own}-continued')], sent
            assert any(e['kind'] == 'control_response' and e['control_id'] == 'continuation'
                       for e in order), kinds
            completed = events_of(case, 'turn_completed')
            assert [(e['turn_id'], e['status'], e.get('continuation')) for e in completed] == [
                (own, 'completed', None), (f'{own}-continued', 'interrupted', True)], completed
            carried = carried_declines(case, key='pio.combraton.dev/continuations')
            assert [(c['turn_id'], c.get('status')) for c in carried] == \
                [(f'{own}-continued', 'interrupted')], carried
            receipt = case.journal()[1][0]['receipt']
            assert receipt['turn_status'] == 'completed', receipt
            with case.client() as c:
                output = c.query('execution.output.read', {'execution': 'work', 'offset': 0})['result']
            import base64
            assert 'SENTINEL-continued' not in base64.b64decode(output['data_base64']).decode()
            assert final['exit'] == {'code': 0}, final
            return dict(outcome='pass', continuation=carried[0])
        if name == 'user_input_declined':
            # Codex's other route for an MCP tool-call approval: declined by
            # PIO, recorded by how many questions and whether one is that
            # approval, never by their text or options (review of L3, round 2,
            # HR-2 and HR-4).
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = exited(case)
            assert events_of(case, 'action_requested') == [], 'surfaced a request for input'
            declined = events_of(case, 'native_request_declined')
            assert [(d['method'], d['questions'], d['mcp_tool_call_approval'], d['phase'])
                    for d in declined] == [('item/tool/requestUserInput', 1, True, 'turn')], declined
            assert 'SENTINEL' not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['method'], d['mcp_tool_call_approval']) for d in carried] == \
                [('item/tool/requestUserInput', True)] and 'SENTINEL' not in json.dumps(carried), carried
            return dict(outcome='pass', declined=declined[0]['method'], exit=final['exit'])
        if name == 'early_elicitation_declined':
            # A request that arrives while the host waits for thread/start:
            # answered at once with PIO's decline, recorded with its phase,
            # and carried on the exit (review of L3, round 2, V-2/HR-6).
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = exited(case)
            declined = events_of(case, 'native_request_declined')
            assert [(d['method'], d['phase'], d['wait'], d['server'], d['mode']) for d in declined] == \
                [('mcpServer/elicitation/request', 'before_turn', 'thread/start', 'someone', 'url')], \
                declined
            assert 'example.invalid' not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['method'], d['phase']) for d in carried] == \
                [('mcpServer/elicitation/request', 'before_turn')], carried
            wire = answered_once(case)
            assert [(m['id'], m['error']['code']) for m in wire] == [('fake-early-1', -32000)], wire
            received = [m for m in case.markers_records() if m['kind'] == 'turn_received']
            assert len(received) == 1 and final['exit'] == {'code': 0}, final
            return dict(outcome='pass', declined=declined[0]['phase'], exit=final['exit'])
        if name == 'early_declines_in_every_wait':
            # A request before each answer the host waits for before its
            # first turn: initialize, account/read and thread/start, each
            # declined once and recorded with its wait (review of L3, round
            # 3, C3-2: only the thread/start wait was exercised).
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = exited(case)
            waits = ['initialize', 'account/read', 'thread/start']
            declined = events_of(case, 'native_request_declined')
            assert [(d['phase'], d['wait']) for d in declined] == \
                [('before_turn', w) for w in waits], declined
            carried = carried_declines(case)
            assert [d['wait'] for d in carried] == waits, carried
            # What the fake sent: Codex's shape in the thread/start window,
            # the new thread's own id; before a thread exists, a defensive
            # shape with none (review of L3, round 4, R4-HC-5).
            thread = events_of(case, 'thread_started')[0]['thread_id']
            sent = [(m['wait'], m['thread_id']) for m in case.markers_records()
                    if m['kind'] == 'early_request_sent']
            assert sent == [('initialize', None), ('account/read', None),
                            ('thread/start', thread)], sent
            wire = answered_once(case)
            assert sorted(m['id'] for m in wire) == sorted(
                f"fake-early-{w.replace('/', '-')}" for w in waits), wire
            assert final['exit'] == {'code': 0}, final
            return dict(outcome='pass', waits=waits)
        if name == 'early_decline_then_exit':
            # The app-server declines-then-dies case: a request during
            # account/read, answered by PIO, and then the app-server exits
            # without answering account/read. The host's wait fails, and the
            # decline it sent is still recorded (review of L3, round 3,
            # R3-HC-3: it returned only with a successful response).
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            poll(lambda: events_of(case, 'host_error') or events_of(case, 'app_server_exited'),
                 lambda found: bool(found), seconds=90)
            declined = events_of(case, 'native_request_declined')
            assert [(d['method'], d['wait'], d['phase']) for d in declined] == \
                [('mcpServer/elicitation/request', 'account/read', 'before_turn')], declined
            wire = answered_once(case)
            assert [(m['id'], m['error']['code']) for m in wire] == \
                [('fake-early-account-read', -32000)], wire
            assert any(m['kind'] == 'exiting_after_early' for m in case.markers_records())
            # And on the stream: the run has no exit event, so the decline
            # rides on the runtime change that marks it refused before
            # delivery (review of L3, round 4, R4-HC-2).
            final = exited(case)
            with case.client() as c:
                stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                      'kinds': ['execution.execution']})['result']
            events = [i['event'] for i in stream['items'] if 'event' in i]
            assert not any(e['type'] == 'execution.exit.observed' for e in events), events
            refused = [e['payload'] for e in events if e['type'] == 'execution.runtime.changed'
                       and e['payload'].get('reason') == 'refused_before_delivery']
            assert [[(d['method'], d['wait'], d['phase']) for d in r[NATIVE]] for r in refused] == \
                [[('mcpServer/elicitation/request', 'account/read', 'before_turn')]], refused
            assert 'example.invalid' not in json.dumps(refused), refused
            assert (final['runtime'], final['exit']) == ('exited', 'unavailable'), final
            return dict(outcome='pass', recorded=declined[0]['wait'], on_stream=True)
        if name == 'command_approval_lapses':
            # A command approval nobody answers: after the caller's two
            # seconds, one decline of PIO's, and the command never runs. In
            # L3 every unexpected child command reaches exactly this path
            # (review of L3, CH-4).
            response, _ = case.submit(delivery=2)
            assert response['result']['outcome']['admission'] == 'admitted', response
            final = exited(case, seconds=30)
            requested = events_of(case, 'action_requested')
            assert [(r['method'], r['approval_kind'], r['command'], r['answer_deadline_seconds'])
                    for r in requested] == [('item/commandExecution/requestApproval', 'command',
                                             'echo fixture', 2)], requested
            denied = events_of(case, 'request_denied_by_default')
            assert [(d['decision'], d['decided_by'], d['sent']) for d in denied] == \
                [('decline', 'pio', {'decision': 'decline'})], denied
            wire = answered_once(case)
            assert [m['result'] for m in wire] == [{'decision': 'decline'}], wire
            answers = [m['decision'] for m in case.markers_records() if m['kind'] == 'approval_answered']
            assert answers == ['decline'], answers
            items = [e for e in events_of(case, 'item_completed') if e['item_id'] == 'item-approval']
            assert [i['status'] for i in items] == ['declined'], 'the command ran although nobody allowed it'
            assert [a['state'] for a in final['actions']] == ['answered'], final['actions']
            with case.client() as c:
                stream = c.query('core.events.read', {'limit': 1000, 'from': 'start',
                                                      'kinds': ['execution.execution']})['result']
            decided = [i['event']['payload'].get('pio.combraton.dev/decision') for i in stream['items']
                       if 'event' in i and i['event']['type'] == 'execution.action.answered']
            assert [(d['decided_by'], d['decision'], d['basis']) for d in decided] == \
                [('pio', 'decline', 'deadline_lapsed')], decided
            return dict(outcome='pass', lapsed=denied[0], native_answers=answers, item='declined')
        if name == 'form_elicitation_declined':
            # A form asking for data: the mode of a tool-call approval, but
            # not one (no `_meta.codex_approval_kind`). Declined by PIO,
            # never surfaced, never answered `accept` with empty content.
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            poll(lambda: case.inspect()['result'],
                 lambda v: v['runtime'] in ('exited', 'requires_action'), 30)
            assert events_of(case, 'action_requested') == [], 'surfaced a form that asks for data'
            final = exited(case)
            declined = events_of(case, 'native_request_declined')
            assert [(d['method'], d['server'], d['mode'], d['approval_kind']) for d in declined] == \
                [('mcpServer/elicitation/request', 'someone', 'form', None)], declined
            assert 'region' not in json.dumps(declined), declined
            # Nothing from _meta but its approval kind, request type and the
            # tool's name or title: never its arguments or description (review
            # of L3, round 2, HR-4).
            assert 'SENTINEL' not in json.dumps(declined), declined
            assert declined[0]['tool'] == 'Region picker', declined
            carried = carried_declines(case)
            assert 'SENTINEL' not in json.dumps(carried), carried
            assert [(d['method'], d['mode']) for d in carried] == \
                [('mcpServer/elicitation/request', 'form')], carried
            wire = answered_once(case)
            assert [m['error']['code'] for m in wire] == [-32000] and \
                all(m['result'] is None for m in wire), wire
            return dict(outcome='pass', declined=declined[0]['mode'], actions=0, exit=final['exit'])
        if name == 'url_elicitation_with_approval_kind_declined':
            # A url-mode elicitation whose server-written `_meta` claims the
            # tool-call approval kind: only a form is one, so PIO declines it
            # natively and never surfaces it (review of L3, round 4, R4-HC-4).
            response, _ = case.submit()
            assert response['result']['outcome']['admission'] == 'admitted', response
            poll(lambda: case.inspect()['result'],
                 lambda v: v['runtime'] in ('exited', 'requires_action'), 30)
            assert events_of(case, 'action_requested') == [], 'surfaced a url-mode elicitation'
            final = exited(case)
            declined = events_of(case, 'native_request_declined')
            assert [(d['method'], d['mode'], d['approval_kind']) for d in declined] == \
                [('mcpServer/elicitation/request', 'url', 'mcp_tool_call')], declined
            assert 'does not recognise as a tool-call approval' in declined[0]['reason'], declined
            assert 'example.invalid' not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['mode'], d['approval_kind']) for d in carried] == [('url', 'mcp_tool_call')]
            wire = answered_once(case)
            assert [m['error']['code'] for m in wire] == [-32000], wire
            return dict(outcome='pass', declined=declined[0]['mode'], actions=0, exit=final['exit'])
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
            assert declined[0]['permission_kinds'] == ['fileSystem'], declined
            assert str(case.root) not in json.dumps(declined), declined
            carried = carried_declines(case)
            assert [(d['method'], d['permission_kinds']) for d in carried] == \
                [('item/permissions/requestApproval', ['fileSystem'])], carried
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
            reason = None if decision == 'decline' else 'labeled fake approval request'
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
                    for a in approval] == [(expected[0], reason, False)], approval
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
