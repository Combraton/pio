#!/usr/bin/env python3
"""Owner-authorized live Codex runs for M2 (issue #5 plan R1-R6).

Drives the user's installed Codex through `pio serve-codex` exactly as the
plan posted on issue #5 states. Each run uses its own private PIO store and a
throwaway fixture repository under $HOME/pio-m2-live/fixtures. Raw transcripts,
task output, configuration copies and paths stay under $HOME/pio-m2-live/private
(mode 0700). The public receipt written to docs/work/m2/codex-live/<run>.json
holds digests, identities and observed facts only.

Stops (owner plan): no run starts if cumulative observed Codex usage has reached
the stop, 80% of the Codex cap; a run whose observed usage exceeds its own limit
is interrupted with `execution.cancel`; a run that ends without a usage report
stops the sequence. The cap was 1,000,000 with its stop at 800,000; the owner
raised it on 2026-09-26 to 1,090,000, stop 872,000, for L3 ("More headroom"),
and again on 2026-09-26 to 1,135,000, stop 908,000, for L3's second attempt
(Q8, "Raise for attempt 2": "Codex cap 1,135,000, stop 908,000 (457,576 +
450,000 = 907,576). Limits per run unchanged."), and again on 2026-09-26 to
1,210,000, stop 968,000, when L3's child ceiling rose to 90,000 (Q9, "Child
90k": "Codex cap 1,210,000 / stop 968,000 (457,576 + 510,000 = 967,576).").

Model (owner decision, 2026-09-19): R1 runs with the user's configuration
untouched and no model passed, under a 50,000 token limit and with no retry on
failure, because the configured model is expensive. R2 to R6 pass
`gpt-5.6-terra` explicitly through the service configuration, which the service
accepts only under the dated test-only exception. `--run model-list` confirms
that model exists for the account without starting a turn.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sqlite3
import subprocess
import sys
import time
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parent))
from public_api import Client, command  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
HOME = Path(os.environ['HOME'])
LIVE = HOME / 'pio-m2-live'
CONTENT = 'pio.combraton.dev/content'
FEATURES = ['execution.controller', 'execution.output', 'execution.discovery', 'execution.workspaces', 'execution.usage', 'execution.actions', 'execution.steering']
# The Codex cap and its stop, for every Codex run: owner decision,
# 2026-09-26 (Q9, "Child 90k", for L3's second attempt with a child ceiling
# of 90,000): "Codex cap 1,210,000 / stop 968,000 (457,576 + 510,000 =
# 967,576)." Raised from 1,135,000 and 908,000 (Q8, "Raise for attempt 2",
# the same day), which had raised 1,090,000 and 872,000 ("More headroom",
# the same day), which had raised 1,000,000 and 800,000.
CAP = 1_210_000
STOP_AT = 968_000
RUN_LIMIT = 250_000
# Must equal `pio_protocol::stream::MODEL_EXCEPTION`.
MODEL_EXCEPTION = 'owner-2026-09-19-m2-fixture-runs'
DEADLINE = 600
DELIVERY = 120

CALC = 'def subtract(a, b):\n    return a - b\n'
TEST = '''import unittest

from calc import add, subtract


class CalcTest(unittest.TestCase):
    def test_add(self):
        self.assertEqual(add(2, 3), 5)

    def test_subtract(self):
        self.assertEqual(subtract(5, 3), 2)


if __name__ == "__main__":
    unittest.main()
'''
IMPLEMENT = 'implement the add(a, b) function that test_calc.py expects in calc.py, run python3 -m unittest -q, and reply with the test result in one line.'
# Owner decision of 2026-09-19: R1 proves the "harness as configured" route with
# the user's own model and nothing passed, once, under its own 50,000 token stop.
# R2 to R6 pass `gpt-5.6-terra` explicitly, as a dated test-only exception.
TERRA = 'gpt-5.6-terra'
R1_LIMIT = 50_000
RUNS = {
    'R1': dict(fixture='r1-j1', journeys=['J1', 'J5'], approval='on-request', model=None, limit=R1_LIMIT,
               brief='In this repository, test_calc.py expects an add(a, b) function in calc.py. Implement it, run python3 -m unittest -q, and reply with the test result in one line.'),
    'R2': dict(model=TERRA, limit=RUN_LIMIT, fixture='r2-j3', journeys=['J3'], approval='on-request', brief='Run sleep 20 first. Then, in this repository, ' + IMPLEMENT),
    'R3': dict(model=TERRA, limit=RUN_LIMIT, fixture='r3-j4-interrupt', journeys=['J4'], approval='on-request', brief='Run sleep 120 and then reply with the word done.'),
    'R4': dict(model=TERRA, limit=RUN_LIMIT, fixture='r4-j4-steer', journeys=['J4'], approval='on-request', brief='Run sleep 30 and then reply with the single word alpha.',
               steer='Reply with the single word beta instead.'),
    'R5': dict(model=TERRA, limit=RUN_LIMIT, fixture='r5-deny', journeys=['approval-deny'], approval='untrusted', brief='Run python3 -m unittest -q and reply with the result in one line.', decision='decline'),
    'R6': dict(model=TERRA, limit=RUN_LIMIT, fixture='r6-allow', journeys=['approval-allow'], approval='untrusted', brief='Run python3 -m unittest -q and reply with the result in one line.', decision='accept'),
}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def digest(data):
    return 'sha256:' + sha(data)


def private_dir(*parts):
    path = LIVE.joinpath('private', *parts)
    path.mkdir(parents=True, exist_ok=True)
    for p in [LIVE, LIVE / 'private', path]:
        os.chmod(p, 0o700)
    return path


class LiveClient(Client):
    def query(self, operation, payload):
        if operation == 'core.negotiate':
            payload = dict(payload, profiles=[dict(name='core', majors=[1], required=True, required_features=['core.events', 'core.capabilities', 'core.effects'], optional_features=[]),
                                             dict(name='execution', majors=[1], required=True, required_features=FEATURES, optional_features=[])])
        return super().query(operation, payload)


def ledger_path():
    return private_dir() / 'usage-ledger.json'


def ledger():
    path = ledger_path()
    return json.loads(path.read_text()) if path.exists() else {'cap': CAP, 'stop_at': STOP_AT, 'runs': {}}


def cumulative(book):
    return sum(r.get('tokens') or 0 for r in book['runs'].values())


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), '-c', 'user.email=pio-fixture@example.invalid', '-c', 'user.name=pio fixture', *args], check=True, capture_output=True, text=True).stdout.strip()


def make_fixture(name):
    fixtures = LIVE / 'fixtures'
    fixtures.mkdir(parents=True, exist_ok=True)
    repo = fixtures / name
    if repo.exists():
        shutil.rmtree(repo)
    repo.mkdir()
    (repo / 'calc.py').write_text(CALC)
    (repo / 'test_calc.py').write_text(TEST)
    git(repo, 'init', '-q')
    git(repo, 'add', '.')
    git(repo, 'commit', '-q', '-m', 'PIO M2 live fixture')
    return repo, git(repo, 'rev-parse', 'HEAD')


class Service:
    def __init__(self, run, approval, executable, model=None, limit=RUN_LIMIT, store=None):
        self.run = run
        self.limit = limit
        self.private = private_dir(run)
        self.store = store or LIVE / 'stores' / f'{run}-{uuid.uuid4().hex[:8]}'
        self.store.parent.mkdir(parents=True, exist_ok=True)
        os.chmod(self.store.parent, 0o700)
        self.socket_dir = private_dir(run, 'socket')
        self.socket = self.socket_dir / 'public.sock'
        self.transcript = self.private / 'public-transcript.jsonl'
        env = {'PATH': f'{HOME}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(HOME), 'USER': os.environ.get('USER', ''), 'LANG': 'en_US.UTF-8'}
        protocol = dict(format='combraton-conformance-config/1', principal='owner', credentials=[dict(credential='ccred1.owner.' + base64.urlsafe_b64encode(os.urandom(32)).decode().rstrip('='))], executor=dict(host_id='codex-host'))
        self.credential = protocol['credentials'][0]['credential']
        thread = dict(sandbox='workspace-write', approvalPolicy=approval)
        codex = dict(executable=str(executable), env=env, codex_home=str(HOME / '.codex'), fixture_root=str(LIVE / 'fixtures'),
                     thread=thread, labeled_fake=False)
        if model:
            # The service refuses a model unless the configuration names the
            # owner's dated exception, so both appear together or not at all.
            thread['model'] = model
            codex['test_only_model_exception'] = MODEL_EXCEPTION
        self.config = dict(format='pio-codex-service/1', protocol=protocol, codex=codex)
        self.config_path = self.private / 'service.json'
        self.config_path.write_text(json.dumps(self.config))
        os.chmod(self.config_path, 0o600)
        self.daemons = []

    def start(self, expect_ready=True, timeout=180):
        n = len(self.daemons)
        stdout = (self.private / f'daemon-{n}.stdout').open('w')
        stderr = (self.private / f'daemon-{n}.stderr').open('w')
        daemon = subprocess.Popen([str(BINARY), 'serve-codex', '--data-dir', str(self.store), '--config', str(self.config_path), '--socket', str(self.socket)], stdout=stdout, stderr=stderr)
        self.daemons.append(daemon)
        if not expect_ready:
            return daemon
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if daemon.poll() is not None:
                raise RuntimeError(f'service exited {daemon.returncode}: {(self.private / f"daemon-{n}.stderr").read_text()[-2000:]}')
            try:
                with self.client() as c:
                    if 'result' in c.query('core.describe', {}):
                        return daemon
            except (OSError, ValueError):
                time.sleep(0.5)
        raise RuntimeError('service not ready')

    def client(self):
        import public_api
        public_api.CREDENTIAL = self.credential
        return LiveClient(self.socket, self.transcript, 60)

    def inspect(self, identity):
        with self.client() as c:
            return c.query('execution.inspect', {'execution': identity})['result']

    def call(self, envelope):
        with self.client() as c:
            return c.call(envelope)

    def journal(self):
        with sqlite3.connect(f'file:{self.store}/journal.sqlite3?mode=ro', uri=True) as db:
            invocations = [json.loads(r[0]) for r in db.execute('select state from invocations')]
            row = db.execute("select value from protocol_projection where key='execution/work'").fetchone()
        return invocations, (json.loads(row[0]) if row else None)

    def events(self):
        records = []
        for path in self.store.glob('codex-*.events.jsonl'):
            records += [json.loads(l) for l in path.read_text().splitlines()]
        return records

    def stop(self, daemon, sig=signal.SIGTERM):
        if daemon.poll() is None:
            daemon.send_signal(sig)
            try:
                daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=5)


def envelope(operation, identity, payload, command_id, revision=0, content=None, media_type=None):
    env = command(operation, dict(kind='execution.execution', id=identity), payload, command_id=command_id, revision=revision)
    if content is not None:
        env['extensions'] = {CONTENT: dict(media_type=media_type, text=content)}
    return env


def usage_total(events):
    totals = [e['total']['totalTokens'] for e in events if e['kind'] == 'usage' and isinstance(e.get('total'), dict)]
    return max(totals) if totals else None


def wait(service, predicate, seconds, identity='work'):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        last = service.inspect(identity)
        if predicate(last):
            return last
        total = usage_total(service.events())
        if total is not None and total > service.limit and not service.__dict__.get('limit_cancelled'):
            service.limit_cancelled = True
            cancel(service, 'usage-limit-cancel')
        time.sleep(1)
    raise TimeoutError(f'bounded wait expired; last runtime={last and last.get("runtime")}')


def cancel(service, command_id):
    view = service.inspect('work')
    return service.call(envelope('execution.cancel', 'work', {}, command_id, revision=view['revision']))


def processes(pattern):
    table = subprocess.check_output(['ps', '-axww', '-o', 'pid=', '-o', 'command='], text=True)
    return [line.strip() for line in table.splitlines() if pattern in line]


def receipt(service, run, spec, repo, base, started, extra):
    invocations, record = service.journal()
    events = service.events()
    kinds = [e['kind'] for e in events]
    first = lambda kind: next((e for e in events if e['kind'] == kind), None)
    view = service.inspect('work')
    with service.client() as c:
        output = c.query('execution.output.read', {'execution': 'work', 'offset': 0, 'max_bytes': 1048576})['result']
    out_bytes = base64.b64decode(output['data_base64'])
    (service.private / 'output.jsonl').write_bytes(out_bytes)
    agent_text = ''.join(json.loads(l)['params'].get('delta', '') for l in out_bytes.decode(errors='replace').splitlines()
                         if l.strip() and json.loads(l)['method'] == 'item/agentMessage/delta')
    (service.private / 'agent-text.txt').write_text(agent_text)
    qualification = json.loads((service.store / 'qualification.json').read_text())
    resolution = qualification['resolution']
    invocation = invocations[0] if invocations else {}
    thread = first('thread_started') or {}
    guard = (first('thread_settings_guard') or {}).get('guard', {})
    tests = subprocess.run([sys.executable, '-m', 'unittest', '-q'], cwd=repo, capture_output=True, text=True)
    diff_stat = git(repo, 'diff', '--stat', base)
    status = git(repo, 'status', '--porcelain')
    check = subprocess.run([str(BINARY), 'check-transcript', str(service.transcript)], capture_output=True, text=True)
    for path in service.store.glob('codex-*'):
        shutil.copy2(path, service.private / path.name)
    usage = usage_total(events)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    return dict(
        format='pio-m2-live-receipt/1', run=run, journeys=spec['journeys'], real_codex=True, live=True,
        recorded_at_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()), seconds=round(time.monotonic() - started, 1),
        pio=dict(head=head, dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)), binary_sha256=sha(BINARY.read_bytes())),
        platform=platform.platform(),
        codex=dict(selected='$HOME/.local/bin/codex', kind=resolution['kind'], wrapper_sha256=resolution['wrapper']['sha256'], wrapper_package_version=resolution['wrapper']['package_version'],
                   native_sha256=resolution['native']['sha256'], native_layout_version=resolution['native']['layout'].get('version'), node=dict(sha256=resolution['node']['sha256'], version=resolution['node']['version']),
                   version=qualification['version'], canonical_schema_listing_sha256=qualification['schema']['canonical_listing_sha256'], schema_drift=qualification['schema']['drift_count'],
                   service_node_resolution=dict(matches_qualification=qualification['service_node_resolution']['matches_qualification'], version=qualification['service_node_resolution']['version'])),
        environment_passed=sorted(service.config['codex']['env']), credential_variables_passed=False,
        authentication_type=(first('account') or {}).get('authentication_type'),
        model=dict(configured=thread.get('configured_model'), requested=thread.get('requested_model'), effective=thread.get('model')),
        model_provider=thread.get('model_provider'),
        thread_settings=dict(requested=guard.get('requested'), configured=guard.get('configured'), guard_allowed=guard.get('allowed'),
                             effective_sandbox=thread.get('sandbox'), effective_approval_policy=thread.get('approval_policy')),
        native=dict(thread_id=thread.get('thread_id'), turn_id=(first('turn_acknowledged') or {}).get('turn_id'), native_process=((first('spawned') or {}).get('native'))),
        identities=dict(invocation_id=invocation.get('invocation_id'), host=invocation.get('host'), app_server_process=invocation.get('child'), host_slot=invocation.get('host_slot'),
                        controller_generation_at_admission=invocation.get('controller_generation'), host_generation_now=view['host']['generation'], phase=invocation.get('phase')),
        delivery=view['deliveries'][0] if view.get('deliveries') else None,
        runtime=view['runtime'], exit=view.get('exit'), cancellation=view.get('cancellation'), actions=view.get('actions'), steering=view.get('steering'),
        deadline_stop=(record or {}).get('codex', {}).get('deadline_stop'), timeouts=dict(delivery=DELIVERY, execution_deadline=DEADLINE), timeouts_passed=(record or {}).get('timeouts_passed'),
        turn_status=(first('turn_completed') or {}).get('status'), turn_error=(first('turn_completed') or {}).get('error'),
        output=dict(bytes=len(out_bytes), sha256=sha(out_bytes), receipt_output_digest=(invocation.get('receipt') or {}).get('output_digest'), agent_text_sha256=sha(agent_text.encode())),
        usage=dict(observed_total_tokens=usage, observation=view['usage'], cap=CAP, stop_at=STOP_AT, run_limit=service.limit),
        config_diff=(first('config_after') or {}).get('diff'), config_before_sha256=((first('config_before') or {}).get('snapshot') or {}).get('raw_sha256'),
        fixture=dict(path=f'$HOME/pio-m2-live/fixtures/{spec["fixture"]}', base=base, brief_sha256=sha(spec['brief'].encode()), diff_stat=diff_stat, status_porcelain_lines=len(status.splitlines()),
                     unittest_exit_after_run=tests.returncode),
        event_kinds=kinds, public_transcript_schema_check=dict(exit=check.returncode, summary=check.stdout.strip() or check.stderr.strip()[-300:]),
        independent_services=dict(cbr_processes=len(processes('cbr ')), combraton_processes=len(processes('combraton')), context_calls='none: PIO has no Context client in this build'),
        **extra)


def run_live(run, args):
    spec = RUNS[run]
    book = ledger()
    if cumulative(book) >= STOP_AT:
        raise SystemExit(f'stop: cumulative observed usage {cumulative(book)} reached {STOP_AT}')
    repo, base = make_fixture(spec['fixture'])
    service = Service(run, spec['approval'], args.executable, spec['model'], spec['limit'])
    daemon = service.start()
    started = time.monotonic()
    brief = spec['brief'].encode()
    payload = dict(brief=dict(digest=digest(brief), media_type='text/plain'), workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                   timeouts=dict(delivery=DELIVERY, execution_deadline=DEADLINE))
    submitted = service.call(envelope('execution.submit', 'work', payload, 'work', content=spec['brief'], media_type='text/plain'))
    extra = dict(submit=dict(admission=submitted.get('result', {}).get('outcome', {}).get('admission'), error=submitted.get('error', {}).get('data')))
    if 'error' in submitted or submitted['result']['outcome']['admission'] != 'admitted':
        raise RuntimeError(f'submit not admitted: {submitted}')
    try:
        if run == 'R2':
            wait(service, lambda v: v['delivery'] == 'acknowledged', 180)
            time.sleep(5)
            before_invocation, _ = service.journal()
            before_view = service.inspect('work')
            service.stop(daemon, signal.SIGKILL)
            killed_at = time.monotonic()
            host_alive = bool(processes(f'codex host {service.store}'))
            daemon = service.start()
            replay = service.call(envelope('execution.submit', 'work', payload, 'work', content=spec['brief'], media_type='text/plain'))
            wait(service, lambda v: v['runtime'] == 'exited', DEADLINE + 120)
            after_invocation, _ = service.journal()
            after_view = service.inspect('work')
            sent = [e for e in service.events() if e['kind'] == 'turn_start_sent']
            extra['j3'] = dict(daemon_killed='SIGKILL', restart_seconds=round(time.monotonic() - killed_at, 1), host_process_alive_during_restart=host_alive,
                               host_before=before_invocation[0]['host'], host_after=after_invocation[0]['host'], app_server_before=before_invocation[0]['child'], app_server_after=after_invocation[0]['child'],
                               same_host_and_app_server=before_invocation[0]['host'] == after_invocation[0]['host'] and before_invocation[0]['child'] == after_invocation[0]['child'],
                               invocations=len(after_invocation), turn_start_sent=len(sent), replay=replay.get('result', {}).get('replay'),
                               generation_before=before_view['host']['generation'], generation_after=after_view['host']['generation'], recovery=after_view.get('recovery'))
        elif run == 'R3':
            wait(service, lambda v: v['delivery'] == 'acknowledged', 180)
            time.sleep(15)
            response = cancel(service, 'cancel')
            wait(service, lambda v: v['runtime'] == 'exited', 300)
            extra['j4_cancel'] = dict(cancel_command=('result' in response), cancel_error=response.get('error', {}).get('data'))
        elif run == 'R4':
            wait(service, lambda v: v['delivery'] == 'acknowledged', 180)
            time.sleep(8)
            view = service.inspect('work')
            message = spec['steer'].encode()
            response = service.call(envelope('execution.steer', 'work', dict(message=dict(digest=digest(message), media_type='text/plain')), 'steer', revision=view['revision'], content=spec['steer'], media_type='text/plain'))
            wait(service, lambda v: v['runtime'] == 'exited', DEADLINE + 60)
            extra['j4_steer'] = dict(steer_request=response.get('result', {}).get('outcome'), steer_error=response.get('error', {}).get('data'), steer_text_sha256=sha(message))
        elif run in ('R5', 'R6'):
            view = wait(service, lambda v: v['runtime'] in ('requires_action', 'exited'), 300)
            if view['runtime'] == 'requires_action':
                action = view['runtime_detail']['action_id']
                body = json.dumps({'decision': spec['decision']}).encode()
                response = service.call(envelope('execution.respond_action', 'work', dict(action_id=action, response=dict(digest=digest(body), media_type='application/json')), 'answer',
                                                 revision=view['revision'], content=body.decode(), media_type='application/json'))
                extra['approval'] = dict(action_id=action, decision=spec['decision'], answer=response.get('result', {}).get('outcome'), answer_error=response.get('error', {}).get('data'),
                                         request=next((e for e in service.events() if e['kind'] == 'action_requested'), None))
                wait(service, lambda v: v['runtime'] in ('requires_action', 'exited'), DEADLINE + 60)
                later = service.inspect('work')
                if later['runtime'] == 'requires_action':
                    extra['approval']['further_request'] = later['runtime_detail']
                    body = json.dumps({'decision': 'decline'}).encode()
                    service.call(envelope('execution.respond_action', 'work', dict(action_id=later['runtime_detail']['action_id'], response=dict(digest=digest(body), media_type='application/json')),
                                          'answer-further-decline', revision=later['revision'], content=body.decode(), media_type='application/json'))
                    wait(service, lambda v: v['runtime'] == 'exited', DEADLINE)
                extra['approval']['item_completed'] = [e for e in service.events() if e['kind'] == 'item_completed']
            else:
                extra['approval'] = dict(requested=False, note='no native approval request arrived; this run does not prove deny/allow')
        else:
            wait(service, lambda v: v['runtime'] == 'exited', DEADLINE + 60)
        invocations = None
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            invocations, _ = service.journal()
            if invocations and invocations[0]['phase'] in ('completed', 'known_not_released'):
                break
            time.sleep(1)
        result = receipt(service, run, spec, repo, base, started, extra)
    finally:
        for d in service.daemons:
            service.stop(d)
    usage = result['usage']['observed_total_tokens']
    book['runs'][run] = dict(tokens=usage, basis='observed' if usage is not None else 'unknown', recorded_at_utc=result['recorded_at_utc'])
    ledger_path().write_text(json.dumps(book, indent=2) + '\n')
    result['usage']['cumulative_observed_tokens'] = cumulative(book)
    out = ROOT / 'docs/work/m2/codex-live'
    out.mkdir(parents=True, exist_ok=True)
    text = json.dumps(result, indent=2) + '\n'
    for secret in (str(HOME), service.credential):
        text = text.replace(secret, '$HOME' if secret == str(HOME) else '<credential redacted>')
    (out / f'{run}.json').write_text(text)
    print(text)
    if usage is None:
        raise SystemExit('stop: run ended without a usage report (unknown liability)')
    return result


def discovery(args):
    """Zero-token discovery, before and after a launch has observed anything.

    J1 and J5 both start by discovering installations. Discovery never starts an
    app-server, so this costs nothing. It runs twice: once against a fresh store,
    where authentication is honestly unknown and the installation is therefore
    not usable, and once against the store a completed run left behind, where a
    launch did observe authentication.
    """
    stores = sorted((LIVE / 'stores').glob('R6-*'), key=lambda p: p.stat().st_mtime)
    assert stores, 'no completed R6 store to read an observed authentication from'
    records = {}
    for name, store in (('fresh_store', None), ('store_of_a_completed_run', stores[-1])):
        service = Service(f'discovery-{name}', 'on-request', args.executable, TERRA, RUN_LIMIT, store)
        before = sorted(p.name for p in service.store.glob('codex-*.events.jsonl')) if store else []
        try:
            service.start()
            with service.client() as c:
                listed = c.query('execution.discovery.list', {})['result']
        finally:
            for d in service.daemons:
                service.stop(d)
        after = sorted(p.name for p in service.store.glob('codex-*.events.jsonl'))
        records[name] = dict(installations=len(listed['installations']), installation=listed['installations'][0],
                             launch_event_files_before=len(before), launch_event_files_after=len(after),
                             discovery_started_no_app_server=before == after)
    result = dict(format='pio-m2-live-receipt/1', run='discovery', live=True, turn_started=False, model_calls=0,
                  recorded_at_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                  pio=dict(head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                           dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))),
                  **records)
    out = ROOT / 'docs/work/m2/codex-live'
    out.mkdir(parents=True, exist_ok=True)
    (out / 'discovery.json').write_text(json.dumps(result, indent=2).replace(str(HOME), '$HOME') + '\n')
    print(json.dumps(result, indent=2))


def wrong_executable(args):
    service = Service('wrong-executable', 'on-request', HOME / '.local/bin/opencode2')
    daemon = service.start(expect_ready=False)
    code = daemon.wait(timeout=120)
    stderr = (service.private / 'daemon-0.stderr').read_text()
    record = json.loads((service.store / 'qualification.json').read_text()) if (service.store / 'qualification.json').exists() else {}
    result = dict(format='pio-m2-live-receipt/1', run='wrong-executable', live=True, model_calls=0, selected='$HOME/.local/bin/opencode2', service_exit=code,
                  refused=('codex_not_qualified' in stderr), refusals=record.get('refusals'), socket_created=service.socket.exists())
    out = ROOT / 'docs/work/m2/codex-live'
    out.mkdir(parents=True, exist_ok=True)
    (out / 'wrong-executable.json').write_text(json.dumps(result, indent=2).replace(str(HOME), '$HOME') + '\n')
    print(json.dumps(result, indent=2))


def model_list(args):
    """Zero-token check that the model R2 to R6 request exists for this account.

    Starts the qualified app-server directly against the user's real Codex home,
    sends `initialize` and `model/list`, and sends no turn. Listing models is not
    a model call and reports no usage.
    """
    private = private_dir('model-list')
    work = private / 'qualify'
    code = subprocess.run([str(BINARY), 'codex', 'qualify', '--executable', str(args.executable), '--work', str(work)],
                          capture_output=True, text=True)
    qualification = json.loads(code.stdout)
    assert qualification['qualified'], qualification['refusals']
    env = {'PATH': f'{HOME}/.local/bin:/usr/bin:/bin', 'HOME': str(HOME), 'CODEX_HOME': str(HOME / '.codex')}
    process = subprocess.Popen([str(args.executable), 'app-server'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=(private / 'app-server.stderr').open('wb'), env=env)

    def send(message):
        process.stdin.write((json.dumps(message) + '\n').encode())
        process.stdin.flush()

    def response(request_id, seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            line = process.stdout.readline()
            if not line:
                break
            message = json.loads(line)
            if message.get('id') == request_id and ('result' in message or 'error' in message):
                return message
        raise TimeoutError(f'no response to {request_id}')

    send({'method': 'initialize', 'id': 0, 'params': {'clientInfo': {'name': 'pio_model_list', 'title': 'PIO model list', 'version': '0.1.0-dev'}}})
    response(0, 60)
    send({'method': 'initialized'})
    send({'method': 'model/list', 'id': 1, 'params': {}})
    listed = response(1, 120)
    process.stdin.close()
    process.wait(timeout=30)
    models = [m.get('model') or m.get('id') for m in listed.get('result', {}).get('data', [])]
    result = dict(format='pio-m2-live-receipt/1', run='model-list', live=True, turn_started=False, model_calls=0,
                  recorded_at_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                  codex_version=qualification['version'], requested_model=TERRA, available=sorted(set(filter(None, models))),
                  requested_model_available=TERRA in models, error=listed.get('error'))
    out = ROOT / 'docs/work/m2/codex-live'
    out.mkdir(parents=True, exist_ok=True)
    (out / 'model-list.json').write_text(json.dumps(result, indent=2).replace(str(HOME), '$HOME') + '\n')
    print(json.dumps(result, indent=2))
    if not result['requested_model_available']:
        raise SystemExit(f'stop: {TERRA} is not in the account model list')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', required=True, choices=[*RUNS, 'wrong-executable', 'model-list', 'discovery'])
    parser.add_argument('--executable', type=Path, default=HOME / '.local/bin/codex')
    args = parser.parse_args()
    if args.run == 'model-list':
        model_list(args)
    elif args.run == 'discovery':
        discovery(args)
    elif args.run == 'wrong-executable':
        wrong_executable(args)
    else:
        run_live(args.run, args)


if __name__ == '__main__':
    main()
