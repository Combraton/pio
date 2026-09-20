#!/usr/bin/env python3
"""Zero-token OpenCode 2.0.1 probe for M3b (ADR 005).

Measures everything the adapter design rests on without a single model call:
the install and version, the ACP stdio transport's `initialize` and
`session/new` shapes, the configured default model and the available model
ids, which environment isolates the user's configuration, and the outward
behaviour of a private instance.

**It never touches the owner's background service.** The owner runs
`opencode serve --service`; this records that process's identity before and
after every probe and fails if it moved. `opencode acp` spawns its own
`serve --stdio --port 0` child, so isolation here is structural rather than a
flag, and the probe records the child to prove it.

PIO never reads OpenCode's credential store, never reads the Keychain and
never passes a key. This probe reads only the user's non-secret
`opencode.jsonc`, and never `cli.json` or `service.json`.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import subprocess
import threading
import time

SERVICE_PATTERN = 'opencode.exe serve --service'
# Never passed to a child: OpenCode finds its own credentials, PIO supplies none.
CREDENTIAL_MARKERS = ('API_KEY', 'TOKEN', 'SECRET', 'PASSWORD', 'CREDENTIAL')


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def owner_service():
    """The owner's service, by pid and start time. Read-only, always."""
    out = subprocess.run(['ps', '-Ao', 'pid,lstart,command'], capture_output=True, text=True).stdout
    return sorted(line.strip() for line in out.splitlines()
                  if SERVICE_PATTERN in line and 'grep' not in line)


class Acp:
    """One ACP stdio session against a private instance."""

    def __init__(self, exe, work, env):
        self.messages = []
        self.child = subprocess.Popen([str(exe), 'acp'], cwd=str(work), env=env,
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      stderr=subprocess.PIPE, text=True, bufsize=1)
        self.queue = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.counter = 0

    def _read(self):
        for line in self.child.stdout:
            self.queue.put(line)

    def call(self, method, params, seconds=45):
        self.counter += 1
        self.child.stdin.write(json.dumps(
            {'jsonrpc': '2.0', 'id': self.counter, 'method': method, 'params': params}) + '\n')
        self.child.stdin.flush()
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            try:
                message = json.loads(self.queue.get(timeout=1))
            except (queue.Empty, ValueError):
                continue
            self.messages.append(message)
            if message.get('id') == self.counter:
                return message
        return None

    def processes(self):
        out = subprocess.run(['ps', '-Ao', 'pid,ppid,command'], capture_output=True, text=True).stdout
        return [line.strip() for line in out.splitlines()
                if 'opencode' in line and 'grep' not in line
                and str(self.child.pid) in line.split()[:2]]

    def close(self):
        self.child.kill()
        self.child.wait(timeout=10)


def session(exe, work, env, want_processes=False):
    acp = Acp(exe, work, env)
    try:
        initialize = acp.call('initialize', {
            'protocolVersion': 1,
            'clientCapabilities': {'fs': {'readTextFile': True, 'writeTextFile': True}}})
        children = acp.processes() if want_processes else []
        created = acp.call('session/new', {'cwd': str(work), 'mcpServers': []})
        result = (created or {}).get('result') or {}
        options = {o['id']: o for o in result.get('configOptions', [])}
        models = [v['value'] for v in options.get('model', {}).get('options', [])]
        return dict(
            initialized=bool((initialize or {}).get('result')),
            agent_info=(initialize or {}).get('result', {}).get('agentInfo'),
            agent_capabilities=(initialize or {}).get('result', {}).get('agentCapabilities'),
            auth_methods=(initialize or {}).get('result', {}).get('authMethods'),
            session_created=bool(result.get('sessionId')),
            config_option_ids=sorted(options),
            configured_model=options.get('model', {}).get('currentValue'),
            mode=options.get('mode', {}).get('currentValue'),
            modes=[v['value'] for v in options.get('mode', {}).get('options', [])],
            effort=options.get('effort', {}).get('currentValue'),
            model_count=len(models),
            minimax_models=sorted(m for m in models if m.startswith('minimax')),
            update_kinds=sorted({m['params']['update']['sessionUpdate']
                                 for m in acp.messages if m.get('method') == 'session/update'}),
            private_server_children=children)
    finally:
        acp.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--executable', type=Path,
                        default=Path.home() / '.local/bin/opencode2')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    work = args.out / 'fixture'
    work.mkdir(exist_ok=True)
    (work / 'README.md').write_text('PIO M3b offline fixture. No task runs here.\n')
    subprocess.run(['git', 'init', '-q', str(work)], check=True)

    before = owner_service()
    resolved = args.executable.resolve()
    base = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'HOME': str(Path.home())}
    isolated_dir = args.out / 'isolated-config'
    isolated_dir.mkdir(exist_ok=True)
    isolated = {'PATH': base['PATH'], 'HOME': str(isolated_dir),
                'OPENCODE_CONFIG_DIR': str(isolated_dir)}

    config_path = Path.home() / '.config/opencode/opencode.jsonc'
    config = json.loads(''.join(
        line for line in config_path.read_text().splitlines(keepends=True)
        if not line.strip().startswith('//'))) if config_path.exists() else {}

    summary = dict(
        format='pio-opencode-probe/1', platform=platform.platform(),
        model_calls=0, turn_completed=False, credentials_read=False,
        executable=dict(selected=str(args.executable), resolved=str(resolved),
                        version=subprocess.run([str(args.executable), '--version'],
                                               capture_output=True, text=True).stdout.strip(),
                        install='npm'),
        # The user's own configuration, read from the non-secret file only.
        configured=dict(model=config.get('model'),
                        providers=sorted(config.get('provider', {})),
                        permission_rules_configured='permission' in config),
        as_configured=session(args.executable, work, base, want_processes=True),
        # The negative control: an isolated configuration directory.
        isolated_control=session(args.executable, work, isolated),
    )
    after = owner_service()
    summary['owner_service_before'] = [sha(line) for line in before]
    summary['owner_service_after'] = [sha(line) for line in after]
    summary['owner_service_untouched'] = before == after and bool(before)
    summary['credential_variables_passed'] = sorted(
        name for env in (base, isolated) for name in env
        if any(marker in name.upper() for marker in CREDENTIAL_MARKERS))
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n')
    print(json.dumps(summary, indent=2, sort_keys=True))
    assert summary['owner_service_untouched'], 'the owner service moved during the probe'
    assert not summary['credential_variables_passed'], 'a credential variable was passed'


if __name__ == '__main__':
    main()
