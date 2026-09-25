#!/usr/bin/env python3
"""The three release harnesses behind one shape, for the M4 proofs.

`approval_desk.py` and `transcript_blocks.py` were written against
`serve-opencode` because that is the permission path measured most recently.
The other two are release scope, and what each *does not* do is as much the
point as what it does:

- **Codex** has no answer timeout and no default deny at all. Its action
  waits until a caller answers it. So the approval the screen shows carries
  **no countdown**, and the lapse the other two prove does not exist here —
  which the walk has to state rather than imply by leaving a field empty.
- **Claude Code** offers a rule update — a `permission_suggestion` — with
  every request. Acting on one widens a permission beyond the request.
  PIO's answer is built by `permission_decision`, which cannot encode one,
  so the walk shows suggestions as offered and **never** as something PIO
  will send.
- **OpenCode** offers an option list with kinds, including an always-allow
  that is offered on every request and never taken.

Each matrix already drives its own service over the public Unix API, and
their case classes are close but not identical: Codex's `inspect` returns the
whole envelope, it has no `respond`, and its decision vocabulary is
`accept`/`decline` rather than `allow`/`deny`. This module is the thin
adapter over those differences — deliberately thin, because a thick one would
hide exactly the per-harness behaviour the proofs exist to show.
"""
import contextlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

ROOT = Path(__file__).resolve().parents[1]


class Harness:
    """One release harness, and what the proofs may expect of it."""

    def __init__(self, name, serve, module, build, ask, decline, allow, deny,
                 lapses, offers_options, suggests, block_kind, audits, work):
        self.name = name
        self.serve = serve
        self.module = module
        self._build = build
        # Whole scenario kwargs, not just the request: Codex asks through
        # `approval`, the other two through `permission_request`.
        self.ask = ask
        self.decline = decline
        self.allow = allow
        self.deny = deny
        # Whether PIO ever decides on the caller's behalf for this harness.
        self.lapses = lapses
        # Whether the harness offers an option list PIO chooses from by kind.
        self.offers_options = offers_options
        # Whether the harness offers a rule update with every request.
        self.suggests = suggests
        self.block_kind = block_kind
        # **Codex emits no `tool_uses` record at all.** No audit reaches
        # the exit event, so a Codex run's placements are never
        # classified — which the screen has to say, rather than showing
        # an empty audit as if nothing had happened.
        self.audits = audits
        # A scenario that produces tool-use blocks beside the approval.
        self.work = work

    @contextlib.contextmanager
    def service(self, out, label, **scenario):
        """One service, released whether or not the pass that used it passed.

        The matrices wrap every case in a `finally` and release it there.
        These proofs did not: `cleanup()` sat on the success path, so a
        failing assertion left the store, the daemon and the harness behind
        until the process exited — and if the process was killed instead, for
        ever. Four stores survived on `/tmp` that way.

        A cleanup that itself fails is not swallowed: a surviving process is
        a real failure, and Python shows it chained to whatever raised first.
        """
        case = self._build(self.module, out, label, scenario)
        try:
            yield Service(self, case)
        finally:
            # The Codex case calls it `close`; the other two `cleanup`.
            (getattr(case, 'cleanup', None) or case.close)()

    def decode(self, record):
        """One spooled record into the blocks a screen would draw from it.

        Never a placement and never a decider: neither is knowable here, and
        inventing one from a path that looks local is the mistake G3 exists
        to prevent.
        """
        if self.block_kind == 'session_update':
            update = record.get('update', {})
            kind = update.get('sessionUpdate')
            if kind == 'agent_message_chunk':
                return [('text', update.get('content', {}).get('text', ''))]
            if kind in ('tool_call', 'tool_call_update'):
                return [('tool', dict(tool_use_id=update.get('toolCallId'),
                                      kind=update.get('kind')))]
            return []
        if self.block_kind == 'assistant':
            if record.get('type') != 'assistant':
                return []
            out = []
            for block in record.get('message', {}).get('content', []):
                if block.get('type') == 'text':
                    out.append(('text', block.get('text', '')))
                elif block.get('type') == 'tool_use':
                    out.append(('tool', dict(tool_use_id=block.get('id'),
                                             kind=block.get('name'))))
            return out
        if self.block_kind == 'item':
            method, params = record.get('method'), record.get('params', {})
            if method == 'item/agentMessage/delta':
                return [('text', params.get('delta', ''))]
            if method == 'item/completed':
                item = params.get('item', {})
                return [('tool', dict(tool_use_id=item.get('id'),
                                      kind=item.get('type')))]
            return []
        raise SystemExit(f'no decoder for {self.block_kind!r}')

    def __repr__(self):
        return f'<harness {self.name}>'


class Service:
    """One running service, with the differences between the three ironed out."""

    def __init__(self, harness, case):
        self.harness = harness
        self.case = case
        self.store = case.store
        self.out = case.out

    def start(self):
        self.case.start()

    def client(self):
        return self.case.client()

    def submit(self, identity='work', delivery_timeout=120):
        if self.harness.name == 'codex':
            return self.case.submit(identity=identity, delivery=delivery_timeout)[0]
        return self.case.submit(identity=identity, delivery_timeout=delivery_timeout)

    def inspect(self, identity='work'):
        answer = self.case.inspect(identity)
        # Codex's case returns the whole envelope; the other two return the
        # view. Sniffing for a `result` key got this wrong, because the view
        # has a `result` field of its own — it is the string `"absent"` until
        # a turn produces one, and the walk then indexed into a string.
        return answer['result'] if self.harness.name == 'codex' else answer

    def respond(self, action_id, decision, revision, identity='work'):
        if self.harness.name == 'codex':
            body = json.dumps({'decision': decision}).encode()
            from codex_host_matrix import digest
            return self.case.execution_command(
                'execution.respond_action', identity,
                dict(action_id=action_id,
                     response=dict(digest=digest(body), media_type='application/json')),
                f'{identity}.answer', body, 'application/json')
        return self.case.respond(action_id, decision, revision, identity=identity)

    def finish(self):
        if hasattr(self.case, 'finish'):
            self.case.finish()


def _opencode(module, out, label, scenario):
    return module.ServiceCase(out, label, model=module.REQUESTED, scenario=scenario)


def _claude(module, out, label, scenario):
    return module.ServiceCase(out, label, scenario=scenario)


def _codex(module, out, label, scenario):
    return module.Case(out, label, scenario=scenario)


def load(name):
    """Import the matrix a harness is driven from, and build its Harness."""
    if name == 'opencode':
        import opencode_host_matrix as module
        return Harness(
            name='opencode', serve='serve-opencode', module=module, build=_opencode,
            # Announced as an `execute` call with a command line and no path,
            # which the resolver will not place.
            ask={'permission_request': {'title': 'run a command', 'kind': 'execute',
                                        'input': {'command': 'git tag pio-approval-marker'}}},
            decline={'permission_request': {'title': 'read a file', 'kind': 'read',
                                            'input': {'file_path': '/etc/hosts'}}},
            allow='allow', deny='deny', lapses=True, offers_options=True,
            suggests=False, block_kind='session_update', audits=True,
            work={'tool_calls': [{'title': 'run a command', 'kind': 'execute',
                                  'input': {'command': 'wc -l README.md'}},
                                 {'title': 'read a file', 'kind': 'read',
                                  'input': {'file_path': 'README.md'}}],
                  'message_chunks': 6, 'delay_ms': 400})
    if name == 'claude':
        import claude_host_matrix as module
        return Harness(
            name='claude', serve='serve-claude', module=module, build=_claude,
            ask={'permission_request': {'tool_name': 'Bash',
                                        'input': {'command': 'git tag pio-approval-marker'}}},
            decline={'permission_request': {'tool_name': 'Read',
                                            'input': {'file_path': '/etc/hosts'}}},
            allow='allow', deny='deny', lapses=True, offers_options=False,
            suggests=True, block_kind='assistant', audits=True,
            work={'tool_uses': [{'name': 'Bash', 'input': {'command': 'wc -l README.md'}},
                                {'name': 'Read', 'input': {'file_path': 'README.md'}}],
                  'delay_ms': 400})
    if name == 'codex':
        import codex_host_matrix as module
        return Harness(
            name='codex', serve='serve-codex', module=module, build=_codex,
            ask={'approval': 'command', 'delay_ms': 100},
            # This harness has no decline path of PIO's own in the matrix's
            # scenarios: it asks, and PIO surfaces. Stated rather than faked.
            # Since 2026-09-25 (owner decision) an action nobody answers
            # lapses to one decline of PIO's, as on the other two.
            decline=None,
            allow='accept', deny='decline', lapses=True, offers_options=False,
            suggests=False, block_kind='item', audits=False, work={'delay_ms': 400})
    raise SystemExit(f'unknown harness {name!r}')


NAMES = ('opencode', 'claude', 'codex')
