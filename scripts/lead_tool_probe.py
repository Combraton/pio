#!/usr/bin/env python3
"""Does a harness actually honour a lead tool registered at session start?

Owner approval `owner-2026-09-22-m4b-lead-tool`, item 1 as amended: **one**
`session/new` against OpenCode and **one** `thread/start` against Codex,
each on a private standalone instance with PIO's own config and home, **no
prompt and no model call**, and the owner's background service recorded
before and after and asserted unmoved.

The question is the same for both, and it is not answerable from a help
surface or a binary: OpenCode's `session/new` **parses** `mcpServers` (the
validator is in the shipped binary, beside `cwd` and `additionalDirectories`)
and Codex's `thread/start` takes a free-form `config` that overrides what
would be read from `config.toml`. Both **accept** a registration. Whether
either **launches** the server and offers its tools is a different claim, and
the only witness that settles it is the server itself: this probe registers a
tool that does nothing but write down every request it receives, and then
reads that file.

No model is called, because nothing is prompted. A session or thread is
started and immediately closed.
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup

ROOT = Path(__file__).resolve().parents[1]

# The witness. An MCP server over stdio that answers `initialize` and
# `tools/list` and writes every request it is handed to a log. It runs no
# command, reads no file and calls no model.
WITNESS = '''#!/usr/bin/env python3
import json, os, sys
log = os.environ["PIO_WITNESS_LOG"]
def note(entry):
    with open(log, "a") as handle:
        handle.write(json.dumps(entry) + "\\n")
note({"event": "started", "argv": sys.argv[1:]})
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        message = json.loads(line)
    except ValueError:
        continue
    note({"event": "request", "method": message.get("method")})
    if message.get("id") is None:
        continue
    if message.get("method") == "initialize":
        result = {"protocolVersion": "2024-11-05",
                  "capabilities": {"tools": {}},
                  "serverInfo": {"name": "pio-lead-witness", "version": "0"}}
    elif message.get("method") == "tools/list":
        result = {"tools": [{"name": "pio_lead",
                             "description": "PIO lead tool (probe witness)",
                             "inputSchema": {"type": "object", "properties": {}}}]}
    else:
        result = {}
    sys.stdout.write(json.dumps(
        {"jsonrpc": "2.0", "id": message["id"], "result": result}) + "\\n")
    sys.stdout.flush()
'''


def owner_service():
    """The owner's own OpenCode background service, before and after."""
    out = subprocess.run(['ps', '-Ao', 'pid,lstart,command'],
                         capture_output=True, text=True).stdout
    return sorted(line.strip() for line in out.splitlines()
                  if 'serve --service' in line and 'opencode' in line
                  and 'grep' not in line)


class Probe:
    """One private instance, with PIO's own home and config."""

    def __init__(self, out, label):
        self.out = out / label
        self.out.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='pio-probe-', dir='/tmp')).resolve()
        os.chmod(self.root, 0o700)
        self.log = self.root / 'witness.jsonl'
        self.witness = self.root / 'witness.py'
        self.witness.write_text(WITNESS)
        self.witness.chmod(0o755)
        self.cwd = self.root / 'work'
        self.cwd.mkdir()
        # Codex refuses to start if CODEX_HOME does not exist, so PIO's own
        # one is made here rather than borrowed from the owner.
        (self.root / 'codex-home').mkdir()
        (self.cwd / 'README.md').write_text('probe fixture\n')
        self.owner_before = owner_service()

    def env(self, **extra):
        return dict(PATH='/usr/bin:/bin:/usr/local/bin', HOME=str(self.root),
                    PIO_WITNESS_LOG=str(self.log), **extra)

    def witnessed(self):
        if not self.log.exists():
            return []
        return [json.loads(l) for l in self.log.read_text().splitlines() if l.strip()]

    def finish(self):
        assert owner_service() == self.owner_before, \
            "the owner's OpenCode service moved during this probe"

    def release(self):
        case_cleanup.release(self.root)


def speak(child, message, seconds=30):
    """One JSON-RPC request on stdio, and its answer."""
    child.stdin.write(json.dumps(message) + '\n')
    child.stdin.flush()
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        line = child.stdout.readline()
        if not line:
            return None
        try:
            answer = json.loads(line)
        except ValueError:
            continue
        if answer.get('id') == message.get('id'):
            return answer
    return None


def probe_opencode(out, executable):
    """One `session/new` with the witness registered. No prompt."""
    probe = Probe(out, 'opencode')
    record = {'harness': 'opencode acp', 'prompted': False, 'model_calls': 0}
    try:
        child = subprocess.Popen(
            [executable, 'acp'], cwd=str(probe.cwd), env=probe.env(),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=(probe.out / 'stderr.log').open('w'), text=True, bufsize=1)
        try:
            record['initialize'] = bool(speak(child, {
                'jsonrpc': '2.0', 'id': 1, 'method': 'initialize',
                'params': {'protocolVersion': 1, 'clientCapabilities': {}}}))
            answer = speak(child, {
                'jsonrpc': '2.0', 'id': 2, 'method': 'session/new',
                'params': {'cwd': str(probe.cwd),
                           'mcpServers': [{'name': 'pio-lead',
                                           'command': sys.executable,
                                           'args': [str(probe.witness)],
                                           'env': [{'name': 'PIO_WITNESS_LOG',
                                                    'value': str(probe.log)}]}]}},
                seconds=60)
            record['session_new'] = 'accepted' if answer and 'result' in answer \
                else ('refused' if answer else 'no answer')
            if answer and 'error' in answer:
                record['error'] = answer['error']
            # The witness may take a moment to be launched.
            for _ in range(40):
                if probe.witnessed():
                    break
                time.sleep(0.25)
        finally:
            child.kill()
            child.wait(timeout=10)
        record['witness'] = probe.witnessed()
        record['honoured'] = any(w.get('method') == 'initialize'
                                 for w in record['witness'])
        probe.finish()
        record['owner_service_untouched'] = True
    finally:
        probe.release()
    return record


def probe_codex(out, executable):
    """One `thread/start` with the witness registered. No prompt."""
    probe = Probe(out, 'codex')
    record = {'harness': 'codex app-server', 'prompted': False, 'model_calls': 0}
    try:
        child = subprocess.Popen(
            [executable, 'app-server'], cwd=str(probe.cwd),
            env=probe.env(CODEX_HOME=str(probe.root / 'codex-home')),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=(probe.out / 'stderr.log').open('w'), text=True, bufsize=1)
        try:
            record['initialize'] = bool(speak(child, {
                'jsonrpc': '2.0', 'id': 1, 'method': 'initialize',
                'params': {'clientInfo': {'name': 'pio-probe', 'version': '0'}}}))
            child.stdin.write(json.dumps(
                {'jsonrpc': '2.0', 'method': 'initialized'}) + '\n')
            child.stdin.flush()
            answer = speak(child, {
                'jsonrpc': '2.0', 'id': 2, 'method': 'thread/start',
                'params': {'cwd': str(probe.cwd),
                           # The per-thread route: a `config` object that
                           # overrides what would be read from config.toml.
                           # `approvalsReviewer` is never set.
                           'config': {'mcp_servers': {
                               'pio_lead': {'command': sys.executable,
                                            'args': [str(probe.witness)],
                                            'env': {'PIO_WITNESS_LOG': str(probe.log)}}}}}},
                seconds=60)
            record['thread_start'] = 'accepted' if answer and 'result' in answer \
                else ('refused' if answer else 'no answer')
            if answer and 'error' in answer:
                record['error'] = answer['error']
            if answer and 'result' in answer:
                record['approvals_reviewer'] = answer['result'].get('approvalsReviewer')
            for _ in range(40):
                if probe.witnessed():
                    break
                time.sleep(0.25)
        finally:
            child.kill()
            child.wait(timeout=10)
        record['witness'] = probe.witnessed()
        record['honoured'] = any(w.get('method') == 'initialize'
                                 for w in record['witness'])
        probe.finish()
        record['owner_service_untouched'] = True
    finally:
        probe.release()
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/lead-tool-probe')
    parser.add_argument('--opencode', default=str(Path.home() /
                                                  '.local/lib/node_modules/@opencode/cli/bin/opencode.exe'))
    parser.add_argument('--codex', default='codex')
    parser.add_argument('--only', choices=['opencode', 'codex'])
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    record = {}
    if args.only != 'codex':
        record['opencode'] = probe_opencode(args.out, args.opencode)
    if args.only != 'opencode':
        record['codex'] = probe_codex(args.out, args.codex)
    (args.out / 'lead-tool-probe.json').write_text(
        json.dumps(record, indent=2, sort_keys=True) + '\n')
    print(json.dumps(record, indent=2, sort_keys=True))


if __name__ == '__main__':
    main()
