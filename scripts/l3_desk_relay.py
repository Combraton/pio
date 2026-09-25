#!/usr/bin/env python3
"""L3's desk relay: the owner's advance decisions, and nothing else.

Owner decisions of 2026-09-25 (after review 53), item 3. By itself the relay
answers only:

(a) `L3.alpha`'s command approval for exactly `sleep 30 && wc -l alpha.md`,
    and `L3.beta`'s for exactly `sleep 5 && wc -l beta.md`, **inside the
    run's fixture** and asking for no network access: allow, which the runner
    sends as Codex's `accept`, a single use. The owner's rule is a request
    inside the fixture that the plan predicted, so the placement PIO
    classified (`inside_fixture`, from the command's working directory) is
    required as well as the exact command (review of L3, F6/CH-6);
(b) if Codex asks despite the pre-allowance, `L3`'s own `mcp_tool_call`
    approvals for `pio-lead` `start_run` and `read_run`: allow, a single
    use, with words that say the pre-allowance did not hold (the runner's
    pre-allowance row fails on the same request).

Anything else is printed as `DESK FOR THE OWNER` and left for the owner to
answer live within its deadline; unanswered, the host's single decline
lands when it lapses. The relay keeps watching after that, so a request it
may answer is never held up behind one it may not.

"Exactly" means the command the owner named, or that command as Codex names
a command it asks about: M2's R5 measured 0.155.1 sending
`/bin/zsh -lc 'python3 -m unittest -q'`, the owner's login shell wrapping it.
Nothing else matches: no other shell, no other quoting, no extra words.

Its command line never names the live tree: the tree is read from the
runner's log in `--dir`, and polled with `os.listdir` and `open()`, with no
subprocess. The cleanup kills every process group whose command line names
the tree (L1).

`--selftest` runs the decision against requests it must answer and
requests it must leave to the owner, then runs the relay itself over a desk
holding an empty pending file beside a valid one: it must answer the valid
one and leave the other to be read again.
"""
import argparse
import datetime
import json
import os
import subprocess
import sys
import tempfile
import threading
import time

DECIDED = ('Owner decision, 2026-09-25 (after review 53), item {item}, quoted: "{quote}". '
           'Applied by the builder\'s relay as soon as the request arrived.')
COMMANDS = {'L3.alpha': 'sleep 30 && wc -l alpha.md', 'L3.beta': 'sleep 5 && wc -l beta.md'}
QUOTE_A = ("L3.alpha's command approval for exactly `sleep 30 && wc -l alpha.md` and "
           "L3.beta's for exactly `sleep 5 && wc -l beta.md`: accept, single use")
QUOTE_B = ("if Codex asks despite the pre-allowance, L3's own mcp_tool_call approvals for "
           'pio-lead start_run and read_run: accept, single use, and the receipt records '
           'that the pre-allowance did not hold')
# Codex's own question for a server with no template of its own
# (`build_mcp_tool_approval_fallback_message`, rust-v0.157.0).
LEAD_TOOLS = {f'Allow the pio-lead MCP server to run tool "{tool}"?'
              for tool in ('start_run', 'read_run')}


def as_codex_names_it(command):
    return (command, f"/bin/zsh -lc '{command}'")


def decided_in_advance(item):
    """The owner's words for a request decided in advance, or None."""
    approval = item.get('approval') or {}
    run = item.get('run')
    if approval.get('method') == 'item/commandExecution/requestApproval' \
            and approval.get('approval_kind') == 'command' \
            and run in COMMANDS \
            and approval.get('command') in as_codex_names_it(COMMANDS[run]) \
            and (approval.get('classification') or {}).get('placement') == 'inside_fixture' \
            and approval.get('network_approval') is False:
        return DECIDED.format(item='3(a)', quote=QUOTE_A)
    if run == 'L3' and approval.get('method') == 'mcpServer/elicitation/request' \
            and approval.get('approval_kind') == 'mcp_tool_call' \
            and approval.get('server') == 'pio-lead' \
            and approval.get('message') in LEAD_TOOLS:
        return DECIDED.format(item='3(b)', quote=QUOTE_B) + \
            ' The pre-allowance did not hold for this call.'
    return None


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def stamp():
    return datetime.datetime.now(datetime.timezone.utc).strftime('%H:%M:%SZ')


def relay(here):
    pid = int(open(os.path.join(here, 'runner.pid')).read())
    done_path = os.path.join(here, 'answered.txt')
    seen = set(open(done_path).read().split()) if os.path.exists(done_path) else set()
    while True:
        if not alive(pid):
            print('RUNNER EXITED', flush=True)
            return
        root = None
        for line in open(os.path.join(here, 'runner.log'), errors='replace'):
            if line.startswith('ROOT '):
                root = line[5:].strip()
                break
        desk = os.path.join(root, 'desk') if root else None
        if desk and os.path.isdir(desk):
            names = os.listdir(desk)
            for name in sorted(names):
                if not (name.startswith('pending-') and name.endswith('.json')):
                    continue
                action = name[len('pending-'):-len('.json')]
                if action in seen or f'answer-{action}.json' in names:
                    continue
                try:
                    with open(os.path.join(desk, name)) as handle:
                        item = json.load(handle)
                except (OSError, ValueError):
                    # Not whole yet (the runner now writes it whole, but a
                    # relay must not die on one): read it again next pass,
                    # and never let it hold up another request (review of
                    # L3, F8).
                    continue
                seen.add(action)
                with open(done_path, 'a') as out:
                    out.write(action + '\n')
                words = decided_in_advance(item)
                if words is None:
                    print(f'DESK FOR THE OWNER {action} at {stamp()}', flush=True)
                    print(json.dumps(item, indent=2), flush=True)
                    continue
                answer = dict(decision='allow', decided_by='owner', words=words)
                target = os.path.join(desk, f'answer-{action}.json')
                with open(target + '.tmp', 'w') as out:
                    json.dump(answer, out)
                os.replace(target + '.tmp', target)
                print(f'ANSWERED IN ADVANCE {action} allow at {stamp()}', flush=True)
        time.sleep(0.5)


def selftest():
    def item(run, **approval):
        return dict(run=run, approval=approval)
    command = 'item/commandExecution/requestApproval'
    mcp = 'mcpServer/elicitation/request'
    ask = 'Allow the pio-lead MCP server to run tool "{}"?'
    inside = dict(classification=dict(subject='cwd', placement='inside_fixture',
                                      target_label='<fixture>/'), network_approval=False)
    answered = [
        item('L3.alpha', method=command, approval_kind='command',
             command='sleep 30 && wc -l alpha.md', **inside),
        item('L3.alpha', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 30 && wc -l alpha.md'", **inside),
        item('L3.beta', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'", **inside),
        item('L3', method=mcp, approval_kind='mcp_tool_call', server='pio-lead',
             message=ask.format('start_run')),
        item('L3', method=mcp, approval_kind='mcp_tool_call', server='pio-lead',
             message=ask.format('read_run')),
    ]
    outside = dict(inside, classification=dict(subject='cwd', placement='outside_fixture',
                                               target_label='<outside>'))
    unplaced = dict(inside, classification=dict(subject='cwd', placement='not_classifiable',
                                                 target_label=None))
    for_the_owner = [
        # The exact command, run outside the fixture, or where PIO could not
        # place it, or with no placement at all, or asking for the network.
        item('L3.alpha', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 30 && wc -l alpha.md'", **outside),
        item('L3.beta', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'", **unplaced),
        item('L3.beta', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'"),
        item('L3.beta', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'",
             **dict(inside, network_approval=True)),
        # Another run's command, a longer command, another shell, a
        # terminal write, a file change.
        item('L3.alpha', method=command, approval_kind='command',
             command='sleep 5 && wc -l beta.md', **inside),
        item('L3.alpha', method=command, approval_kind='command',
             command="/bin/zsh -lc 'sleep 30 && wc -l alpha.md; rm -f alpha.md'", **inside),
        item('L3.alpha', method=command, approval_kind='command',
             command="/bin/bash -lc 'sleep 30 && wc -l alpha.md'", **inside),
        item('L3.alpha', method=command, approval_kind='writeStdin',
             command='sleep 30 && wc -l alpha.md', **inside),
        item('L3.beta', method='item/fileChange/requestApproval', approval_kind=None),
        item('L3', method=command, approval_kind='command',
             command='sleep 30 && wc -l alpha.md', **inside),
        # Another tool, another server, a child asking about the lead's tool.
        item('L3', method=mcp, approval_kind='mcp_tool_call', server='pio-lead',
             message=ask.format('stop_run')),
        item('L3', method=mcp, approval_kind='mcp_tool_call', server='computer-use',
             message='Allow the computer-use MCP server to run tool "click"?'),
        item('L3.alpha', method=mcp, approval_kind='mcp_tool_call', server='pio-lead',
             message=ask.format('start_run')),
        item('L3', method=mcp, approval_kind=None, server='pio-lead',
             message=ask.format('start_run')),
    ]
    for case in answered:
        assert decided_in_advance(case), case
    for case in for_the_owner:
        assert decided_in_advance(case) is None, case
    assert 'did not hold' in decided_in_advance(answered[3])
    print(f'l3 desk relay: {len(answered)} answered in advance, '
          f'{len(for_the_owner)} left for the owner')
    half_written()


def half_written():
    """The relay over a desk with an empty pending file beside a valid one:
    it answers the valid one, does not die, and leaves the empty one unseen
    so it is read again (review of L3, F8)."""
    with tempfile.TemporaryDirectory(prefix='l3-relay-selftest-') as here:
        tree = os.path.join(here, 'tree')
        desk = os.path.join(tree, 'desk')
        os.makedirs(desk)
        runner = subprocess.Popen(['sleep', '3'])
        with open(os.path.join(here, 'runner.pid'), 'w') as out:
            out.write(str(runner.pid))
        with open(os.path.join(here, 'runner.log'), 'w') as out:
            out.write(f'ROOT {tree}\n')
        open(os.path.join(desk, 'pending-L3.alpha.action-1.json'), 'w').close()
        beta = dict(run='L3.beta', approval=dict(
            method='item/commandExecution/requestApproval', approval_kind='command',
            command="/bin/zsh -lc 'sleep 5 && wc -l beta.md'", network_approval=False,
            classification=dict(subject='cwd', placement='inside_fixture',
                                target_label='<fixture>/')))
        with open(os.path.join(desk, 'pending-L3.beta.action-1.json'), 'w') as out:
            json.dump(beta, out)
        watching = threading.Thread(target=relay, args=(here,), daemon=True)
        watching.start()
        runner.wait()
        watching.join(timeout=10)
        assert not watching.is_alive(), 'the relay did not see the runner exit'
        answered = sorted(n for n in os.listdir(desk) if n.startswith('answer-'))
        assert answered == ['answer-L3.beta.action-1.json'], answered
        with open(os.path.join(here, 'answered.txt')) as handle:
            assert handle.read().split() == ['L3.beta.action-1']
    print('l3 desk relay: an empty pending file was left to be read again, and the valid '
          'one beside it was answered')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--dir', help="where the runner's pid and log are")
    parser.add_argument('--selftest', action='store_true')
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if not args.dir:
        raise SystemExit('--dir is required')
    relay(args.dir)


if __name__ == '__main__':
    main()
