#!/usr/bin/env python3
"""G3 — transcript blocks as they happen, and the audit that lands at the end.

Screen 5 shows the transcript while the run is still working, and screen 1
previews its last few blocks. Neither can wait for the end of the turn, which
is the only moment PIO knows where a tool use landed. So G3 is two different
things that the first draft of the map ran together:

- **Live**, the screen reads `execution.output.read` from its own offset and
  decodes what the host spooled — one record per `session/update` for
  OpenCode. A tool use is visible here, but **where it landed is not**: the
  screen must show `not yet classified` and `unknown` rather than guess from
  a path that looks local. That is the *absence* of a record, so nothing has
  to travel for it.
- **At the end**, `execution.exit.observed` carries the audit under
  `pio.combraton.dev/tool-uses`: placement and decider per tool use, plus the
  harness's own status for each call so a reader can see the two disagree.

This proves both through `serve-opencode` with the labeled ACP fake behind
it — a real host, a real spool, the real projection, the public Unix API in
front — and proves that the ids seen live and the ids in the audit are the
same set, which is what lets the screen fill in the blanks in place.

Mutants:

- `--mutant guess-inside` lets the live reader call a tool use `inside`
  because its command mentions the workspace. The audit says
  `not_classifiable`, and the run fails. This is the mutant that matters:
  guessing is exactly what the screen must not do.
- `--mutant whole-spool` re-reads the transcript from offset 0 on every tick
  instead of from `next_offset`. It draws the same screen and fails the
  assertion that a live reader never re-reads what it has already seen.
- `--baseline-as-mutant` runs the join against the binary built at
  `a22c98c`, where `execution.exit.observed` carries no audit at all, so
  nothing can be filled in.
"""
import argparse
import base64
import contextlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import opencode_host_matrix as matrix
from opencode_host_matrix import poll, release_live_cases
from approval_desk import (BASELINE_COMMIT, NS, build_baseline, check_event,
                           event_schema, service, with_binary)

ROOT = Path(__file__).resolve().parents[1]
AUDIT = NS + 'tool-uses'
# Two calls the live reader can see and cannot place. One is a command line
# the resolver will not place at all; the other names a file in the
# workspace. Neither is classifiable from the announcement, which is the
# point: on the screen they are the same until the audit says otherwise.
CALLS = [{'title': 'run a command', 'kind': 'execute',
          'input': {'command': 'wc -l README.md'}},
         {'title': 'read a file', 'kind': 'read',
          'input': {'file_path': 'README.md'}}]
# The live window. A turn from this fake is over in about a second, which is
# not long enough to watch anything — the first draft of this proof read the
# transcript forty times and got zero bytes every time, then everything at
# once after the run had exited, and still passed. A pending approval is a
# real pause with real blocks already spooled behind it, so "as they happen"
# becomes something the run measures instead of something it hopes for.
ASKS = {'title': 'run a command', 'kind': 'execute',
        'input': {'command': 'git tag pio-transcript-marker'}}


class Transcript:
    """The screen's model of one run's transcript, built only from reads.

    Keeps its own offset, the way a screen does, so "as they happen" is a
    property of the client and not of the service.
    """

    def __init__(self, case, identity, guess=False):
        self.case = case
        self.identity = identity
        self.guess = guess
        self.offset = 0
        self.tail = b''
        self.blocks = []
        self.reads = []          # (offset, bytes) per read, for the mutant
        self.tools = {}          # tool_use_id -> what the screen can say now

    def pull(self, whole_spool=False):
        """One tick. Reads from where it left off, or from the start."""
        with self.case.client() as client:
            answer = client.query('execution.output.read',
                                  {'execution': self.identity,
                                   'offset': 0 if whole_spool else self.offset,
                                   'max_bytes': 65536})
        result = answer.get('result')
        assert result is not None, f'execution.output.read refused: {answer}'
        data = base64.b64decode(result['data_base64'])
        self.reads.append((result['offset'], len(data)))
        assert result['coverage'] == 'complete', result
        if whole_spool:
            self.tail, self.blocks, self.tools = b'', [], {}
        self.offset = result['next_offset']
        self.tail += data
        *lines, self.tail = self.tail.split(b'\n')
        for line in lines:
            if line.strip():
                self.absorb(json.loads(line), self.guess)
        return len(data)

    def absorb(self, record, guess=False):
        """One spooled record becomes one block on the screen."""
        update = record.get('update', {})
        kind = update.get('sessionUpdate')
        if kind == 'agent_message_chunk':
            self.blocks.append(('text', update['content'].get('text', '')))
            return
        if kind not in ('tool_call', 'tool_call_update'):
            return
        identity = update.get('toolCallId')
        seen = self.tools.setdefault(identity, {
            'tool_use_id': identity, 'kind': update.get('kind'),
            # Live, PIO has not classified anything. The screen says so.
            'placement': 'not yet classified', 'decided_by': 'unknown'})
        if kind == 'tool_call':
            self.blocks.append(('tool', identity))
        if update.get('kind'):
            seen['kind'] = update['kind']
        if guess:
            # The mutant. A path that looks local is not a placement.
            target = (update.get('rawInput') or {})
            text = target.get('command') or target.get('file_path') or ''
            if text and not text.startswith('/'):
                seen['placement'] = 'inside'

    def fill(self, audit):
        """The end-of-turn audit lands, and the blanks are filled in place."""
        for row in audit['audit']['tool_uses']:
            seen = self.tools.get(row['tool_use_id'])
            if seen is None:
                continue
            # A screen is allowed to say it does not know. Anything else
            # it says has to survive the audit — otherwise it told the user
            # something PIO never determined.
            assert seen['placement'] in ('not yet classified', row['placement']), \
                (f"the screen said {seen['placement']!r} while the run was "
                 f"working; the audit says {row['placement']!r}", seen, row)
            seen['placement'] = row['placement']
            seen['decided_by'] = row['decided_by'] or 'nobody was asked'
            seen['outcome'] = row['outcome']


def audit_of(case, identity):
    """The audit as a caller gets it: off the stream, nowhere else."""
    schema = event_schema()
    with case.client() as client:
        answer = client.query('core.events.read',
                              {'limit': 1000, 'from': 'start',
                               'kinds': ['execution.execution']})
    result = answer.get('result')
    assert result is not None, answer
    found = None
    for item in result['items']:
        event = item['event']
        check_event(event, schema)
        if event['subject']['id'] == identity \
                and event['type'] == 'execution.exit.observed':
            found = event
    return found


def run(out, label, mutant=None):
    with service(out, label, permission_request=ASKS, tool_calls=CALLS,
                 message_chunks=6, delay_ms=400) as case:
        case.start()
        case.submit(identity='run-1', delivery_timeout=300)
        screen = Transcript(case, 'run-1', guess=mutant == 'guess-inside')

        # --- Live. Read from the moment the run starts, not from the pause:
        #     the harness's opening chunk and the tool call it then asks about
        #     are spooled several hundred milliseconds apart, so a screen that
        #     accumulates sees two batches and a screen that waits sees one.
        live_reads, live_bytes, still = 0, 0, None
        for _ in range(200):
            got = screen.pull(whole_spool=mutant == 'whole-spool')
            if got:
                live_reads += 1
                live_bytes += got
            still = case.inspect('run-1')
            assert still['runtime'] != 'exited', 'the run finished before it could be watched'
            if still['runtime'] == 'requires_action' and live_reads >= 2 and screen.tools:
                break
        assert still['runtime'] == 'requires_action', still
        assert still['delivery'] == 'acknowledged', still
        # The claim, measured: bytes reached the screen **while the run was
        # still working**, in more than one read, and the transcript already has
        # blocks in it.
        assert live_reads >= 2, (live_reads, screen.reads)
        assert screen.blocks, screen.reads
        assert screen.tools, 'the pending tool call never reached the screen'
        live_ids = set(screen.tools)
        if mutant != 'guess-inside':
            assert all(t['placement'] == 'not yet classified'
                       for t in screen.tools.values()), screen.tools
            assert all(t['decided_by'] == 'unknown'
                       for t in screen.tools.values()), screen.tools
        # And nothing on the stream places them either, so the screen is not
        # merely failing to look: there is nothing yet to look at.
        assert audit_of(case, 'run-1') is None, 'the run has not exited'

        # --- The caller answers, the turn finishes, the rest of the blocks land.
        case.respond(still['runtime_detail']['action_id'], 'allow', still['revision'],
                     identity='run-1')
        poll(lambda: case.inspect('run-1'), lambda v: v['runtime'] == 'exited', seconds=200)
        # Bounded: a reader that re-reads the whole spool always gets bytes
        # back, so "drain until empty" is only a terminating loop for a reader
        # that advances its offset.
        for _ in range(60):
            if not screen.pull(whole_spool=mutant == 'whole-spool'):
                break
        assert len(screen.tools) == len(CALLS) + 1, screen.tools
        assert live_ids < set(screen.tools), (live_ids, set(screen.tools))
        # A live reader never re-reads what it has already seen: each read starts
        # exactly where the last one ended.
        ends = [offset + length for offset, length in screen.reads]
        starts = [offset for offset, _ in screen.reads]
        first_repeat = next((n for n, (start, end) in
                             enumerate(zip(starts[1:], ends[:-1]), 1) if start != end),
                            None)
        assert first_repeat is None, (
            f'read {first_repeat} started at {starts[first_repeat]} but the one '
            f'before it ended at {ends[first_repeat - 1]}: this reader re-reads '
            'transcript it has already shown, so "as they happen" is a redraw')

        # --- The audit lands on the event that already marks the end of the turn.
        exit_event = audit_of(case, 'run-1')
        assert exit_event is not None, 'no exit event on the stream'
        assert AUDIT in exit_event['payload'], (
            'the turn ended and the audit reached the stream nowhere: '
            f"payload={sorted(exit_event['payload'])}")
        audit = exit_event['payload'][AUDIT]
        screen.fill(audit)

        placed = {t['tool_use_id']: t for t in screen.tools.values()}
        assert set(placed) == {row['tool_use_id'] for row in audit['audit']['tool_uses']}, \
            (sorted(placed), audit['audit']['tool_uses'])
        for row in audit['audit']['tool_uses']:
            assert placed[row['tool_use_id']]['placement'] == row['placement'], placed
            assert row['placement'] != 'not yet classified', row
        # Three deciders, told apart: the caller answered one, and nobody was
        # asked about the other two. A screen that showed one word for both
        # would be claiming PIO saw something it did not.
        deciders = {t['tool_use_id']: t['decided_by'] for t in screen.tools.values()}
        assert sorted(deciders.values()) == ['caller', 'nobody was asked',
                                             'nobody was asked'], deciders
        placements = sorted(t['placement'] for t in screen.tools.values())
        assert placements == ['inside_fixture', 'not_classifiable', 'not_classifiable'], \
            placements
        # The harness's own status travels beside the audit, so a reader can see
        # the two disagree rather than being handed one of them.
        assert audit['harness_status'], audit
        assert {s['tool_use_id'] for s in audit['harness_status']} == set(placed), audit

        record = dict(blocks=len(screen.blocks), reads=len(screen.reads),
                      reads_while_the_run_was_working=live_reads,
                      bytes_while_the_run_was_working=live_bytes,
                      live_ids=sorted(live_ids),
                      after_the_audit={t['tool_use_id']: dict(
                          placement=t['placement'], decided_by=t['decided_by'],
                          outcome=t.get('outcome')) for t in screen.tools.values()},
                      containment=audit['audit']['containment'])
        case.finish()
        return record


def baseline_mutant(out, commit):
    binary = build_baseline(commit)
    try:
        with_binary(binary, lambda: run(out, 'mutant'))
    except AssertionError as death:
        release_live_cases()
        return str(death)[:300]
    raise SystemExit(f'MUTANT SURVIVED: the join held against {commit}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'target/transcript-blocks')
    parser.add_argument('--mutant', choices=['guess-inside', 'whole-spool'])
    parser.add_argument('--baseline-as-mutant', action='store_true')
    parser.add_argument('--baseline-commit', default=BASELINE_COMMIT)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    try:
        if args.baseline_as_mutant:
            print(json.dumps(dict(mutant=args.baseline_commit,
                                  died=baseline_mutant(args.out, args.baseline_commit)),
                             indent=2))
            print('the mutant died')
            return
        record = run(args.out, 'blocks', args.mutant)
        (args.out / 'transcript-blocks.json').write_text(
            json.dumps(record, indent=2, sort_keys=True) + '\n')
        print(json.dumps(record, indent=2, sort_keys=True))
        print('transcript blocks: pass')
    finally:
        release_live_cases()


if __name__ == '__main__':
    main()
