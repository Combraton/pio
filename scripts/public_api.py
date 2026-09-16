"""Small Protocol/1 test caller. Commands use frozen envelopes, never diagnostics."""
import hashlib
import json
import socket
import uuid

CREDENTIAL = 'ccred1.owner.' + 'm'*43
FEATURES = ['core.events','core.capabilities','core.effects']
def digest(value):
    return 'sha256:' + hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()

def command(operation, subject, payload, command_id='work', revision=0, epoch=0):
    intent = dict(operation=operation,subject=subject,preconditions=[dict(subject=subject,revision=revision)],requires=[],payload=payload,extensions={})
    result = dict(intent, message_id=str(uuid.uuid4()), command_id=command_id, command_digest=digest(intent),dedupe_generation=1,authority_epoch=epoch)
    return result

def submit(duration=10000, identity='work'):
    return command('execution.submit',dict(kind='execution.execution',id=identity),dict(brief=dict(digest='sha256:'+hashlib.sha256(f'fake work {duration}'.encode()).hexdigest(),media_type='text/plain')),command_id=identity)

class Client:
    def __init__(self, path, transcript=None):
        self.stream = socket.socket(socket.AF_UNIX)
        self.stream.settimeout(3)
        self.stream.connect(str(path))
        self.file = self.stream.makefile('rb')
        self.transcript = transcript
        self.query('core.authenticate',dict(credential=CREDENTIAL))
        self.query('core.negotiate',dict(profiles=[dict(name='core',majors=[1],required=True,required_features=FEATURES,optional_features=[]),dict(name='execution',majors=[1],required=True,required_features=['execution.controller','execution.output','execution.discovery'],optional_features=[])],caller=dict(name="pio-matrix",version="1"),receive_limits=dict(max_frame_bytes=1048576)))
    def call(self, envelope):
        frame = dict(jsonrpc='2.0',id=str(uuid.uuid4()),method=envelope['operation'],params=envelope)
        self.stream.sendall(json.dumps(frame).encode()+b'\n')
        while True:
            line = self.file.readline()
            if not line: raise EOFError('service closed socket')
            result = json.loads(line)
            if result.get('id') == frame['id']: break
        if self.transcript:
            # Credentials are test-only, but redact even these in receipts.
            logged = json.loads(json.dumps(frame))
            if frame['method']=='core.authenticate': logged['params']['payload']['credential']='<test credential redacted>'
            with self.transcript.open('a') as out: out.write(json.dumps(dict(request=logged,response=result))+'\n')
        return result
    def query(self, operation, payload):
        return self.call(dict(operation=operation,message_id=str(uuid.uuid4()),payload=payload))
    def close(self):
        self.file.close(); self.stream.close()
    def __enter__(self): return self
    def __exit__(self,*_): self.close()
