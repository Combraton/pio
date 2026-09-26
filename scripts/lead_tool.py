#!/usr/bin/env python3
"""The lead's tool: the public Protocol API, behind a grant, over MCP.

This is what a lead session is given and the only thing it is given. It
speaks MCP on stdio — which is how both OpenCode and Codex were measured to
launch a registered server — and every call it makes goes out on the public
Unix socket with the lead's grant presented on the envelope.

It is deliberately small. There is no privileged path in it, no second
credential, and nothing it can do that the grant does not already allow: a
non-authority principal presenting no grant is refused `grant_required`, and
the grant carries `execution.submit`, `execution.steer`, `execution.read` and
`core.events.read` and **not** `execution.respond_action`. The credential is
readable from the lead's own shell in principle; the grant terms, not
secrecy, are the boundary.

Every request is written to `PIO_LEAD_LOG` with the grant id on it, so what
the lead did is readable afterwards — which is the record the Protocol has
no field for (Combraton/protocol#17).

**The credential is read from a file named on the command line**, never
passed as a value. The spec that launches this tool is journaled with the
lead's run, so a value there would sit in the journal; admission refuses one.

**`read_run` waits for the run, here, not in the model.** It returns when the
run has exited or after `READ_WAIT` seconds, whichever is first. A read that
returned at once made every poll a model step, and a lead told to read "again
until exited" had nothing but its deadline to stop it (review 44). What it
returns is bounded too: the run's words are cut to their last `TEXT_LIMIT`
characters, so no call can add more than that to the lead's context.

**A stopped lead's calls are held.** Past `PIO_LEAD_CALL_CEILING` calls, or
once the runner has created the file `PIO_LEAD_STOP`, a call is held for
`HOLD` seconds and then refused. OpenCode does not end a turn on
`session/cancel` (M3b, R4); the host kills it ten seconds later. A call held
past the kill is a model step the lead cannot take in between. The stop file
is checked again when a result is ready, so a call that was running when the
lead was stopped (a `read_run` waits up to 55 s) hands back nothing either.

**Only the plan's children.** With `PIO_LEAD_CHILDREN` set, `start_run`
refuses any other name before anything reaches the service, so every run the
lead can start is one the runner knows by name.

**On Codex, no result that would let the lead step past its share.** Codex
reports a step once its tool has finished (M2 R5, R6), so when a result is
ready the lead's last report covers every step but the one that made this
call; handing the result back starts one more. With `PIO_LEAD_METER` set,
the tool waits for the runner's meter to have read the stream after the call
arrived, and withholds the result if the lead's reported total is past the
meter's `hold_above` (its ceiling less one step): the step that made the
call and the one the result would start could take it past its share. The
runner then stops the lead (review of L3, A7).
"""
import base64
import hashlib
import json
import os
import socket
import sys
import time
import uuid

SOCKET = os.environ.get('PIO_LEAD_SOCKET', '')
GRANT = os.environ.get('PIO_LEAD_GRANT', '')
LEAD = os.environ.get('PIO_LEAD_ID', 'L1')
WORKSPACE = os.environ.get('PIO_LEAD_WORKSPACE', '')
BASE = os.environ.get('PIO_LEAD_BASE', '')
LOG = os.environ.get('PIO_LEAD_LOG', '')
CONTENT = 'pio.combraton.dev/content'
# Under the MCP TypeScript SDK's default request timeout (60 seconds), and far
# under what OpenCode 2.0.11 allows: its shipped code turns an ACP-supplied
# server into a local server with no timeout, and `callTool` falls back to
# 43,200,000 ms (12 hours). That is static evidence from the shipped code
# (review 45), not a measurement; the live run's tool log is the check. A
# longer wait is fewer model steps: at 20 seconds, a child held at the desk
# cost the lead a step every 20 seconds, and the lead's meter could stop it
# before the owner had answered (review 45).
READ_WAIT = 55
TEXT_LIMIT = 2000
CALL_CEILING = int(os.environ.get('PIO_LEAD_CALL_CEILING') or 0)
STOP = os.environ.get('PIO_LEAD_STOP', '')
HOLD = 30
CALLS = []
# The runner's meter for this lead (Codex only): its reported total, and how
# far past it no result is handed back.
METER = os.environ.get('PIO_LEAD_METER', '')
# A report reaches the runner's meter within this long of Codex sending it;
# the meter must have begun a pass this long after the call arrived.
FRESH = 1.5
METER_WAIT = 30
# The host's own deadline for each run it starts, in seconds: 900, unless the
# runner shortens it for a rehearsal's mutant (review of L3, round 3,
# SPEND-6).
DEADLINE = int(os.environ.get('PIO_LEAD_DEADLINE') or 900)
# The plan's children, by name (`PIO_LEAD_CHILDREN`, comma-separated). A
# start under any other name is refused here, before it reaches the service:
# a child the runner does not know by name is a run nothing meters, stops or
# charges by name (review of L3, round 3, SPEND-1). Unset, any name is
# started, as the M4b proofs start theirs.
CHILDREN = [c for c in os.environ.get('PIO_LEAD_CHILDREN', '').split(',') if c]


def credential():
    """The lead's Protocol credential, from `--credential-file PATH`."""
    args = sys.argv[1:]
    if '--credential-file' not in args:
        return ''
    with open(args[args.index('--credential-file') + 1]) as handle:
        return handle.read().strip()


CREDENTIAL = credential()


def note(entry):
    if not LOG:
        return
    with open(LOG, 'a') as handle:
        handle.write(json.dumps(dict(entry, grant=GRANT)) + '\n')


def digest(data):
    return 'sha256:' + hashlib.sha256(data).hexdigest()


class Api:
    """One connection to the public socket, authenticated as the lead."""

    def __init__(self):
        self.stream = socket.socket(socket.AF_UNIX)
        self.stream.settimeout(30)
        self.stream.connect(SOCKET)
        self.file = self.stream.makefile('rb')
        self.call('core.authenticate', dict(credential=CREDENTIAL), grant=False)
        self.call('core.negotiate', dict(
            profiles=[dict(name='core', majors=[1], required=True,
                           required_features=['core.events', 'core.capabilities',
                                              'core.effects', 'core.grants'],
                           optional_features=[]),
                      dict(name='execution', majors=[1], required=True,
                           required_features=['execution.controller', 'execution.output',
                                              'execution.discovery', 'execution.workspaces',
                                              'execution.usage', 'execution.actions',
                                              'execution.steering'],
                           optional_features=[])],
            caller=dict(name='pio-lead-tool', version='1'),
            receive_limits=dict(max_frame_bytes=1048576)), grant=False)

    def send(self, envelope):
        frame = dict(jsonrpc='2.0', id=str(uuid.uuid4()),
                     method=envelope['operation'], params=envelope)
        self.stream.sendall(json.dumps(frame).encode() + b'\n')
        while True:
            line = self.file.readline()
            if not line:
                raise EOFError('the service closed the socket')
            answer = json.loads(line)
            if answer.get('id') == frame['id']:
                return answer

    def call(self, operation, payload, grant=True, command=None, content=None):
        envelope = dict(operation=operation, message_id=str(uuid.uuid4()),
                        payload=payload)
        if command is not None:
            subject = dict(kind='execution.execution', id=command['subject'])
            envelope = dict(operation=operation, subject=subject,
                            preconditions=[dict(subject=subject,
                                                revision=command.get('revision', 0))],
                            requires=[], payload=payload, extensions={},
                            message_id=str(uuid.uuid4()),
                            command_id=command['id'], dedupe_generation=1,
                            authority_epoch=0)
            envelope['command_digest'] = 'sha256:' + hashlib.sha256(json.dumps(
                {k: envelope[k] for k in ('operation', 'subject', 'preconditions',
                                          'requires', 'payload', 'extensions')},
                sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        if content is not None:
            envelope['extensions'] = {CONTENT: content}
        if grant and GRANT:
            envelope['grant'] = GRANT
        return self.send(envelope)


def start_run(name, brief):
    """Start one run under this lead, and report how it went."""
    identity = f'{LEAD}.{name}'
    if CHILDREN and name not in CHILDREN:
        return dict(run=identity, started=False,
                    refused=dict(code='not_a_planned_child', children=CHILDREN,
                                 message=f'this lead may start only {CHILDREN}'))
    body = brief.encode()
    api = Api()
    answer = api.call(
        'execution.submit',
        dict(brief=dict(digest=digest(body), media_type='text/plain'),
             workspace=dict(repository=WORKSPACE, base=BASE, cleanup='retain'),
             timeouts=dict(delivery=300, execution_deadline=DEADLINE),
             # Bound to the grant: the initiator must be this lead, and the
             # depth exactly one below it. PIO refuses anything else.
             origin=dict(initiator=dict(kind='execution.execution', id=LEAD),
                         depth=1, call_budget=0)),
        command=dict(subject=identity, id=identity),
        content=dict(media_type='text/plain', text=brief))
    if 'error' in answer:
        return dict(run=identity, started=False, refused=answer['error']['data'])
    outcome = answer['result']['outcome']
    # A refused admission is not a start. Reporting one as started is how a
    # lead would come to believe it had a run it does not have.
    if outcome.get('admission') == 'refused':
        return dict(run=identity, started=False,
                    refused=dict(code=outcome.get('reason'),
                                 alternative=outcome.get('alternative')))
    return dict(run=identity, started=True, admission=outcome.get('admission'))


def messages(raw):
    """Each message a run said, in order, from its spooled records, and
    which of them were completed.

    OpenCode spools one record per `session/update`; its words are the
    `agent_message_chunk`s, one message. Codex spools its app-server
    notifications, and a turn can say more than one thing: on this model a
    run says what it is about to do before its command, then answers (M2 R5
    and R6: `agentMessage`, `commandExecution`, ..., `agentMessage`). So each
    `agentMessage` item is its own message: the text of the completed item
    where there is one, else its `item/agentMessage/delta`s (never both,
    which would say it twice). Handing the lead raw JSON would spend its
    tokens on framing, and it is the same decoding a screen does.
    """
    order, deltas, finished, chunks = [], {}, {}, []
    for line in raw.splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if not isinstance(record, dict):
            continue
        update = record.get('update') or {}
        if update.get('sessionUpdate') == 'agent_message_chunk':
            chunks.append(update.get('content', {}).get('text', ''))
        params = record.get('params') or {}
        item = params.get('item') or {}
        if record.get('method') == 'item/agentMessage/delta':
            key = params.get('itemId') or ''
            order += [] if key in deltas or key in finished else [key]
            deltas.setdefault(key, []).append(params.get('delta', ''))
        elif record.get('method') == 'item/completed' and item.get('type') == 'agentMessage':
            key = item.get('id') or ''
            order += [] if key in deltas or key in finished else [key]
            finished[key] = item.get('text', '')
    said = [dict(text=finished[k], completed=True) if k in finished
            else dict(text=''.join(deltas[k]), completed=False) for k in order]
    if chunks:
        said.insert(0, dict(text=''.join(chunks), completed=True))
    return said


def message_text(raw):
    """Everything a run said, one message to a line, so an answer split
    across two messages still reads as two lines (review of L3, F5)."""
    return '\n'.join(m['text'] for m in messages(raw))


def final_answer(raw):
    """What a run answered. On Codex, the last agentMessage it completed
    after its last completed command: a run cut off before it answered has
    only its preamble, which quotes the command, numbers and all, and that
    is no answer (review of L3, round 2, V-7). None when there is none. On
    OpenCode, whose updates carry no items, the words it said."""
    chunks, answer, ran = [], None, False
    for line in raw.splitlines():
        try:
            record = json.loads(line)
        except ValueError:
            continue
        if not isinstance(record, dict):
            continue
        update = record.get('update') or {}
        if update.get('sessionUpdate') == 'agent_message_chunk':
            chunks.append(update.get('content', {}).get('text', ''))
        item = (record.get('params') or {}).get('item') or {}
        if record.get('method') != 'item/completed':
            continue
        if item.get('type') == 'commandExecution':
            ran, answer = True, None
        elif item.get('type') == 'agentMessage' and ran:
            answer = item.get('text', '')
    if chunks:
        return ''.join(chunks)
    return answer


def read_run(name):
    """What a run this lead started has produced, once it has exited or
    `READ_WAIT` seconds have passed."""
    identity = f'{LEAD}.{name}'
    api = Api()
    until = time.monotonic() + READ_WAIT
    while True:
        view = api.call('execution.inspect', dict(execution=identity))
        if 'error' in view:
            return dict(run=identity, refused=view['error']['data'])
        view = view['result']
        if view['runtime'] == 'exited' or view.get('admission') == 'refused' \
                or time.monotonic() >= until:
            break
        time.sleep(0.5)
    out = api.call('execution.output.read',
                   dict(execution=identity, offset=0, max_bytes=65536))
    raw = ''
    if 'result' in out:
        raw = base64.b64decode(out['result']['data_base64']).decode(
            'utf-8', 'replace')
    return dict(run=identity, runtime=view['runtime'], exit=view.get('exit'),
                text=message_text(raw)[-TEXT_LIMIT:])


TOOLS = {
    'start_run': (start_run, 'Start one run under this lead.',
                  {'type': 'object',
                   'properties': {'name': {'type': 'string'},
                                  'brief': {'type': 'string'}},
                   'required': ['name', 'brief']}),
    'read_run': (read_run, 'Read a run this lead started. Waits up to '
                           f'{READ_WAIT} seconds for it to exit.',
                 {'type': 'object',
                  'properties': {'name': {'type': 'string'}},
                  'required': ['name']}),
}


def withheld(arrived):
    """Why a response that is ready must not be handed back, if it must not,
    and the meter reading it was judged on (None without a meter)."""
    why = ['the runner stopped this lead'] if STOP and os.path.exists(STOP) else []
    if not METER:
        return why, None
    until = time.monotonic() + METER_WAIT
    seen = None
    while seen is None:
        try:
            with open(METER) as handle:
                seen = json.load(handle)
            if seen.get('pass_started', 0) < arrived + FRESH:
                seen = None
        except (OSError, ValueError):
            seen = None
        if seen is None and time.monotonic() >= until:
            return why + [f"the runner's meter has not read the lead's usage in the "
                          f'{METER_WAIT} s since this call arrived'], None
        if seen is None:
            time.sleep(0.1)
    if seen['total'] > seen['hold_above']:
        why.append(f"the lead has reported {seen['total']} tokens, past "
                   f"{seen['hold_above']}: this result would start a step past its share")
    return why, seen


def handle(message):
    method = message.get('method')
    if method == 'initialize':
        return {'protocolVersion': '2024-11-05',
                'capabilities': {'tools': {}},
                'serverInfo': {'name': 'pio-lead', 'version': '1'}}
    if method == 'tools/list':
        return {'tools': [{'name': name, 'description': about,
                           'inputSchema': schema}
                          for name, (_, about, schema) in TOOLS.items()]}
    if method == 'tools/call':
        arrived = time.time()
        name = message['params']['name']
        arguments = message['params'].get('arguments') or {}
        CALLS.append(name)
        why = ([f'call {len(CALLS)}, past the ceiling of {CALL_CEILING}']
               if CALL_CEILING and len(CALLS) > CALL_CEILING else []) + \
            (['the runner stopped this lead'] if STOP and os.path.exists(STOP) else [])
        if why:
            note({'event': 'held', 'tool': name, 'call': len(CALLS), 'why': why,
                  'seconds': HOLD})
            time.sleep(HOLD)
            return {'isError': True,
                    'content': [{'type': 'text', 'text': 'stopped: ' + '; '.join(why)}]}
        began = time.monotonic()
        if name not in TOOLS:
            note({'event': 'tool_failed', 'tool': name, 'call': len(CALLS),
                  'error': 'no such tool'})
            response = {'isError': True,
                        'content': [{'type': 'text', 'text': f'no tool {name}'}]}
        else:
            try:
                result = TOOLS[name][0](**arguments)
            except Exception as error:  # reported, never swallowed
                note({'event': 'tool_failed', 'tool': name, 'call': len(CALLS),
                      'error': repr(error)})
                response = {'isError': True,
                            'content': [{'type': 'text', 'text': repr(error)}]}
            else:
                # How long the call took, so a read that waited can be shown
                # to have waited, from the tool's own log (L1b).
                note({'event': 'tool_call', 'tool': name, 'call': len(CALLS),
                      'arguments': arguments, 'result': result,
                      'seconds': round(time.monotonic() - began, 1)})
                response = {'content': [{'type': 'text', 'text': json.dumps(result)}]}
        # **Every** response passes the gate: a result, an error or an
        # unknown tool starts the lead's next step alike (review of L3, round
        # 2, SB-1). What the gate saw is logged for each, so a row can show it
        # ran on every one.
        held, seen = withheld(arrived)
        note({'event': 'gate', 'tool': name, 'call': len(CALLS), 'held': bool(held),
              'total': (seen or {}).get('total'), 'hold_above': (seen or {}).get('hold_above')})
        if held:
            # Done, and not handed back: the lead takes no step on it.
            note({'event': 'held', 'tool': name, 'call': len(CALLS), 'why': held,
                  'seconds': HOLD, 'result_withheld': True})
            time.sleep(HOLD)
            return {'isError': True,
                    'content': [{'type': 'text', 'text': 'stopped: ' + '; '.join(held)}]}
        return response
    return {}


def selftest():
    """withheld() judges a response only on a meter pass that began at least
    FRESH seconds after the call arrived (review of L3, round 2, SB-7): a
    stale reading under the hold must not be accepted while a fresh one
    past it is on its way, and a fresh one under the hold hands back."""
    import tempfile
    import threading
    global METER, STOP
    with tempfile.TemporaryDirectory(prefix='lead-tool-selftest-') as here:
        METER, STOP = os.path.join(here, 'lead-meter.json'), ''

        def write(total, started):
            with open(METER + '.tmp', 'w') as out:
                json.dump(dict(total=total, hold_above=95000, reports=1,
                               pass_started=started), out)
            os.replace(METER + '.tmp', METER)

        arrived = time.time()
        write(90000, arrived - 5)          # stale, and under the hold

        def fresh():
            while time.time() < arrived + FRESH + 0.05:
                time.sleep(0.05)
            write(120000, time.time())     # the report that came after it
        writer = threading.Thread(target=fresh)
        writer.start()
        why, seen = withheld(arrived)
        writer.join()
        assert seen and seen['total'] == 120000 and why, (why, seen)
        assert time.time() >= arrived + FRESH, 'judged before a fresh pass could exist'
        # A fresh pass under the hold hands the response back.
        arrived = time.time()
        write(90000, arrived + FRESH)
        why, seen = withheld(arrived)
        assert why == [] and seen['total'] == 90000, (why, seen)
    print('lead tool selftest: a stale meter reading was not taken; the fresh one past the '
          'hold withheld the response, and a fresh one under it handed it back')


def main():
    if '--selftest' in sys.argv[1:]:
        return selftest()
    note({'event': 'started', 'lead': LEAD})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except ValueError:
            continue
        note({'event': 'request', 'method': message.get('method')})
        if message.get('id') is None:
            continue
        sys.stdout.write(json.dumps(
            {'jsonrpc': '2.0', 'id': message['id'],
             'result': handle(message)}) + '\n')
        sys.stdout.flush()


if __name__ == '__main__':
    main()
