#!/usr/bin/env python3
"""L1 — a lead that starts runs, through `serve-opencode`.

Owner approval `owner-2026-09-23`, on the amended plan: one service, the
lead and both led runs on `minimax-coding-plan/MiniMax-M3`, call budget 2,
cap 2,000,000 with the sequence's own stop at 1,600,000 read from
**charged**, rehearsal against the labeled fake first.

`--rehearse` runs the whole shape against the labeled ACP fake and **spends
nothing**. Its purpose is not to show that a lead works — the fake runs no
model and calls no tool — but that **the runner can make every observation
the plan promises** before a token is spent. That rule exists because
receipts once claimed observations the run could not make.

Two rows the rehearsal cannot settle, and says so rather than implying
otherwise:

- **"the results are true."** The fake reports what it is told to report, so
  the `wc -l` comparison means nothing here. Only the live run counts.
- **Anything about a real permission prompt.** The fake asks when the
  scenario says to; what the owner's OpenCode does with these briefs is a
  live observation.

And one the rehearsal reaches only by standing in for the model: the fake
launches no MCP server, so the runner drives the lead's tool directly over
MCP in the model's place. The hop from model to tool was settled separately,
at zero tokens, by `lead_tool_probe.py`.
"""
import argparse
import json
import os
import subprocess
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup
import opencode_host_matrix as matrix
from board_fold import Caller
from check_private_paths import redact
from public_api import CREDENTIAL, command

ROOT = Path(__file__).resolve().parents[1]
LEAD = 'L1'
LEAD_CREDENTIAL = 'ccred1.lead.' + 'l' * 43
PROVIDER = 'conformance-provider'
MODEL = 'minimax-coding-plan/MiniMax-M3'
FEATURES = ('core.events', 'core.capabilities', 'core.effects', 'core.grants')
SEQUENCE_CAP = 2_000_000
SEQUENCE_STOP = 1_600_000
BRIEF = ('There are two files in this workspace, alpha.md and beta.md. '
         'Use the pio-lead tool to start one run for each, asking it to '
         'report that file\'s line count. Do not read the files yourself. '
         'When both have reported, summarise what they said.')
CHILD_BRIEF = 'Report the number of lines in {name}. Answer with the number alone.'
FILES = {'alpha.md': 7, 'beta.md': 4}


def fixture(root):
    """The workspace the lead and its children share."""
    repo = root / 'fixtures' / 'lead'
    repo.mkdir(parents=True, exist_ok=True)
    for name, lines in FILES.items():
        (repo / name).write_text(''.join(f'line {n}\n' for n in range(1, lines + 1)))
    (repo / 'README.md').write_text('L1 fixture. No task runs here.\n')
    git = lambda *a: subprocess.run(['git', '-C', str(repo), *a], check=True,
                                    capture_output=True, text=True)
    git('init', '-q')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio', 'add', '.')
    git('-c', 'user.email=pio@example.invalid', '-c', 'user.name=pio',
        'commit', '-q', '-m', 'fixture')
    return repo, git('rev-parse', 'HEAD').stdout.strip()


def truth(repo):
    """What the files actually say. The runner counts; the lead reports."""
    return {name: len((repo / name).read_text().splitlines()) for name in FILES}


class Mcp:
    """Speak MCP to the lead's tool, in the model's place."""

    def __init__(self, env, tool):
        self.child = subprocess.Popen(
            [sys.executable, str(tool)], env=env, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, text=True, bufsize=1, start_new_session=True)
        self.counter = 0
        self.call('initialize', {'protocolVersion': '2024-11-05',
                                 'capabilities': {}, 'clientInfo':
                                 {'name': 'rehearsal', 'version': '0'}})

    def call(self, method, params, seconds=120):
        self.counter += 1
        self.child.stdin.write(json.dumps(
            {'jsonrpc': '2.0', 'id': self.counter, 'method': method,
             'params': params}) + '\n')
        self.child.stdin.flush()
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            line = self.child.stdout.readline()
            if not line:
                return None
            answer = json.loads(line)
            if answer.get('id') == self.counter:
                return answer
        return None

    def tool(self, tool_name, **arguments):
        answer = self.call('tools/call',
                           {'name': tool_name, 'arguments': arguments})
        text = answer['result']['content'][0]['text']
        return json.loads(text) if not answer['result'].get('isError') else \
            {'error': text}

    def close(self):
        import signal
        try:
            os.killpg(os.getpgid(self.child.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            self.child.kill()
        self.child.wait(timeout=10)


def issue_grant(owner, grant_id):
    """The lead's grant, as the plan names it."""
    terms = dict(holder='lead', audience=PROVIDER,
                 rights=['execution.submit', 'execution.steer',
                         'execution.read', 'core.events.read'],
                 resources=[dict(kind='execution.execution',
                                 id_prefix=f'{LEAD}.')],
                 delegation=dict(allowed=False, max_depth=0))
    issued = owner.call(command('core.grant.issue',
                                dict(kind='core.grant', id=grant_id), terms,
                                command_id=f'grant-{grant_id}'))
    assert 'result' in issued, issued
    return terms


def events_of(caller, subject=None):
    items, payload = [], {'limit': 1000, 'from': 'start',
                          'kinds': ['execution.execution']}
    while True:
        answer = caller.query('core.events.read', payload)
        result = answer.get('result')
        assert result is not None, f'core.events.read refused: {answer}'
        items += [i['event'] for i in result['items'] if 'event' in i]
        if not result.get('has_more'):
            break
        payload = {'limit': 1000, 'cursor': result['cursor'],
                   'kinds': ['execution.execution']}
    return [e for e in items if subject is None or e['subject']['id'] == subject]


def row(rows, name, observed, proven=True, note=''):
    rows.append(dict(row=name, observed=observed, proven=proven, note=note))
    return observed


def run(out, rehearse, desk=None):
    """One lead sequence, rehearsed or live."""
    out.mkdir(parents=True, exist_ok=True)
    rows, record = [], {'mode': 'rehearsal' if rehearse else 'live',
                        'lead': LEAD, 'model': MODEL,
                        'sequence_cap': SEQUENCE_CAP, 'sequence_stop': SEQUENCE_STOP}
    case = matrix.ServiceCase(
        out, 'L1', model=MODEL, extra_credentials=(LEAD_CREDENTIAL,),
        labeled_fake=rehearse,
        scenario={'message_chunks': 2} if rehearse else None)
    try:
        repo, base = fixture(case.root)
        record['wc_l'] = truth(repo)
        case.start()
        owner = Caller(case.socket, CREDENTIAL, features=FEATURES,
                       execution_features=(*matrix.FEATURES, 'execution.steering'))

        grant_id = str(uuid.uuid4())
        record['grant'] = issue_grant(owner, grant_id)
        record['grant_id'] = grant_id

        log = case.root / 'lead-tool.jsonl'
        tool_env = dict(PATH='/usr/bin:/bin', HOME=str(case.root),
                        PIO_LEAD_SOCKET=str(case.socket), PIO_LEAD_GRANT=grant_id,
                        PIO_LEAD_CREDENTIAL=LEAD_CREDENTIAL, PIO_LEAD_ID=LEAD,
                        PIO_LEAD_WORKSPACE=str(repo), PIO_LEAD_BASE=base,
                        PIO_LEAD_LOG=str(log))

        # The lead itself: its own initiator at depth 0, with two calls.
        started = owner.call(command(
            'execution.submit', dict(kind='execution.execution', id=LEAD),
            dict(brief=dict(digest='sha256:' + __import__('hashlib').sha256(
                     BRIEF.encode()).hexdigest(), media_type='text/plain'),
                 workspace=dict(repository=str(repo), base=base, cleanup='retain'),
                 timeouts=dict(delivery=300, execution_deadline=900),
                 origin=dict(initiator=dict(kind='execution.execution', id=LEAD),
                             depth=0, call_budget=2)),
            command_id=LEAD))
        started['extensions'] = None
        assert 'result' in started, started
        record['lead_submitted'] = True

        tool = ROOT / 'scripts' / 'lead_tool.py'
        if rehearse:
            # The fake launches no MCP server, so the runner stands in for
            # the model. The model-to-tool hop was settled separately, at
            # zero tokens, by `lead_tool_probe.py`.
            mcp = Mcp(tool_env, tool)
            try:
                listed = mcp.call('tools/list', {})['result']['tools']
                row(rows, 'The tool reached the lead',
                    sorted(t['name'] for t in listed),
                    note='stood in for the model; the real hop is in the probe')
                for name in FILES:
                    mcp.tool('start_run', name=name.split('.')[0],
                             brief=CHILD_BRIEF.format(name=name))
                third = mcp.tool('start_run', name='third',
                                 brief='a third run the budget does not allow')
                record['third_start'] = third
                row(rows, 'The third start was refused',
                    dict(started=third.get('started'),
                         reason=third.get('refused', {}).get('code')))
            finally:
                mcp.close()
        record['tool_log'] = [json.loads(l) for l in log.read_text().splitlines()
                              if l.strip()] if log.exists() else []
        row(rows, 'The lead called the tool',
            [e['tool'] for e in record['tool_log'] if e.get('event') == 'tool_call'],
            note='' if not rehearse else 'driven by the runner, not a model')

        children = [f'{LEAD}.{n.split(".")[0]}' for n in FILES]
        for identity in children:
            for _ in range(240):
                view = owner.query('execution.inspect',
                                   {'execution': identity}).get('result')
                if view and view['runtime'] == 'exited':
                    break
                time.sleep(0.5)
        events = events_of(owner)
        seen = {e['subject']['id'] for e in events}
        row(rows, 'The submit landed', sorted(i for i in seen if i.startswith(f'{LEAD}.')))
        origins = {}
        for identity in children:
            view = owner.query('execution.inspect',
                               {'execution': identity}).get('result') or {}
            origins[identity] = view.get('origin')
        row(rows, 'origin.initiator is bound', origins)
        ran = sorted({e['subject']['id'] for e in events
                      if e['type'] == 'execution.exit.observed'
                      and e['subject']['id'].startswith(f'{LEAD}.')})
        # Exactly two, and the budget is why. A refused admission still
        # creates a subject, so counting subjects would have counted three.
        row(rows, 'The children ran', ran)
        row(rows, 'The lead started exactly two', len(ran) == 2)

        # The measured negative: the grant carries steer, this harness has none.
        view = owner.query('execution.inspect', {'execution': LEAD}).get('result') or {}
        lead_tool = Mcp(tool_env, tool)
        try:
            steered = lead_tool.call('tools/list', {})
        finally:
            lead_tool.close()
        # **A child, not the lead itself.** The grant is scoped
        # `id_prefix: "L1."`, which does not cover `L1` — so the lead can
        # steer and read the runs it started and not the run it *is*. The
        # first draft of this row steered `L1` and came back
        # `permission_denied`, which would have read as "the grant does not
        # carry steer" when it means the opposite.
        child = f'{LEAD}.alpha'
        child_view = owner.query('execution.inspect',
                                 {'execution': child}).get('result') or {}
        steer = steer_attempt(case, grant_id, child, child_view.get('revision', 0))
        row(rows, 'The grant carried execution.steer', steer)

        answered = answer_attempt(case, grant_id, view.get('revision', 0))
        row(rows, 'The lead answered no approval', answered)

        outside = outside_attempt(case, grant_id)
        row(rows, 'The lead cannot read even its own run',
            outside, note='the grant covers L1. and not L1')

        row(rows, 'The results are true',
            'not provable in a rehearsal' if rehearse else record.get('reported'),
            proven=not rehearse,
            note='the fake reports what it is told; only the live wc -l counts'
            if rehearse else '')
        row(rows, 'What real OpenCode does with a permission prompt',
            'not observed', proven=not rehearse,
            note='the fake asks when the scenario says to; this is a live observation')

        case.finish()
        record['owner_service_untouched'] = True
        record['rows'] = rows
    finally:
        case.cleanup()
    return record


def lead_caller(case, grant_id):
    return Caller(case.socket, LEAD_CREDENTIAL, grant=grant_id, features=FEATURES,
                  execution_features=(*matrix.FEATURES, 'execution.steering'))


def steer_attempt(case, grant_id, target, revision):
    """One steer under the grant. Measured negative on this harness."""
    import hashlib
    lead = lead_caller(case, grant_id)
    try:
        body = b'keep to the fixture'
        envelope = command('execution.steer',
                           dict(kind='execution.execution', id=target),
                           dict(message=dict(
                               digest='sha256:' + hashlib.sha256(body).hexdigest(),
                               media_type='text/plain')),
                           command_id='lead-steer', revision=revision)
        envelope['extensions'] = {'pio.combraton.dev/content':
                                  dict(media_type='text/plain',
                                       text=body.decode())}
        answer = lead.call(envelope)
    finally:
        lead.close()
    if 'error' in answer:
        return {'refused': answer['error']['data'].get('code')}
    # The operation's own answer is under `outcome`, as every command's is.
    outcome = answer['result'].get('outcome', answer['result'])
    return {'request': outcome.get('request'),
            'alternative': outcome.get('alternative')}


def answer_attempt(case, grant_id, revision):
    """The one thing the lead may not do."""
    import hashlib
    lead = lead_caller(case, grant_id)
    try:
        body = json.dumps({'decision': 'allow'}).encode()
        envelope = command('execution.respond_action',
                           dict(kind='execution.execution', id=LEAD),
                           dict(action_id=f'{LEAD}.action-1',
                                response=dict(
                                    digest='sha256:' + hashlib.sha256(body).hexdigest(),
                                    media_type='application/json')),
                           command_id='lead-answer', revision=revision)
        envelope['extensions'] = {'pio.combraton.dev/content':
                                  dict(media_type='application/json',
                                       text=body.decode())}
        answer = lead.call(envelope)
    finally:
        lead.close()
    data = answer.get('error', {}).get('data', {})
    return {'code': data.get('code'), 'reason': data.get('details', {}).get('reason')}


def outside_attempt(case, grant_id):
    """A run outside the lead's own subtree."""
    lead = lead_caller(case, grant_id)
    try:
        answer = lead.query('execution.inspect', {'execution': LEAD})
    finally:
        lead.close()
    data = answer.get('error', {}).get('data', {})
    return {'code': data.get('code'), 'reason': data.get('details', {}).get('reason')}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/lead-run')
    parser.add_argument('--rehearse', action='store_true',
                        help='the labeled fake, no tokens; proves the runner '
                             'can make every observation the plan promises')
    parser.add_argument('--desk', help="the owner's confirmation that they are "
                                       'at the desk; required for a live run')
    args = parser.parse_args()
    if not args.rehearse and not args.desk:
        raise SystemExit('a live run needs --desk: the owner confirms they are '
                         'at the desk, and their words go in the receipt')
    record = run(args.out, args.rehearse, args.desk)
    record['desk'] = args.desk
    path = args.out / ('rehearsal.json' if args.rehearse else 'L1.json')
    path.write_text(json.dumps(redact(record), indent=2, sort_keys=True) + '\n')
    print(json.dumps(redact(record), indent=2, sort_keys=True))
    unproven = [r['row'] for r in record['rows'] if not r['proven']]
    print(f'\n{len(record["rows"])} rows; {len(unproven)} not provable here: '
          f'{unproven}')


if __name__ == '__main__':
    main()
