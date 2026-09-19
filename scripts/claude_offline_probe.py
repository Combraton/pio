#!/usr/bin/env python3
"""Offline Claude Code probe: no model call, no credential read, isolated config.

Records three things the M3 adapter design rests on, at zero model tokens:

1. the command-line surface identity of the selected executable;
2. the credential route, from `claude auth status`, with the account identity
   fields dropped at this boundary and never written anywhere;
3. the stream-json shapes, from a launch in an isolated `CLAUDE_CONFIG_DIR`
   with no credential variables, which fails authentication before any model
   call and therefore spends nothing.

The user's real configuration is read only by step 2, which makes no model call
and is verified here to leave the configuration byte-identical. Raw transcripts
stay under OUT/private; OUT/summary.json holds digests and redacted facts.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

# `auth status` prints these; they identify the account and never leave here.
ACCOUNT_IDENTITY = ('email', 'orgId', 'orgName', 'organizationName', 'accountUuid', 'userID')
SURFACE_COMMANDS = ([], ['auth'], ['mcp'], ['plugin'], ['project'], ['doctor'], ['agents'], ['install'])


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def run(argv, env=None, stdin=None, seconds=120):
    return subprocess.run(argv, capture_output=True, text=True, input=stdin,
                          env=env, timeout=seconds)


def surface(executable):
    """Canonical digest of the CLI surface: cheap, deterministic, and it moves
    when a self-update changes the interface PIO drives."""
    digests, repeats = {}, {}
    for command in SURFACE_COMMANDS:
        name = '<top>' if not command else command[0]
        digests[name] = sha(run([str(executable), *command, '--help']).stdout)
        if not command:
            repeats[name] = sorted({sha(run([str(executable), '--help']).stdout) for _ in range(3)})
    listing = ''.join(f'{k}\t{v}\n' for k, v in sorted(digests.items()))
    return dict(commands=sorted(digests), digests=digests,
                surface_listing_sha256=sha(listing),
                top_help_digests_over_3_runs=repeats.get('<top>', []))


def auth_route(executable, config_dir=None):
    """Observe the route. Never read a credential file or the keychain."""
    env = {k: v for k, v in os.environ.items()
           if 'ANTHROPIC' not in k and 'API_KEY' not in k and 'TOKEN' not in k}
    if config_dir:
        env['CLAUDE_CONFIG_DIR'] = str(config_dir)
        env['HOME'] = str(config_dir)
    result = run([str(executable), 'auth', 'status'], env=env)
    try:
        status = json.loads(result.stdout)
    except json.JSONDecodeError:
        return dict(exit=result.returncode, parsed=False)
    dropped = sorted(k for k in status if k in ACCOUNT_IDENTITY)
    kept = {k: v for k, v in status.items()
            if k in ('loggedIn', 'authMethod', 'apiProvider', 'subscriptionType')}
    return dict(exit=result.returncode, parsed=True, observed=kept,
                account_identity_fields_dropped=dropped)


def stream_shapes(executable, root):
    """Launch with an isolated config and no credentials: authentication fails
    before any model call, so the message shapes cost nothing."""
    home = root / 'config'
    home.mkdir(parents=True)
    repo = root / 'fixture'
    repo.mkdir()
    (repo / 'README.md').write_text('PIO M3 offline probe fixture. No task runs here.\n')
    subprocess.run(['git', 'init', '-q', str(repo)], check=True)
    env = {k: v for k, v in os.environ.items()
           if 'ANTHROPIC' not in k and 'API_KEY' not in k and 'TOKEN' not in k}
    env.update(CLAUDE_CONFIG_DIR=str(home), HOME=str(home))
    brief = 'reply with the single word probe'
    message = json.dumps({'type': 'user', 'message': {'role': 'user',
                                                      'content': [{'type': 'text', 'text': brief}]}})
    result = run([str(executable), '--print', '--input-format', 'stream-json',
                  '--output-format', 'stream-json', '--verbose', '--replay-user-messages',
                  '--permission-prompts', 'none', '--permission-mode', 'acceptEdits'],
                 env=env, stdin=message + '\n')
    (root / 'transcript.jsonl').write_text(result.stdout)
    messages = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    init = next((m for m in messages if m.get('subtype') == 'init'), {})
    final = next((m for m in messages if m.get('type') == 'result'), {})
    replay = next((m for m in messages if m.get('type') == 'user' and m.get('isReplay')), None)
    failure = next((m for m in messages if m.get('error')), {})
    return dict(
        exit=result.returncode, brief_sha256=sha(brief),
        message_sequence=[f"{m.get('type')}/{m.get('subtype')}" if m.get('subtype') else m.get('type')
                          for m in messages],
        init_keys=sorted(init), init_facts={k: init.get(k) for k in
                                            ('apiKeySource', 'permissionMode', 'model',
                                             'claude_code_version', 'capabilities')},
        init_name_lists={k: len(init.get(k) or []) for k in
                         ('tools', 'mcp_servers', 'plugins', 'slash_commands', 'skills', 'agents')},
        replay_acknowledged=bool(replay),
        replay_matches_sent=bool(replay) and replay['message'] == json.loads(message)['message'],
        authentication_failure={k: failure.get(k) for k in ('error', 'is_api_error_message')},
        result_keys=sorted(final), is_error=final.get('is_error'),
        terminal_reason=final.get('terminal_reason'),
        usage_at_turn_end=final.get('usage'), model_usage=final.get('modelUsage'),
        total_cost_usd=final.get('total_cost_usd'),
        permission_denials=final.get('permission_denials'),
        transcript_sha256=sha(result.stdout))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--executable', type=Path, default=shutil.which('claude'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    assert args.executable, 'no claude executable selected'
    args.out.mkdir(parents=True, exist_ok=False)
    private = args.out / 'private'
    private.mkdir(mode=0o700)

    selected = Path(args.executable)
    resolved = selected.resolve()
    settings = Path.home() / '.claude/settings.json'
    state = Path.home() / '.claude.json'
    before = {p.name: sha(p.read_bytes()) for p in (settings, state) if p.exists()}

    summary = dict(
        format='pio-claude-offline-probe/1', platform=platform.platform(),
        model_calls=0, turn_completed=False, credentials_read=False,
        executable=dict(selected=str(selected), resolved=str(resolved),
                        version=run([str(selected), '--version']).stdout.strip(),
                        binary_sha256=sha(resolved.read_bytes()),
                        resolved_path_carries_version=resolved.parent.name in
                        run([str(selected), '--version']).stdout),
        cli_surface=surface(selected),
        configured_route=auth_route(selected),
        missing_route_control=auth_route(selected, private / 'no-credentials'),
        stream=stream_shapes(selected, private / 'stream'))

    after = {p.name: sha(p.read_bytes()) for p in (settings, state) if p.exists()}
    summary['user_configuration_unchanged'] = before == after
    summary['user_configuration_digests'] = before
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n')
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == '__main__':
    main()
