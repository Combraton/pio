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
past the kill is a model step the lead cannot take in between.
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
# A third of the MCP TypeScript SDK's default request timeout (60 seconds).
# What OpenCode itself allows a tool call is not measured; the live run's
# tool log shows whether a waiting read came back.
READ_WAIT = 20
TEXT_LIMIT = 2000
CALL_CEILING = int(os.environ.get('PIO_LEAD_CALL_CEILING') or 0)
STOP = os.environ.get('PIO_LEAD_STOP', '')
HOLD = 30
CALLS = []


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
    body = brief.encode()
    api = Api()
    answer = api.call(
        'execution.submit',
        dict(brief=dict(digest=digest(body), media_type='text/plain'),
             workspace=dict(repository=WORKSPACE, base=BASE, cleanup='retain'),
             timeouts=dict(delivery=300, execution_deadline=900),
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


def message_text(raw):
    """What a run said, from its spooled records.

    OpenCode spools one record per `session/update`; the words are the
    `agent_message_chunk`s. Handing the lead raw JSON would spend its tokens
    on framing, and it is the same decoding a screen does.
    """
    words = []
    for line in raw.splitlines():
        try:
            update = json.loads(line).get('update', {})
        except ValueError:
            continue
        if update.get('sessionUpdate') == 'agent_message_chunk':
            words.append(update.get('content', {}).get('text', ''))
    return ''.join(words)


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
        if name not in TOOLS:
            return {'isError': True,
                    'content': [{'type': 'text', 'text': f'no tool {name}'}]}
        try:
            result = TOOLS[name][0](**arguments)
        except Exception as error:  # reported, never swallowed
            note({'event': 'tool_failed', 'tool': name, 'error': repr(error)})
            return {'isError': True,
                    'content': [{'type': 'text', 'text': repr(error)}]}
        note({'event': 'tool_call', 'tool': name, 'arguments': arguments,
              'result': result})
        return {'content': [{'type': 'text', 'text': json.dumps(result)}]}
    return {}


def main():
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
