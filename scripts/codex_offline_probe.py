#!/usr/bin/env python3
"""Offline Codex app-server probe: no turn, no model call, isolated CODEX_HOME.

Qualifies the selected executable, then starts its app-server once per case with
an isolated CODEX_HOME and credential-free environment, sends only `initialize`
and `thread/start` for a throwaway fixture repository, and records the Codex
configuration before and after. The user's real Codex home is never read or
written. Raw transcripts, configuration copies and paths stay under
OUT/private; OUT/summary.json holds digests and redacted observations.

Cases, each in its own isolated home:

- `requested_workspace_write_on_request`: the settings live runs request.
- `absent_settings_default`: no `sandbox` or `approvalPolicy` in `thread/start`
  and an empty configuration, so the response reports the version's own
  trusted-project defaults. This is what the thread-settings guard assumes for
  an absent setting, measured rather than read from source.
- `requested_untrusted`: `approvalPolicy: "untrusted"` requested per thread,
  which the R5 and R6 plan needs.
- `configured_untrusted`: `approval_policy = "untrusted"` in the configuration,
  which 0.155.1 rejects (`UnsupportedUntrustedApprovalPolicyError`).
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / 'target/debug/pio'


def pio(*args):
    result = subprocess.run([str(BINARY), *map(str, args)], capture_output=True, text=True)
    return result.returncode, result.stdout, result.stderr


class AppServer:
    def __init__(self, argv, env, transcript, stderr):
        self.process = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, env=env)
        self.transcript = transcript
        self.lines = []
        self.lock = threading.Condition()
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for raw in self.process.stdout:
            message = json.loads(raw)
            with self.transcript.open('a') as out:
                out.write(json.dumps({'direction': 'server', 'message': message}) + '\n')
            with self.lock:
                self.lines.append(message)
                self.lock.notify_all()

    def send(self, message):
        with self.transcript.open('a') as out:
            out.write(json.dumps({'direction': 'client', 'message': message}) + '\n')
        self.process.stdin.write((json.dumps(message) + '\n').encode())
        self.process.stdin.flush()

    def response(self, request_id, seconds):
        deadline = time.monotonic() + seconds
        with self.lock:
            while True:
                for message in self.lines:
                    if message.get('id') == request_id and ('result' in message or 'error' in message):
                        return message
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f'no response to request {request_id}')
                self.lock.wait(remaining)


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


CASES = [
    dict(name='requested_workspace_write_on_request', config=None,
         request={'sandbox': 'workspace-write', 'approvalPolicy': 'on-request'}),
    dict(name='absent_settings_default', config=None, request={}),
    dict(name='requested_untrusted', config=None,
         request={'sandbox': 'workspace-write', 'approvalPolicy': 'untrusted'}),
    dict(name='configured_untrusted', config='approval_policy = "untrusted"\n',
         request={'sandbox': 'workspace-write', 'approvalPolicy': 'on-request'}),
    dict(name='absent_settings_trusted_project', config='{trusted_project}', request={}),
]


def probe(case, executable, private):
    """Run one case against its own isolated CODEX_HOME. No turn is sent."""
    root = private / case['name']
    root.mkdir(mode=0o700)
    home = root / 'codex-home'
    home.mkdir(mode=0o700)
    fixtures = root / 'fixtures'
    repo = fixtures / 'probe-repo'
    repo.mkdir(parents=True)
    (repo / 'README.md').write_text('PIO offline probe fixture. No task runs here.\n')
    subprocess.run(['git', 'init', '-q', str(repo)], check=True)
    config = case['config']
    if config == '{trusted_project}':
        # The version's default for an already trusted project, which is what
        # the thread-settings guard assumes when a setting is absent.
        config = f'[projects."{repo}"]\ntrust_level = "trusted"\n'
    if config:
        (home / 'config.toml').write_text(config)

    code, before, _ = pio('codex', 'config-snapshot', '--codex-home', home)
    assert code == 0
    (root / 'config-before.json').write_text(before)

    env = {k: v for k, v in os.environ.items() if 'API_KEY' not in k and 'OPENAI' not in k and 'TOKEN' not in k}
    env['CODEX_HOME'] = str(home)
    transcript = root / 'app-server-transcript.jsonl'
    stderr = (root / 'app-server.stderr').open('wb')
    server = AppServer([str(executable), 'app-server'], env, transcript, stderr)
    started = time.monotonic()
    initialized, thread = {}, {}
    timed_out = None
    try:
        server.send({'method': 'initialize', 'id': 0, 'params': {'clientInfo': {'name': 'pio_offline_probe', 'title': 'PIO offline probe', 'version': '0.1.0-dev'}}})
        initialized = server.response(0, 30)
        server.send({'method': 'initialized'})
        server.send({'method': 'thread/start', 'id': 1, 'params': dict(cwd=str(repo), **case['request'])})
        thread = server.response(1, 60)
        time.sleep(1)
    except TimeoutError as error:
        # A refusing app-server exits instead of answering; that is an
        # observation, not a probe failure.
        timed_out = str(error)
    try:
        server.process.stdin.close()
    except (BrokenPipeError, OSError):
        pass
    try:
        exit_code = server.process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        server.process.terminate()
        exit_code = server.process.wait(timeout=10)
    stderr.close()
    elapsed = time.monotonic() - started

    code, after, _ = pio('codex', 'config-snapshot', '--codex-home', home)
    assert code == 0
    (root / 'config-after.json').write_text(after)
    code, diff, err = pio('codex', 'config-diff', root / 'config-before.json', root / 'config-after.json', '--fixture-root', fixtures)
    assert code == 0, err
    methods = [m.get('method') for m in server.lines if 'method' in m]
    requests = [m.get('method') for m in server.lines if 'method' in m and 'id' in m]
    result = thread.get('result', {})
    stderr_text = (root / 'app-server.stderr').read_text(errors='replace')
    return dict(
        case=case['name'], configured=config, requested=case['request'],
        exit=exit_code, seconds=round(elapsed, 3), timed_out=timed_out,
        credential_environment_removed=True,
        initialize=dict(ok='result' in initialized, result_keys=sorted(initialized.get('result', {}))),
        thread_start=dict(ok='result' in thread, error=thread.get('error'), result_keys=sorted(result),
                          thread_id_present=bool(result.get('thread', {}).get('id')),
                          sandbox_projection=result.get('sandbox'), approval_policy=result.get('approvalPolicy'),
                          model=result.get('model'),
                          model_provider=result.get('modelProvider') or result.get('thread', {}).get('modelProvider')),
        notifications=sorted(set(methods) - set(requests)), server_requests=requests,
        stderr_mentions_untrusted='approval_policy = "untrusted" is no longer supported' in stderr_text,
        stderr_sha256=digest(stderr_text),
        transcript_sha256=hashlib.sha256(transcript.read_bytes()).hexdigest(),
        config_diff=json.loads(diff))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', type=Path, default=shutil.which('codex'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    assert args.executable, 'no codex executable selected'
    args.out.mkdir(parents=True, exist_ok=False)
    private = args.out / 'private'
    private.mkdir(mode=0o700)

    code, out, err = pio('codex', 'qualify', '--executable', args.executable, '--work', private / 'qualify')
    (private / 'qualification.json').write_text(out)
    qualification = json.loads(out)
    resolution = qualification.get('resolution', {})
    summary = dict(format='pio-codex-offline-probe/2', platform=platform.platform(), turn_started=False,
                   model_request_sent=False, user_codex_home_used=False,
                   pinned=qualification.get('pinned'),
                   qualification=dict(exit=code, qualified=qualification['qualified'], refusals=qualification['refusals'],
                                      native_sha256=resolution.get('native', {}).get('sha256'),
                                      wrapper_sha256=resolution.get('wrapper', {}).get('sha256'),
                                      node=dict(sha256=(resolution.get('node') or {}).get('sha256'),
                                                version=(resolution.get('node') or {}).get('version')),
                                      canonical_listing_sha256=qualification.get('schema', {}).get('canonical_listing_sha256')))
    if code != 0:
        (args.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
        raise SystemExit('executable not qualified; app-server not started')

    summary['cases'] = [probe(case, args.executable, private) for case in CASES]
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
