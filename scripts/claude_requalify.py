#!/usr/bin/env python3
"""Re-qualify the installed Claude Code, at zero model tokens, in one command.

The cask tracks latest and self-updates, so this must be cheap and it must be
one thing to run. It binds both halves of the interface PIO drives:

1. the **command-line surface** — the canonical digest of the top-level help
   plus each subcommand's help, compared against the committed surface identity;
2. the **stream shapes** — from a launch in an isolated `CLAUDE_CONFIG_DIR` with
   no credentials, which fails authentication before any model call and so
   spends nothing. The `system/init` key set and capability list are compared
   against the committed stream identity, because a help digest cannot see them.

It also records, as measurements rather than assumptions: the product default
permission mode and model with an empty configuration and no flags; whether
`system/init` arrives before anything is written to stdin; and the credential
route, with the account identity fields dropped at this boundary.

Every child process runs with a cleared environment — only PATH, HOME and
CLAUDE_CONFIG_DIR are passed, never a credential variable. The user's own
configuration is read only to observe the route and is verified byte-identical
before and after. Exit 0 clean, 3 on drift, 4 if the user's configuration moved.
"""
import argparse
import hashlib
import json
import os
import platform
import queue
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

# `auth status` prints these; they identify the account and never leave here.
ACCOUNT_IDENTITY = ('email', 'orgId', 'orgName', 'organizationName', 'accountUuid', 'userID')
ROUTE_FIELDS = ('loggedIn', 'authMethod', 'apiProvider', 'subscriptionType')
ENV_ALLOWLIST = ('PATH', 'HOME', 'USER', 'CLAUDE_CONFIG_DIR')
SURFACE_COMMANDS = ([], ['auth'], ['mcp'], ['plugin'], ['project'], ['doctor'], ['agents'], ['install'])
STREAM_ARGS = ['--print', '--input-format', 'stream-json', '--output-format', 'stream-json',
               '--verbose', '--replay-user-messages']
# The user's configuration lives in more than one file; a snapshot that misses
# one is not a snapshot. `settings.local.json` also carries permission rules.
USER_CONFIG = ('.claude/settings.json', '.claude/settings.local.json', '.claude.json')
BASE_PATH = '/usr/bin:/bin:/usr/sbin:/sbin'
# The control-protocol handshake, in the shape the pinned SDK sends it
# (`query.py`): a subtype and hooks, and nothing else when nothing is
# configured. No hooks, no agents, no system prompt snapshot, so attaching
# changes nothing about the session it attaches to.
INITIALIZE = {'type': 'control_request', 'request_id': 'req_init_probe',
              'request': {'subtype': 'initialize', 'hooks': None}}

# Set once by main(); every launch uses the selected executable.
EXECUTABLE = None


def sha(data):
    return hashlib.sha256(data if isinstance(data, bytes) else data.encode()).hexdigest()


def env_for(home, config_dir=None):
    """A cleared environment. Nothing is inherited: not PATH, not a credential
    variable, and not the CLAUDE_CODE_* variables an enclosing session exports.

    `USER` is passed because the credential route is not observable without it:
    measured, `auth status` reports `loggedIn: false` when `USER` is absent,
    even given the user's real `HOME`. Leaving it out would make a working login
    look missing and every refusal unfalsifiable.

    `CLAUDE_CONFIG_DIR` is set only when isolating. Measured: the product expects
    `.claude.json` inside a configured directory, while as the user has it that
    file sits at `~/.claude.json` beside `~/.claude/`, so pointing the variable
    at `~/.claude` makes the harness report the configuration missing.
    """
    env = {'PATH': BASE_PATH, 'HOME': str(home)}
    if 'USER' in os.environ:
        env['USER'] = os.environ['USER']
    if config_dir is not None:
        env['CLAUDE_CONFIG_DIR'] = str(config_dir)
    return env


def run(argv, env, stdin=None, seconds=120):
    return subprocess.run(argv, capture_output=True, text=True, input=stdin,
                          env=env, timeout=seconds)


def surface(executable, home):
    """Cheap, deterministic fingerprint of the interface PIO drives."""
    digests, repeats = {}, []
    for command in SURFACE_COMMANDS:
        name = '<top>' if not command else command[0]
        digests[name] = sha(run([str(executable), *command, '--help'], env_for(home)).stdout)
    repeats = sorted({sha(run([str(executable), '--help'], env_for(home)).stdout) for _ in range(3)})
    listing = ''.join(f'{k}\t{v}\n' for k, v in sorted(digests.items()))
    # Same shape the Rust adapter emits, so one artefact serves both producers.
    return dict(command_count=len(digests), commands=digests,
                surface_listing_sha256=sha(listing),
                top_help_digests_over_3_runs=repeats)


def auth_route(executable, home, config_dir=None):
    """Observe the route. Never read a credential file or the keychain."""
    result = run([str(executable), 'auth', 'status'], env_for(home, config_dir))
    try:
        status = json.loads(result.stdout)
    except json.JSONDecodeError:
        return dict(exit=result.returncode, parsed=False)
    return dict(exit=result.returncode, parsed=True,
                observed={k: v for k, v in status.items() if k in ROUTE_FIELDS},
                account_identity_fields_dropped=sorted(k for k in status if k in ACCOUNT_IDENTITY))


def _reader(pipe, q):
    for line in pipe:
        q.put((time.monotonic(), line))
    q.put(None)


def launch(root, label, extra_args=(), write=True, silence_seconds=0, control_first=None):
    """Launch with an isolated config and no credentials. Authentication fails
    before any model call, so the shapes cost nothing.

    `silence_seconds` holds stdin open without writing, to measure whether
    `system/init` arrives on its own or waits for input.
    """
    home = root / label
    shutil.rmtree(home, ignore_errors=True)
    home.mkdir(parents=True)
    work = root / f'{label}-work'
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir()
    (work / 'README.md').write_text('PIO re-qualification fixture. No task runs here.\n')
    brief = 'reply with the single word probe'
    message = json.dumps({'type': 'user', 'message': {'role': 'user',
                                                      'content': [{'type': 'text', 'text': brief}]}})
    child = subprocess.Popen([str(EXECUTABLE), *STREAM_ARGS, *extra_args], cwd=str(work),
                             env=env_for(home, home), stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             text=True, bufsize=1)
    q = queue.Queue()
    threading.Thread(target=_reader, args=(child.stdout, q), daemon=True).start()
    messages, silent_until, wrote_at, init, init_at = [], time.monotonic() + silence_seconds, None, None, None

    def drain(until):
        nonlocal init, init_at
        while time.monotonic() < until:
            try:
                item = q.get(timeout=0.5)
            except queue.Empty:
                continue
            if item is None:
                return True
            at, line = item
            if not line.strip():
                continue
            msg = json.loads(line)
            messages.append(msg)
            if msg.get('subtype') == 'init' and init is None:
                init, init_at = msg, at
        return False

    drain(silent_until)
    messages_before_write = len(messages)
    # The handshake, before anything else is written. A host that never
    # announces itself is not a host: measured on R3b, the CLI then denies
    # anything that would prompt and PIO never learns it was asked.
    control_answered_in = None
    if control_first is not None:
        sent_at = time.monotonic()
        child.stdin.write(json.dumps(control_first) + '\n')
        child.stdin.flush()
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline and not any(
                m.get('type') == 'control_response' for m in messages):
            if drain(time.monotonic() + 0.5):
                break
        if any(m.get('type') == 'control_response' for m in messages):
            control_answered_in = round(time.monotonic() - sent_at, 3)
    if write:
        wrote_at = time.monotonic()
        child.stdin.write(message + '\n')
        child.stdin.flush()
    try:
        child.stdin.close()
    except (OSError, ValueError):
        pass
    drain(time.monotonic() + 60)
    try:
        child.wait(timeout=30)
    except subprocess.TimeoutExpired:
        child.kill()
    init = init or {}
    stderr_text = ''
    try:
        stderr_text = child.stderr.read() or ''
    except (OSError, ValueError):
        pass
    answer = next((m for m in messages if m.get('type') == 'control_response'), {})
    inner = answer.get('response') or {}
    final = next((m for m in messages if m.get('type') == 'result'), {})
    replay = next((m for m in messages if m.get('type') == 'user' and m.get('isReplay')), None)
    failure = next((m for m in messages if m.get('error')), {})
    return dict(
        label=label, extra_args=list(extra_args), exit=child.returncode,
        brief_sha256=sha(brief),
        # A rejected flag is a usage error on stderr and a fast non-zero exit,
        # so the flag is recorded as accepted only when neither happened.
        stderr_sha256=sha(stderr_text) if stderr_text else None,
        stderr_mentions_unknown_option=('unknown option' in stderr_text.lower()
                                        or 'unknown argument' in stderr_text.lower()),
        initialize_sent=control_first is not None,
        initialize_answered=bool(answer),
        initialize_answered_in_seconds=control_answered_in,
        initialize_response_subtype=inner.get('subtype'),
        # Keys only. The handshake's payload is the CLI's, not PIO's to record.
        initialize_response_keys=sorted(inner),
        initialize_response_error=inner.get('error'),
        messages_before_any_stdin_write=messages_before_write,
        held_stdin_open_seconds=silence_seconds,
        init_received=bool(init_at),
        seconds_from_write_to_init=round(init_at - wrote_at, 3) if init_at and wrote_at else None,
        message_sequence=[f"{m.get('type')}/{m.get('subtype')}" if m.get('subtype') else m.get('type')
                          for m in messages],
        init_keys=sorted(init),
        capabilities=init.get('capabilities'),
        permissionMode=init.get('permissionMode'), model=init.get('model'),
        apiKeySource=init.get('apiKeySource'),
        claude_code_version=init.get('claude_code_version'),
        init_name_list_lengths={k: len(init.get(k) or []) for k in
                                ('tools', 'mcp_servers', 'plugins', 'slash_commands', 'skills', 'agents')},
        replay_acknowledged=bool(replay),
        replay_matches_sent=bool(replay) and replay['message'] == json.loads(message)['message'],
        authentication_failure={k: failure.get(k) for k in ('error', 'is_api_error_message')},
        result_keys=sorted(final), is_error=final.get('is_error'),
        terminal_reason=final.get('terminal_reason'),
        usage_at_turn_end=final.get('usage'), total_cost_usd=final.get('total_cost_usd'),
        permission_denials=final.get('permission_denials'))


def drift(expected, actual, fields):
    return [dict(field=f, expected=expected.get(f), actual=actual.get(f))
            for f in fields if expected.get(f) != actual.get(f)]


def main():
    global EXECUTABLE
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--executable', type=Path, default=shutil.which('claude'))
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--adapters', type=Path, default=Path('adapters/claude'))
    parser.add_argument('--update-baseline', action='store_true',
                        help='write the surface and stream identities instead of comparing')
    args = parser.parse_args()
    assert args.executable, 'no claude executable selected'
    EXECUTABLE = Path(args.executable)
    args.out.mkdir(parents=True, exist_ok=True)
    private = args.out / 'private'
    private.mkdir(mode=0o700, exist_ok=True)

    real_home = Path.home()
    config = [real_home / p for p in USER_CONFIG]
    before = {p.name: sha(p.read_bytes()) for p in config if p.exists()}

    resolved = EXECUTABLE.resolve()
    # `--version` prints `2.1.278 (Claude Code)`; the adapter directory is the
    # version alone, parsed the same way the Rust adapter parses it.
    version_output = run([str(EXECUTABLE), '--version'], env_for(real_home)).stdout.strip()
    version = version_output.split()[0] if version_output.split() else ''
    measured_surface = surface(EXECUTABLE, real_home)

    # The stream probe, three ways: the product's own defaults with nothing
    # configured and no flags; the same with the mode PIO requests; and a run
    # that holds stdin open to see whether init waits for input.
    defaults = launch(private, 'product-defaults')
    requested = launch(private, 'requested-mode', ('--permission-mode', 'acceptEdits'))
    timing = launch(private, 'init-timing', silence_seconds=20)
    # Attachment, measured three ways and at zero tokens, because R3b showed
    # PIO was not attached at all: it passed `--permission-prompts host`,
    # named no prompt tool and never sent the handshake, so the CLI denied
    # anything that would prompt and PIO never saw the request.
    tool_only = launch(private, 'prompt-tool', ('--permission-prompt-tool', 'stdio'),
                       write=False)
    attached = launch(private, 'attached',
                      ('--permission-mode', 'acceptEdits',
                       '--permission-prompts', 'host',
                       '--permission-prompt-tool', 'stdio'),
                      write=False, control_first=INITIALIZE)
    # The control: the same handshake with no prompt tool named. If this one
    # is answered too, the flag is not what makes the CLI listen.
    unattached = launch(private, 'no-prompt-tool',
                        ('--permission-mode', 'acceptEdits',
                         '--permission-prompts', 'host'),
                        write=False, control_first=INITIALIZE)

    version_dir = args.adapters / version
    surface_path = version_dir / 'surface-identity.json'
    stream_path = version_dir / 'stream-identity.json'
    stream_identity = dict(
        format='pio-claude-stream-identity/1', version=version,
        init_keys=defaults['init_keys'], capabilities=defaults['capabilities'],
        message_sequence=defaults['message_sequence'],
        result_keys=defaults['result_keys'],
        product_default_permission_mode=defaults['permissionMode'],
        product_default_model=defaults['model'],
        init_waits_for_stdin=timing['messages_before_any_stdin_write'] == 0,
        # How PIO attaches as the host that answers permission prompts.
        permission_prompt_tool_accepted=(
            not tool_only['stderr_mentions_unknown_option']),
        initialize_answered=attached['initialize_answered'],
        initialize_response_subtype=attached['initialize_response_subtype'],
        initialize_response_keys=attached['initialize_response_keys'])

    if args.update_baseline:
        version_dir.mkdir(parents=True, exist_ok=True)
        surface_path.write_text(json.dumps(
            dict(format='pio-claude-surface-identity/1', version=version, **measured_surface),
            indent=2, sort_keys=True) + '\n')
        stream_path.write_text(json.dumps(stream_identity, indent=2, sort_keys=True) + '\n')
        findings = []
    else:
        findings = []
        if not surface_path.exists() or not stream_path.exists():
            findings.append(dict(reason='unsupported_version', version=version,
                                 expected_dir=str(version_dir)))
        else:
            expected_surface = json.loads(surface_path.read_text())
            if expected_surface['surface_listing_sha256'] != measured_surface['surface_listing_sha256']:
                findings += [dict(reason='surface_drift', command=name,
                                  expected=expected_surface['commands'].get(name),
                                  actual=measured_surface['commands'].get(name))
                             for name in sorted(set(expected_surface['commands']) |
                                                set(measured_surface['commands']))
                             if expected_surface['commands'].get(name) !=
                             measured_surface['commands'].get(name)]
            expected_stream = json.loads(stream_path.read_text())
            findings += [dict(reason='stream_drift', **d) for d in drift(
                expected_stream, stream_identity,
                ('init_keys', 'capabilities', 'message_sequence', 'result_keys',
                 'product_default_permission_mode', 'product_default_model',
                 'init_waits_for_stdin', 'permission_prompt_tool_accepted',
                 'initialize_answered', 'initialize_response_subtype',
                 'initialize_response_keys'))]

    after = {p.name: sha(p.read_bytes()) for p in config if p.exists()}
    summary = dict(
        format='pio-claude-requalification/2', platform=platform.platform(),
        model_calls=0, turn_completed=False, credentials_read=False,
        environment_inherited=False, env_allowlist=list(ENV_ALLOWLIST),
        executable=dict(selected=str(EXECUTABLE), resolved=str(resolved), version=version,
                        binary_sha256=sha(resolved.read_bytes()),
                        version_output=version_output,
                        resolved_path_carries_version=resolved.parent.name == version),
        cli_surface=measured_surface, stream_identity=stream_identity,
        configured_route=auth_route(EXECUTABLE, real_home),
        # Differs from the line above in exactly one thing: the configuration
        # the harness can see. Both carry USER, so a refusal here cannot be an
        # artefact of the environment rather than of the missing credential.
        missing_route_control=auth_route(EXECUTABLE, private / 'no-credentials',
                                         private / 'no-credentials'),
        stream=dict(product_defaults=defaults, requested_mode=requested, init_timing=timing,
                    prompt_tool_only=tool_only, attached=attached, unattached=unattached),
        findings=findings, qualified=not findings,
        user_configuration_unchanged=before == after,
        user_configuration_files=sorted(before))
    (args.out / 'summary.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n')
    print(json.dumps(summary, indent=2, sort_keys=True))
    if before != after:
        sys.exit(4)
    sys.exit(3 if findings else 0)


if __name__ == '__main__':
    main()
