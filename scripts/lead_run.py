#!/usr/bin/env python3
"""L1 — a lead that starts runs, through one `serve-opencode` service.

Owner approvals `owner-2026-09-22-m4b-lead-tool` and 2026-09-23 (L1, the
amended plan), posted on issue #12: one service; the lead and both led runs on
`minimax-coding-plan/MiniMax-M3`; call budget 2; cap 2,000,000 for the lead
sequence with its stop at 1,600,000 read from **charged**; rehearsal against
the labeled fake first; the owner at the desk for the live run.

**One code path.** `--rehearse` and the live run differ in the harness binary
and its environment and in nothing else a row depends on. The labeled fake
launches the lead tool the way OpenCode 2.0.11 was measured doing
(`lead_tool_probe.py`) and scripts the calls a model would make; the live run
gives the same tool to the owner's own OpenCode. The first version of this
runner had a rehearsal branch and an unbuilt live branch, and its rehearsal's
lead was never admitted; both are why it was reverted.

**Every row carries its expected value, and a mismatch fails the run.** A row
that cannot be observed in a rehearsal says so and is not counted as proven.

What only the live run can show: that a real model uses the tool at all,
that what the runs report is what the files say, and what the owner's OpenCode
does with a permission prompt.

`--mutant` (rehearsal only) changes one thing and must fail a named row:

- `no-tool` submits the lead without the tool, so the host sends
  `mcpServers: []` and no child is ever started;
- `reports-refused-as-started` gives the tool the bug the first rehearsal
  caught: a refused admission reported as a start;
- `grant-may-answer` puts `execution.respond_action` in the lead's grant;
- `grant-no-steer` takes `execution.steer` out of it;
- `wrong-child` has each led run report one line too many;
- `wrong-relay` has the lead relay one line too many;
- `lead-without-brief` sends the lead's brief without its bytes, which is how
  the first rehearsal's lead came to be refused.
"""
import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
import opencode_host_matrix as matrix
import opencode_live_run as live
from approval_desk import ASKS
from board_fold import Caller
from check_private_paths import redact
from lead_tool import message_text
from public_api import command

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'
TOOL = ROOT / 'scripts' / 'lead_tool.py'
LEAD = 'L1'
SEQUENCE = 'M4-lead-opencode'
MODEL = 'minimax-coding-plan/MiniMax-M3'
PROVIDER = 'conformance-provider'
BUDGET = 2
SEQUENCE_CAP = 2_000_000
SEQUENCE_STOP = 1_600_000
DELIVERY_TIMEOUT = 300
EXECUTION_DEADLINE = 900
FEATURES = ('core.events', 'core.capabilities', 'core.effects', 'core.grants')
EXECUTION_FEATURES = (*matrix.FEATURES, 'execution.steering')
CONTENT = 'pio.combraton.dev/content'
LEAD_TOOL = 'pio.combraton.dev/lead-tool'
UNDER_GRANT = 'pio.combraton.dev/under-grant'
APPROVAL = 'pio.combraton.dev/approval'
DECISION = 'pio.combraton.dev/decision'
FILES = {'alpha.md': 7, 'beta.md': 4}
CHILDREN = {name.split('.')[0]: name for name in FILES}


def child_brief(name):
    return f'Report the number of lines in {name}. Answer with the number alone.'


BRIEF = ('There are two files in this workspace, alpha.md and beta.md. Use the '
         'pio-lead tool, and nothing else, to do this. Call start_run twice: '
         "name 'alpha' with the brief '" + child_brief('alpha.md') + "', and name "
         "'beta' with the brief '" + child_brief('beta.md') + "'. Then call "
         'read_run for alpha and for beta, again until its runtime is exited. Do '
         'not read the files yourself and do not start any other run. Finish '
         'with exactly two lines, alpha.md: <number> and beta.md: <number>, using '
         'the numbers the two runs reported.')

MUTANTS = {
    'no-tool': 'Only the lead got the tool',
    'reports-refused-as-started': 'A third start is refused by PIO',
    'grant-may-answer': 'The lead may not answer an approval',
    'grant-no-steer': 'A steer while the child runs',
    'wrong-child': 'Each child reported the true count',
    'wrong-relay': 'The lead relayed the true counts',
    'lead-without-brief': 'The lead was admitted',
}


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def private(path):
    """A directory only this user can enter; the service refuses anything else."""
    path.mkdir(parents=True, exist_ok=True)
    os.chmod(path, 0o700)
    return path


def fixture(root):
    """The workspace the lead and its children share. Counts are the runner's."""
    repo = private(root / 'fixtures') / 'lead'
    repo.mkdir()
    for name, lines in FILES.items():
        (repo / name).write_text(''.join(f'line {n}\n' for n in range(1, lines + 1)))
    (repo / 'README.md').write_text('L1 fixture. A throwaway repository.\n')
    git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True,
                                    capture_output=True, text=True).stdout.strip()
    git('init', '-q')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
        'commit', '-q', '-m', 'fixture')
    return repo, git('rev-parse', 'HEAD')


def wc_l(repo):
    """What the files say, counted by the runner and nobody else."""
    return {name: len((repo / name).read_text().splitlines()) for name in FILES}


def fresh_credential(principal):
    return f'ccred1.{principal}.' + base64.urlsafe_b64encode(
        os.urandom(32)).decode().rstrip('=')


class Rows:
    """Every observation, its expected value, and whether they agree."""

    def __init__(self, rehearse):
        self.rehearse = rehearse
        self.rows = []

    def add(self, name, observed, expected, holds=None, live_only=False, note=''):
        agrees = holds(observed) if holds else observed == expected
        provable = not (live_only and self.rehearse)
        self.rows.append(dict(row=name, observed=observed, expected=expected,
                              holds=bool(agrees) if provable else None,
                              proven=bool(agrees) and provable, note=note))

    def failed(self):
        return [r['row'] for r in self.rows if r['holds'] is False]


class Service:
    """One `serve-opencode` for the lead and its children, rehearsed or live."""

    def __init__(self, root, rehearse, scenario):
        self.root = root
        self.rehearse = rehearse
        self.store = root / 'store'
        self.socket = private(root / 'socket') / 'public.sock'
        if len(str(self.socket).encode()) > 100:
            raise SystemExit(f'socket path too long for the platform: {self.socket}')
        self.owner_credential = fresh_credential('owner')
        self.lead_credential = fresh_credential('lead')
        if rehearse:
            home = root
            executable = root / 'fake-opencode'
            executable.write_text(f"#!/bin/sh\nexec '{BINARY}' opencode fake-acp \"$@\"\n")
            executable.chmod(0o755)
            config_dir = private(root / 'opencode-config')
            env = {'PATH': '/usr/bin:/bin', 'HOME': str(home),
                   'USER': os.environ.get('USER', 'pio'),
                   'PIO_OPENCODE_FAKE_SCENARIO': json.dumps(scenario)}
        else:
            # The owner's OpenCode, exactly as they configured it. PIO passes
            # no key: OpenCode authenticates itself.
            home = live.HOME
            executable = Path(shutil.which('opencode2')
                              or str(home / '.local/bin/opencode2'))
            config_dir = home / '.config/opencode'
            env = {'PATH': f'{home}/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin',
                   'HOME': str(home), 'USER': os.environ.get('USER', '')}
        self.config = dict(
            format='pio-opencode-service/1',
            protocol=dict(format='combraton-conformance-config/1', principal='owner',
                          provider_id=PROVIDER,
                          credentials=[dict(credential=self.owner_credential),
                                       dict(credential=self.lead_credential)],
                          executor=dict(host_id='opencode-host')),
            opencode=dict(executable=str(executable), env=env,
                          config_dir=str(config_dir), home=str(home),
                          fixture_root=str(root / 'fixtures'),
                          labeled_fake=rehearse, model=MODEL,
                          test_only_model_exception=live.MODEL_EXCEPTION))
        self.config_path = root / 'service.json'
        self.config_path.write_text(json.dumps(self.config))
        os.chmod(self.config_path, 0o600)
        self.daemon = None

    def start(self):
        out = (self.root / 'daemon.stdout').open('w')
        err = (self.root / 'daemon.stderr').open('w')
        self.daemon = subprocess.Popen(
            [str(BINARY), 'serve-opencode', '--data-dir', str(self.store),
             '--config', str(self.config_path), '--socket', str(self.socket)],
            stdout=out, stderr=err)
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            try:
                owner = self.owner()
                owner.close()
                return
            except (OSError, ValueError, KeyError, AssertionError):
                time.sleep(0.25)
        raise SystemExit('the service did not become ready; see daemon.stderr')

    def owner(self):
        return Caller(self.socket, self.owner_credential, features=FEATURES,
                      execution_features=EXECUTION_FEATURES)

    def lead(self, grant_id):
        return Caller(self.socket, self.lead_credential, grant=grant_id,
                      features=FEATURES, execution_features=EXECUTION_FEATURES)

    def host_events(self, views, briefs):
        """Each execution's own host events.

        The host names its files by invocation, and the journal's invocation
        records carry a host command hash rather than the execution id. The
        public view links the two where usage was reported
        (`usage.observations[].invocation_id`); otherwise the invocation is
        the one whose launch spec carries that run's brief digest. A run
        neither finds is left out, and the rows that need it fail.
        """
        journal = self.store / 'journal.sqlite3'
        if not journal.exists():
            return {}
        with sqlite3.connect(f'file:{journal}?mode=ro', uri=True) as db:
            states = [json.loads(r[0]) for r in db.execute('select state from invocations')]
        by_run = {}
        for identity, current in views.items():
            observations = ((current or {}).get('usage') or {}).get('observations') or []
            invocation = observations[0]['invocation_id'] if observations else None
            if invocation is None and identity in briefs:
                digest = 'sha256:' + sha(briefs[identity])
                invocation = next((s['invocation_id'] for s in states
                                   if (s.get('payload') or {}).get('brief', {}).get('digest')
                                   == digest), None)
            path = self.store / f'opencode-{invocation}.events.jsonl'
            if invocation and path.exists():
                by_run[identity] = [json.loads(l) for l in path.read_text().splitlines()
                                    if l.strip()]
        return by_run

    def release(self, remove):
        if self.daemon and self.daemon.poll() is None:
            self.daemon.kill()
            self.daemon.wait(timeout=10)
        case_cleanup.release(self.root if remove else self.store, remove=remove)


def view(caller, identity):
    answer = caller.query('execution.inspect', {'execution': identity})
    return answer.get('result')


def events(caller):
    items, payload = [], {'limit': 1000, 'from': 'start', 'kinds': ['execution.execution']}
    while True:
        result = caller.query('core.events.read', payload).get('result')
        assert result is not None, 'core.events.read refused'
        items += [i['event'] for i in result['items'] if 'event' in i]
        if not result.get('has_more'):
            return items
        payload = {'limit': 1000, 'cursor': result['cursor'],
                   'kinds': ['execution.execution']}


def spoken(caller, identity):
    """What a run said, decoded from its spool the way the lead tool does."""
    raw, offset = b'', 0
    while True:
        result = caller.query('execution.output.read', {
            'execution': identity, 'offset': offset, 'max_bytes': 65536}).get('result')
        if not result:
            break
        raw += base64.b64decode(result['data_base64'])
        if result['next_offset'] == offset:
            break
        offset = result['next_offset']
    return message_text(raw.decode('utf-8', 'replace'))


def first_number(text):
    found = re.search(r'\d+', text or '')
    return int(found.group()) if found else None


def relayed(text):
    return {f'{m.group(1)}.md': int(m.group(2))
            for m in re.finditer(r'\b(alpha|beta)\.md\s*:\s*(\d+)', text or '')}


def submit(caller, identity, brief, repo, base, origin, extensions):
    body = brief.encode()
    payload = dict(brief=dict(digest='sha256:' + sha(body), media_type='text/plain'),
                   workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                   timeouts=dict(delivery=DELIVERY_TIMEOUT,
                                 execution_deadline=EXECUTION_DEADLINE))
    if origin is not None:
        payload['origin'] = origin
    envelope = command('execution.submit', dict(kind='execution.execution', id=identity),
                       payload, command_id=identity)
    envelope['extensions'] = extensions
    return caller.call(envelope)


def steer(caller, identity):
    """One steer under the lead's grant; what came back, and who it names."""
    current = view(caller, identity) or {}
    note = b'keep to the fixture'
    envelope = command('execution.steer', dict(kind='execution.execution', id=identity),
                       dict(message=dict(digest='sha256:' + sha(note),
                                         media_type='text/plain')),
                       command_id=f'{identity}.steer-{uuid.uuid4().hex[:6]}',
                       revision=current.get('revision', 0))
    envelope['extensions'] = {CONTENT: dict(media_type='text/plain', text=note.decode())}
    answer = caller.call(envelope)
    if 'error' in answer:
        return dict(refused=answer['error']['data'].get('code'),
                    runtime_at_steer=current.get('runtime'))
    outcome = answer['result'].get('outcome', {})
    return dict(request=outcome.get('request'), alternative=outcome.get('alternative'),
                runtime_at_steer=current.get('runtime'))


def respond(caller, identity, action_id, decision, revision):
    body = json.dumps({'decision': decision}).encode()
    envelope = command('execution.respond_action',
                       dict(kind='execution.execution', id=identity),
                       dict(action_id=action_id,
                            response=dict(digest='sha256:' + sha(body),
                                          media_type='application/json')),
                       command_id=f'{identity}.answer-{action_id}', revision=revision)
    envelope['extensions'] = {CONTENT: dict(media_type='application/json',
                                            text=body.decode())}
    return caller.call(envelope)


class ToolInstance:
    """The lead tool, run by the runner in the model's place for one call.

    The third start goes through the same tool the lead holds, so a tool that
    reports a refused admission as a start fails here as it would for the
    lead. It is made while the lead is still running: an initiator that has
    exited starts nothing, whatever its budget.
    """

    def __init__(self, spec, log):
        env = dict({v['name']: v['value'] for v in spec['env']}, PIO_LEAD_LOG=str(log))
        self.child = subprocess.Popen([spec['command'], *spec['args']], env=env,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      text=True, bufsize=1, start_new_session=True)
        self.next = 0
        self.call('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {},
                                 'clientInfo': {'name': 'lead_run', 'version': '1'}})

    def call(self, method, params):
        self.next += 1
        self.child.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': self.next,
                                           'method': method, 'params': params}) + '\n')
        self.child.stdin.flush()
        while True:
            line = self.child.stdout.readline()
            if not line:
                raise EOFError('the lead tool closed its output')
            answer = json.loads(line)
            if answer.get('id') == self.next:
                return answer

    def tool(self, tool_name, **arguments):
        result = self.call('tools/call', {'name': tool_name, 'arguments': arguments})['result']
        text = result['content'][0]['text']
        return {'error': text} if result.get('isError') else json.loads(text)

    def close(self):
        self.child.stdin.close()
        self.child.wait(timeout=10)


class Desk:
    """Every pending approval, put in front of the person who decides.

    Live, the request is written to `desk/pending-<action>.json` and the
    runner waits for `desk/answer-<action>.json` — the owner's decision,
    relayed by the builder with their words. A rehearsal answers for itself
    and says so. Only `allow` and `deny` are encodable: the host takes the
    single-use option by kind, and an `*_always` option is never selected.
    """

    def __init__(self, directory, rehearse):
        self.directory = private(directory)
        self.rehearse = rehearse
        self.seen = {}

    def relay(self, service, identity, current, stream):
        for action in current.get('actions', []):
            if action['state'] != 'pending' or action['action_id'] in self.seen:
                continue
            approval = next((e['payload'].get(APPROVAL) for e in stream
                             if e['subject']['id'] == identity
                             and e['type'] == 'execution.runtime.changed'
                             and e['payload'].get('action_id') == action['action_id']), None)
            item = dict(run=identity, action_id=action['action_id'],
                        requested_at=action['requested_at'], approval=approval)
            self.seen[action['action_id']] = item
            pending = self.directory / f"pending-{action['action_id']}.json"
            pending.write_text(json.dumps(redact(item), indent=2) + '\n')
            print(f"DESK pending {identity} {action['action_id']} {pending}", flush=True)
            answer = self.answer(action['action_id'])
            item['answer'] = answer
            if answer['decision'] not in ('allow', 'deny'):
                raise SystemExit(f"the desk answered {answer['decision']!r}; "
                                 'only allow and deny are single-use')
            owner = service.owner()
            try:
                sent = respond(owner, identity, action['action_id'], answer['decision'],
                               view(owner, identity)['revision'])
            finally:
                owner.close()
            item['sent'] = sent.get('result', {}).get('outcome', {}).get('state') \
                or sent.get('error', {}).get('data')
            print(f"DESK answered {action['action_id']} {answer['decision']} "
                  f"by {answer['decided_by']}", flush=True)

    def answer(self, action_id):
        path = self.directory / f'answer-{action_id}.json'
        if self.rehearse:
            return dict(decision='allow', decided_by='rehearsal',
                        words='rehearsal: the runner answers; no owner is asked')
        deadline = time.monotonic() + DELIVERY_TIMEOUT - 20
        while time.monotonic() < deadline:
            if path.exists():
                return json.loads(path.read_text())
            time.sleep(0.5)
        return dict(decision=None, decided_by=None,
                    words='no answer reached the desk before the deadline')


def mutated_tool(root):
    """The lead tool with the bug the first rehearsal caught put back."""
    text = TOOL.read_text()
    fix = "    if outcome.get('admission') == 'refused':\n"
    assert text.count(fix) == 1, 'the mutant no longer matches the tool'
    path = root / 'lead_tool_mutant.py'
    path.write_text(text.replace(fix, "    if False:\n"))
    return path


def scenario(mutant):
    """What the labeled fake plays: the calls a model would make, and the
    answers a model would give, neither of them a model."""
    reads = [dict(tool='read_run', arguments=dict(name=short), until='exited',
                  report_as=name) for short, name in CHILDREN.items()]
    starts = [dict(tool='start_run', arguments=dict(name=short, brief=child_brief(name)))
              for short, name in CHILDREN.items()]
    return dict(
        lead=dict(calls=starts + reads,
                  relay_offset=1 if mutant == 'wrong-relay' else 0),
        answer_line_counts=True, led_offset=1 if mutant == 'wrong-child' else 0,
        # Long enough to be steered while it runs, and asked about one thing,
        # so the desk has something to relay.
        led_delay_ms=3000, permission_request=ASKS, ask_in='led',
        usage_total=4096)


def run(args):
    rehearse = args.rehearse
    started_at = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    record = dict(format='pio-lead-run/1', mode='rehearsal' if rehearse else 'live',
                  lead=LEAD, model=MODEL, budget=BUDGET, sequence=SEQUENCE,
                  sequence_cap=SEQUENCE_CAP, sequence_stop=SEQUENCE_STOP,
                  mutant=args.mutant, started_at=started_at, desk=args.desk)
    head = subprocess.run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'],
                          capture_output=True, text=True).stdout.strip()
    dirty = bool(subprocess.run(['git', '-C', str(ROOT), 'status', '--porcelain'],
                                capture_output=True, text=True).stdout.strip())
    record.update(commit=head, dirty=dirty)
    names = {LEAD: f'{args.attempt}/{LEAD}' if args.attempt else LEAD}
    names.update({f'{LEAD}.{c}': f'{names[LEAD]}.{c}' for c in CHILDREN})
    if rehearse:
        record['preflight'] = {'checked': False, 'reason': 'rehearsal'}
        root = Path(tempfile.mkdtemp(prefix='pio-l1-', dir='/tmp')).resolve()
        os.chmod(root, 0o700)
    else:
        # A clean tree, a binary built from it, and its digest. The build is
        # the runner's own, so "built from HEAD" is not an inference.
        subprocess.run(['cargo', 'build', '--locked', '--workspace'], cwd=ROOT, check=True)
        record['preflight'] = live.preflight(False, root=ROOT, binary=BINARY)
        book = live.ledger()
        for key in names.values():
            if key in book['runs']:
                raise SystemExit(f'the ledger already holds {key}; those tokens were '
                                 'spent. Run again under --attempt instead.')
        spent = sequence_charged(book)
        if spent >= SEQUENCE_STOP:
            raise SystemExit(f'stop: the lead sequence has charged {spent} of '
                             f'{SEQUENCE_CAP}, at or past the {SEQUENCE_STOP} stop')
        if live.cumulative(book) >= live.STOP_AT:
            raise SystemExit('stop: the MiniMax cap has reached its stop')
        root = private(live.HOME / 'pio-m4-live' / f'L1-{uuid.uuid4().hex[:8]}')
    rows = Rows(rehearse)
    service = Service(root, rehearse, scenario(args.mutant))
    desk = Desk(root / 'desk', rehearse)
    try:
        repo, base = fixture(root)
        truth = wc_l(repo)
        record['wc_l'] = truth
        record['owner_service_before'] = live.owner_service()
        record['sessions_before'] = live.session_listing(repo, rehearse)
        record['configured'] = live.configured_model(rehearse)
        service.start()
        owner = service.owner()

        grant_id = str(uuid.uuid4())
        rights = ['execution.submit', 'execution.steer', 'execution.read',
                  'core.events.read']
        if args.mutant == 'grant-may-answer':
            rights.append('execution.respond_action')
        if args.mutant == 'grant-no-steer':
            rights.remove('execution.steer')
        terms = dict(holder='lead', audience=PROVIDER, rights=rights,
                     resources=[dict(kind='execution.execution', id_prefix=f'{LEAD}.')],
                     delegation=dict(allowed=False, max_depth=0))
        issued = owner.call(command('core.grant.issue', dict(kind='core.grant', id=grant_id),
                                    terms, command_id=f'grant-{grant_id}'))
        assert 'result' in issued, issued
        record['grant'] = dict(id=grant_id, terms=terms)

        credential_file = root / 'lead.credential'
        credential_file.write_text(service.lead_credential + '\n')
        os.chmod(credential_file, 0o600)
        tool = mutated_tool(root) if args.mutant == 'reports-refused-as-started' else TOOL
        spec = dict(name='pio-lead', command=sys.executable,
                    args=[str(tool), '--credential-file', str(credential_file)],
                    env=[dict(name='PIO_LEAD_SOCKET', value=str(service.socket)),
                         dict(name='PIO_LEAD_GRANT', value=grant_id),
                         dict(name='PIO_LEAD_ID', value=LEAD),
                         dict(name='PIO_LEAD_WORKSPACE', value=str(repo)),
                         dict(name='PIO_LEAD_BASE', value=base),
                         dict(name='PIO_LEAD_LOG', value=str(root / 'lead-tool.jsonl'))])
        extensions = {CONTENT: dict(media_type='text/plain', text=BRIEF)}
        if args.mutant == 'lead-without-brief':
            extensions = {}
        if args.mutant != 'no-tool':
            extensions[LEAD_TOOL] = spec
        record['lead_submit'] = submit(owner, LEAD, BRIEF, repo, base,
                                       dict(initiator=dict(kind='execution.execution',
                                                           id=LEAD),
                                            depth=0, call_budget=BUDGET),
                                       extensions).get('result', {}).get('outcome')

        # Watch until the lead's turn is over: relay approvals, steer a child
        # while it is running, and make the third start while the lead is.
        lead_grant = service.lead(grant_id)
        steered, third = None, None
        deadline = time.monotonic() + EXECUTION_DEADLINE + 120
        while time.monotonic() < deadline:
            views = {i: view(owner, i) for i in (LEAD, *[f'{LEAD}.{c}' for c in CHILDREN])}
            stream = None
            for identity, current in views.items():
                if current and current.get('runtime') == 'requires_action':
                    stream = stream or events(owner)
                    desk.relay(service, identity, current, stream)
            first = views[f'{LEAD}.alpha']
            if steered is None and first and first['admission'] == 'admitted' \
                    and first['runtime'] != 'exited':
                steered = steer(lead_grant, f'{LEAD}.alpha')
            both = all(views[f'{LEAD}.{c}'] and views[f'{LEAD}.{c}']['admission'] == 'admitted'
                       for c in CHILDREN)
            lead_view = views[LEAD] or {}
            if third is None and both and lead_view.get('runtime') != 'exited':
                instance = ToolInstance(spec, root / 'runner-tool.jsonl')
                try:
                    third = dict(result=instance.tool('start_run', name='third',
                                                      brief='a third run the budget does not allow'),
                                 lead_runtime=view(owner, LEAD)['runtime'])
                finally:
                    instance.close()
            if lead_view.get('runtime') == 'exited' or lead_view.get('admission') == 'refused':
                break
            time.sleep(0.25)
        # The children, too, before anything is read from them.
        for c in CHILDREN:
            end = time.monotonic() + 120
            while time.monotonic() < end:
                current = view(owner, f'{LEAD}.{c}')
                if not current or current['runtime'] == 'exited' \
                        or current['admission'] == 'refused':
                    break
                time.sleep(0.25)

        exited_steer = steer(lead_grant, f'{LEAD}.alpha')
        answer = respond(lead_grant, f'{LEAD}.alpha', f'{LEAD}.alpha.action-1', 'allow',
                         (view(owner, f'{LEAD}.alpha') or {}).get('revision', 0))
        own = lead_grant.query('execution.inspect', {'execution': LEAD})
        # Attaching a tool is the owner's act: under the lead's grant it is
        # refused before anything else about the submit is looked at.
        attach = submit(lead_grant, f'{LEAD}.attached', 'give my child a tool', repo, base,
                        dict(initiator=dict(kind='execution.execution', id=LEAD), depth=1,
                             call_budget=0),
                        {CONTENT: dict(media_type='text/plain', text='give my child a tool'),
                         LEAD_TOOL: spec})
        lead_grant.close()
        # And a spec that carries a credential value is refused at admission,
        # because the spec is journaled. A credential-shaped dummy, never the
        # lead's own: a refused submit is journaled too.
        leaky = dict(spec, env=[*spec['env'], dict(name='PIO_LEAD_NOTE',
                                                   value='ccred1.lead.' + 'x' * 43)])
        brief_bytes = 'a spec with a credential value in it'
        credential_check = submit(owner, 'credential-check', brief_bytes, repo, base, None,
                                  {CONTENT: dict(media_type='text/plain', text=brief_bytes),
                                   LEAD_TOOL: leaky})
        # Read after every command the runner makes, so the stream holds them.
        views = {i: view(owner, i) or {} for i in
                 (LEAD, *[f'{LEAD}.{c}' for c in CHILDREN], f'{LEAD}.third')}
        stream = events(owner)
        tool_log = [json.loads(l) for l in (root / 'lead-tool.jsonl').read_text().splitlines()
                    if l.strip()] if (root / 'lead-tool.jsonl').exists() else []
        briefs = {LEAD: BRIEF}
        briefs.update({f"{LEAD}.{e['arguments'].get('name')}": e['arguments'].get('brief', '')
                       for e in tool_log if e.get('event') == 'tool_call'
                       and e.get('tool') == 'start_run'})
        host = service.host_events(views, briefs)
        said = {c: spoken(owner, f'{LEAD}.{c}') for c in CHILDREN}
        relay = spoken(owner, LEAD)
        owner.close()
        record.update(views=views, tool_log=tool_log, spoken=dict(said, lead=relay),
                      steer_running=steered, steer_exited=exited_steer, third=third,
                      desk=list(desk.seen.values()))

        # --- The lead's own run.
        lead_view = views[LEAD]
        rows.add('The lead was admitted', lead_view.get('admission'), 'admitted')
        rows.add("The lead's delivery was acknowledged", lead_view.get('delivery'),
                 'acknowledged')
        rows.add('The lead exited normally',
                 dict(runtime=lead_view.get('runtime'), exit=lead_view.get('exit')),
                 dict(runtime='exited', exit={'code': 0}))

        # --- The tool, the lead's session and nobody else's.
        sent = {run: [e.get('names') for e in host.get(run, [])
                      if e['kind'] == 'mcp_servers_sent']
                for run in (LEAD, *[f'{LEAD}.{c}' for c in CHILDREN])}
        rows.add('Only the lead got the tool', sent,
                 {LEAD: [['pio-lead']], **{f'{LEAD}.{c}': [[]] for c in CHILDREN}})
        # The tool's own witness, not the host's account of itself: each
        # launch writes `started`. One is the lead's session; none means the
        # lead never had it, and three means the children did too.
        rows.add('The tool was launched exactly once',
                 len([e for e in tool_log if e.get('event') == 'started']), 1)
        methods = [e['method'] for e in tool_log if e.get('event') == 'request']
        rows.add('The tool reached the lead', methods[:3],
                 ['initialize', 'notifications/initialized', 'tools/list'])
        calls = [e for e in tool_log if e.get('event') == 'tool_call']
        rows.add('The lead started its two runs through the tool',
                 sorted((e['arguments'].get('name'), e['result'].get('started'))
                        for e in calls if e['tool'] == 'start_run'),
                 sorted((c, True) for c in CHILDREN))
        rows.add('Every tool request carried the grant',
                 sorted({e.get('grant') for e in tool_log}), [grant_id])

        # --- The runs it started.
        children = {f'{LEAD}.{c}': views[f'{LEAD}.{c}'] for c in CHILDREN}
        rows.add('The submits landed', {i: v.get('admission') for i, v in children.items()},
                 {i: 'admitted' for i in children})
        rows.add('origin.initiator is bound', {i: v.get('origin') for i, v in children.items()},
                 {i: dict(initiator=dict(kind='execution.execution', id=LEAD), depth=1,
                          call_budget=0) for i in children})
        rows.add('The children ran',
                 {i: dict(delivery=v.get('delivery'), runtime=v.get('runtime'),
                          exit=v.get('exit')) for i, v in children.items()},
                 {i: dict(delivery='acknowledged', runtime='exited', exit={'code': 0})
                  for i in children})
        rows.add('A third start is refused by PIO',
                 third and dict(started=third['result'].get('started'),
                                code=(third['result'].get('refused') or {}).get('code'),
                                lead_running=third['lead_runtime'] != 'exited'),
                 dict(started=False, code='call_budget_spent', lead_running=True))
        led = sorted(e['subject']['id'] for e in stream
                     if e['type'] == 'execution.exit.observed'
                     and e['subject']['id'].startswith(f'{LEAD}.'))
        rows.add(f'{LEAD} has exactly two children that ran', led, sorted(children))

        # --- What the grant carries, and the one thing it does not.
        under = [e['payload'].get(UNDER_GRANT) for e in stream
                 if e['type'] == 'execution.steer.requested'
                 and e['subject']['id'] == f'{LEAD}.alpha']
        rows.add('A steer while the child runs', steered,
                 'not_supported, while the child had not exited',
                 holds=lambda s: bool(s) and s.get('request') == 'not_supported'
                 and s.get('runtime_at_steer') not in (None, 'exited'),
                 note='OpenCode has no steer: the host never sets a turn id')
        rows.add('A steer on an exited run', exited_steer,
                 'not_supported on any harness, once the turn is over',
                 holds=lambda s: s.get('request') == 'not_supported'
                 and s.get('runtime_at_steer') == 'exited')
        rows.add('Each steer names the grant that sent it',
                 [dict(grant=u and u.get('grant'), holder=u and u.get('holder'),
                       recorded_by=u and u.get('recorded_by')) for u in under],
                 [dict(grant=grant_id, holder='lead', recorded_by='pio')] * 2)
        data = answer.get('error', {}).get('data', {})
        rows.add('The lead may not answer an approval',
                 dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
                 dict(code='permission_denied', reason='right_missing'),
                 note='aimed at a child it may read, so only the missing right can refuse it')
        data = attach.get('error', {}).get('data', {})
        rows.add('A grant cannot attach the tool',
                 dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
                 dict(code='permission_denied', reason='owner_authority_required'))
        outcome = credential_check.get('result', {}).get('outcome', {})
        rows.add('A tool spec carrying a credential is refused',
                 dict(admission=outcome.get('admission'), reason=outcome.get('reason'),
                      named='lead_tool_carries_a_credential_value'
                      in str(outcome.get('alternative'))),
                 dict(admission='refused', reason='capability_unavailable', named=True))
        data = own.get('error', {}).get('data', {})
        rows.add('The lead cannot read its own run',
                 dict(code=data.get('code'), reason=(data.get('details') or {}).get('reason')),
                 dict(code='permission_denied', reason='out_of_scope'),
                 note=f'the grant covers {LEAD}. and not {LEAD}')

        # --- The results, against the runner's own count.
        # In a rehearsal the fake counted and the relay is scripted, so these
        # two show the runner compares — which is what kills `wrong-child`
        # and `wrong-relay` — and not that a model got anything right.
        compares = ('rehearsal: the fake counted, so this shows the runner compares'
                    if rehearse else '')
        rows.add('Each child reported the true count',
                 {CHILDREN[c]: first_number(said[c]) for c in CHILDREN}, truth,
                 note=compares)
        rows.add('The lead relayed the true counts', relayed(relay), truth, note=compares)

        # --- Approvals: the desk, never PIO by default, never "always".
        # Who decided is on the stream; the option actually sent is the host's
        # own record of what it applied, because the stream's decision for a
        # caller's answer carries only the decision.
        decisions = []
        for e in stream:
            if e['type'] != 'execution.action.answered':
                continue
            run_id, action_id = e['subject']['id'], e['payload'].get('action_id', '')
            seq = int(action_id.rsplit('-', 1)[-1]) if action_id[-1:].isdigit() else None
            applied = next((a for a in host.get(run_id, []) if a['kind'] == 'control_applied'
                            and a.get('action_seq') == seq), {})
            relayed_item = desk.seen.get(action_id, {})
            decisions.append(dict(
                run=run_id, action_id=action_id,
                desk=(relayed_item.get('answer') or {}).get('decision'),
                decided_by=e['payload'].get(DECISION, {}).get('decided_by'),
                applied=applied.get('applied'), option_kind=applied.get('option_kind'),
                always_option_taken=applied.get('always_option_taken')))
        lapsed = [run_id for run_id, records in host.items()
                  if any(r['kind'] == 'request_denied_by_default' for r in records)]
        single_use = {'allow': 'allow_once', 'deny': 'reject_once'}
        rows.add('Every approval was decided at the desk',
                 dict(decisions=decisions, lapsed=lapsed, relayed=len(desk.seen)),
                 'each relayed, decided by the caller, sent as the single-use kind, '
                 'never always, none lapsed',
                 holds=lambda o: not o['lapsed'] and o['relayed'] == len(o['decisions'])
                 and all(d['decided_by'] == 'caller' and d['applied'] is True
                         and d['option_kind'] == single_use.get(d['desk'])
                         and d['always_option_taken'] is False for d in o['decisions']),
                 note=f'{len(decisions)} approval(s) asked')
        rows.add('What real OpenCode does with a permission prompt',
                 [dict(run=d['run'], option_kind=d['option_kind']) for d in decisions],
                 'observed live', holds=lambda ds: True, live_only=True)

        # --- Usage, never zero for unknown.
        usage = {}
        for identity in (LEAD, *children):
            observations = (views[identity].get('usage') or {}).get('observations') or []
            usage[identity] = observations[0]['amount'] if observations else None
        rows.add('Every run reported its usage', usage, 'a positive amount per run',
                 holds=lambda u: all(isinstance(a, int) and a > 0 for a in u.values()))
        record['usage'] = usage

        record['owner_service_after'] = live.owner_service()
        rows.add("The owner's OpenCode service was untouched",
                 record['owner_service_after'] == record['owner_service_before'], True)
        record['sessions_after'] = live.session_listing(repo, rehearse)
        rows.add('PIO deleted no session',
                 live.deleted_nothing(record['sessions_before'], record['sessions_after']),
                 True, live_only=True)
    finally:
        service.release(remove=rehearse)

    record['rows'] = rows.rows
    record['failed'] = rows.failed()
    if not rehearse:
        record['charge'] = charge(names, usage, started_at)
    return record


def sequence_charged(book):
    return sum(e.get('charged') or 0 for e in book['runs'].values()
               if e.get('sequence') == SEQUENCE)


def charge(names, usage, at):
    """One ledger line per run, the sequence total, and whether a stop fired.

    Charged, never observed: a run that reported nothing is charged the
    sequence's allowance and its usage is recorded as unknown, never zero.
    """
    book = live.ledger()
    lines = {}
    for identity, amount in usage.items():
        entry = dict(sequence=SEQUENCE, model=MODEL, at=at)
        if isinstance(amount, int) and amount > 0:
            entry.update(observed_total_tokens=amount, charged=amount, charge_basis='observed')
        else:
            entry.update(observed_total_tokens=None, usage='unknown',
                         charged=live.CANCEL_ALLOWANCE, charge_basis='allowance',
                         why=live.CHARGE_BASIS)
        book['runs'][names[identity]] = entry
        lines[names[identity]] = entry
    live.ledger_path().write_text(json.dumps(book, indent=2) + '\n')
    total = sequence_charged(book)
    return dict(lines=lines, sequence_charged=total, sequence_cap=SEQUENCE_CAP,
                sequence_stop=SEQUENCE_STOP, stop_reached=total >= SEQUENCE_STOP,
                minimax_charged=live.cumulative(book), minimax_cap=live.CAP,
                measured_against='charged', limit_is_next_turn_only=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/lead-run')
    parser.add_argument('--rehearse', action='store_true',
                        help='the labeled fake, no tokens')
    parser.add_argument('--desk', help="the owner's words confirming they are at the "
                                       'desk; required for a live run, quoted in the receipt')
    parser.add_argument('--attempt', help='a new name for a live run the ledger already holds')
    parser.add_argument('--mutant', choices=sorted(MUTANTS))
    args = parser.parse_args()
    if not args.rehearse and not args.desk:
        raise SystemExit('a live run needs --desk: the owner confirms they are at the '
                         'desk, and their words go in the receipt')
    if args.mutant and not args.rehearse:
        raise SystemExit('a mutant is a rehearsal; it never spends a token')
    args.out.mkdir(parents=True, exist_ok=True)
    record = run(args)
    name = ('rehearsal' if args.rehearse else (args.attempt or LEAD)) + \
        (f'-mutant-{args.mutant}' if args.mutant else '')
    path = args.out / f'{name}.json'
    path.write_text(json.dumps(redact(record), indent=2, sort_keys=True) + '\n')
    for row in record['rows']:
        mark = {True: 'ok  ', False: 'FAIL', None: 'n/a '}[row['holds']]
        print(f"{mark} {row['row']}: {json.dumps(row['observed'])[:140]}")
    unproven = [r['row'] for r in record['rows'] if r['holds'] is None]
    print(f"\n{len(record['rows'])} rows; not provable here: {unproven}")
    if args.mutant:
        wanted = MUTANTS[args.mutant]
        assert wanted in record['failed'], (
            f'mutant {args.mutant} did not fail its row {wanted!r}; failed: {record["failed"]}')
        print(f'mutant {args.mutant}: dies on {wanted!r}')
        raise SystemExit(1)
    if record['failed']:
        raise SystemExit(f"FAILED ROWS: {record['failed']}")
    print('lead run: every row holds')


if __name__ == '__main__':
    main()
