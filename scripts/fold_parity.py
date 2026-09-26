#!/usr/bin/env python3
"""The Rust folds against the Python folds they were ported from.

`pio_client` (crates/pio-client) carries the three client-side folds the M4
screen draws from: the **board** (G1, G6), the **approval walk** and every
decision (G2), and the **transcript blocks** with the audit filled in (G3).
Each is a port of the Python fold that proved it: `board_fold.Board.absorb`
and `board_view`, `approval_desk.walk` and `decisions`, and
`transcript_blocks.Transcript.feed` and `fill`. A port is only as good as the
check that it still agrees, so this feeds **the same recorded stream** to
both and requires identical output, field for field.

The recordings are real: `--record` drives `serve-fake` (the board, with and
without a retention gap), `serve-opencode`, `serve-claude` and `serve-codex`
behind their labeled fakes (the walk and the blocks), and writes what came
off the public socket — event pages, inspected views, output reads as they
arrived. The committed set in `crates/pio-client/tests/fixtures/` was made
that way, and `tests/folds.rs` holds the Rust side to the Python side's
answer on it (`<name>.expected.json`, written by `--write-expected`).

    fold_parity.py --out DIR                   the committed recordings
    fold_parity.py --out DIR --record          fresh recordings as well
    fold_parity.py --out DIR --mutant NAME     a Rust fold with one field broken

`--mutant` builds the Rust fold from a clean worktree of HEAD with one named
edit applied, and requires the parity check to fail **on that field**:

- `walk-drops-offer` removes `if_nobody_answers` from every walk row;
- `board-exited-running` draws an exited run as `running`;
- `board-liability-uncertain` makes unresolved usage alone `uncertain` again
  (the rule before the orchestrator's decision of 2026-09-26);
- `blocks-nobody-unknown` fills an undecided tool use as `unknown` instead of
  `nobody was asked`.
"""
import argparse
import base64
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'crates/pio-client/tests/fixtures'

MUTANTS = {
    'walk-drops-offer': ('crates/pio-client/src/walk.rs',
                         'row["arrived"] = event["sequence"].clone();',
                         'row["arrived"] = event["sequence"].clone();\n'
                         '            row.as_object_mut().map(|o| o.remove("if_nobody_answers"));',
                         'if_nobody_answers'),
    'board-exited-running': ('crates/pio-client/src/board.rs',
                             'if view["runtime"] == "exited" {\n        return "finished";',
                             'if view["runtime"] == "exited" {\n        return "running";',
                             'state'),
    'board-liability-uncertain': ('crates/pio-client/src/board.rs',
                                  'if view["delivery"] == "ambiguous" || view["runtime"] == "unknown" {',
                                  'if view["delivery"] == "ambiguous" || view["runtime"] == "unknown"'
                                  ' || view["usage"]["liability"] == "unresolved" {',
                                  'state'),
    'blocks-nobody-unknown': ('crates/pio-client/src/blocks.rs',
                              'Value::Null => json!("nobody was asked"),',
                              'Value::Null => json!("unknown"),',
                              'decided_by'),
}


# --- the Python folds, over a recording ---------------------------------------

def python_fold(recording):
    kind = recording['kind']
    if kind == 'board':
        from board_fold import Board, board_view
        board, rounds = Board(caller=None), []
        for round_ in recording['rounds']:
            moved = set()
            for page in round_['pages']:
                moved |= board.absorb(page)
            for identity in sorted(moved):
                board.draws[identity] = board.draws.get(identity, 0) + 1
                if identity in round_['views']:
                    board.drawn[identity] = round_['views'][identity]
            rounds.append(dict(moved=sorted(moved), board=board_view(board)))
        return dict(rounds=rounds)
    if kind == 'walk':
        from approval_desk import decisions, walk
        events = recording['events']
        return dict(walk=walk(events), arrival=walk(events, 'arrival'),
                    decided=decisions(events))
    if kind == 'blocks':
        import proof_harness
        from transcript_blocks import AUDIT, Transcript
        screen = Transcript(None, 'run-1', harness=proof_harness.load(recording['harness']))
        for read in recording['reads']:
            screen.feed(base64.b64decode(read))
        audited = False
        exit_event = recording.get('exit')
        if exit_event and AUDIT in exit_event['payload']:
            screen.fill(exit_event['payload'][AUDIT])
            audited = True
        return dict(screen.view(), audited=audited)
    raise SystemExit(f'unknown recording kind {kind!r}')


def rust_fold(binary, path):
    result = subprocess.run([str(binary), str(path)], capture_output=True, text=True)
    if result.returncode != 0:
        raise AssertionError(f'the Rust fold failed on {path.name}: {result.stderr}')
    return json.loads(result.stdout)


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'))


def differences(left, right, path='$'):
    """Every leaf where two JSON values differ, by path."""
    if isinstance(left, dict) and isinstance(right, dict):
        for key in sorted(set(left) | set(right)):
            if key not in left or key not in right:
                yield f'{path}.{key}'
            else:
                yield from differences(left[key], right[key], f'{path}.{key}')
    elif isinstance(left, list) and isinstance(right, list):
        for index, (a, b) in enumerate(zip(left, right)):
            yield from differences(a, b, f'{path}[{index}]')
        for index in range(min(len(left), len(right)), max(len(left), len(right))):
            yield f'{path}[{index}]'
    elif canonical(left) != canonical(right):
        yield path


def field_of(path):
    """The last named field of a path: `$.walk[0].due` -> `due`."""
    return path.rsplit('.', 1)[-1].split('[', 1)[0]


def compare(binary, paths, out):
    results = []
    for path in paths:
        recording = json.loads(path.read_text())
        python = python_fold(recording)
        rust = rust_fold(binary, path)
        differing = list(differences(python, rust))
        (out / f'{path.stem}.python.json').write_text(canonical(python) + '\n')
        (out / f'{path.stem}.rust.json').write_text(canonical(rust) + '\n')
        results.append(dict(recording=path.name, kind=recording['kind'],
                            identical=not differing, differences=len(differing),
                            fields=sorted({field_of(d) for d in differing}),
                            first_difference=differing[0] if differing else None))
    return results


def fold_binary(root=ROOT, target=None):
    env = dict(os.environ)
    if target:
        env['CARGO_TARGET_DIR'] = str(target)
    subprocess.run(['cargo', 'build', '--quiet', '--locked', '-p', 'pio-client',
                    '--example', 'fold'], cwd=root, check=True, env=env)
    return Path(target or root / 'target') / 'debug/examples/fold'


def recordings(directory):
    return sorted(p for p in directory.glob('*.json') if not p.name.endswith('.expected.json'))


# --- recording from real services ---------------------------------------------

def events_pages(client, kinds=('execution.execution',)):
    """Every page of `core.events.read` from the start, as the service sent it."""
    pages, payload = [], {'limit': 1000, 'from': 'start', 'kinds': list(kinds)}
    while True:
        answer = client.query('core.events.read', payload)
        assert 'result' in answer, answer
        pages.append(answer['result'])
        if not answer['result']['items']:
            return pages
        payload = {'limit': 1000, 'cursor': answer['result']['next_cursor'],
                   'kinds': list(kinds)}


def flat_events(pages):
    return [item['event'] for page in pages for item in page['items'] if 'event' in item]


def board_round(client, cursor=None):
    """One board refresh as the service answered it: the pages from the
    cursor (or the start) to an empty one, and the view of every subject
    they name. Returns the round and the cursor to continue from."""
    pages = []
    payload = {'limit': 1000, 'kinds': ['execution.execution']}
    payload.update({'cursor': cursor} if cursor else {'from': 'start'})
    while True:
        answer = client.query('core.events.read', payload)
        assert 'result' in answer, answer
        pages.append(answer['result'])
        cursor = answer['result']['next_cursor']
        if not answer['result']['items']:
            break
        payload = {'limit': 1000, 'kinds': ['execution.execution'], 'cursor': cursor}
    named = {item['event']['subject']['id'] for page in pages
             for item in page['items'] if 'event' in item}
    named |= {entry['subject']['id'] for page in pages for item in page['items']
              if 'gap' in item for entry in item['gap']['snapshot']['subjects']}
    views = {}
    for identity in sorted(named):
        answer = client.query('execution.inspect', {'execution': identity})
        if 'result' in answer:
            views[identity] = answer['result']
    return dict(pages=pages, views=views), cursor


def record_board(out, into, retain=None):
    """Six runs, then a seventh while it is still working: the board of G1."""
    from board_fold import CREDENTIAL, Caller, start_service
    from public_api import submit
    name = 'board-retention' if retain else 'board'
    root = Path(tempfile.mkdtemp(prefix='pio-fp-', dir='/tmp')).resolve()
    os.chmod(root, 0o700)
    daemon, socket_path, files = start_service(
        root, out, events={'retain_last': retain} if retain else None, name=f'daemon-{name}')
    try:
        owner = Caller(socket_path, CREDENTIAL)
        for index in range(1, 7):
            assert 'result' in owner.call(submit(1200, identity=f'run-{index}'))
        time.sleep(2.5)
        first, cursor = board_round(owner)
        assert 'result' in owner.call(submit(1200, identity='run-7'))
        time.sleep(0.4)
        second, _ = board_round(owner, cursor)
        rounds = [first, second]
        owner.close()
        recording = dict(kind='board', rounds=rounds)
        (into / f'{name}.json').write_text(json.dumps(recording, indent=1) + '\n')
    finally:
        daemon.kill()
        daemon.wait(timeout=10)
        for handle in files:
            handle.close()
        shutil.rmtree(root, ignore_errors=True)


def record_harness(out, into, name):
    """One run that asks, is answered, and works: the walk while it waits and
    after, and the blocks as they were read, with the exit that carries the
    audit (or, for Codex, does not)."""
    import proof_harness
    from transcript_blocks import Transcript
    harness = proof_harness.load(name)

    class Recorder(Transcript):
        def feed(self, data):
            self.recorded.append(base64.b64encode(data).decode())
            super().feed(data)

    scenario = dict(harness.ask, **harness.work)
    with harness.service(out, f'parity-{name}', **scenario) as svc:
        svc.start()
        svc.submit(identity='run-1', delivery_timeout=300)
        screen = Recorder(svc, 'run-1', harness=harness)
        screen.recorded = []
        for _ in range(200):
            screen.pull()
            still = svc.inspect('run-1')
            if still['runtime'] == 'requires_action' and screen.blocks:
                break
        assert still['runtime'] == 'requires_action', still
        with svc.client() as client:
            waiting = flat_events(events_pages(client))
        svc.respond(still['runtime_detail']['action_id'], harness.allow, still['revision'],
                    identity='run-1')
        from opencode_host_matrix import poll
        poll(lambda: svc.inspect('run-1'), lambda v: v['runtime'] == 'exited', seconds=200)
        for _ in range(60):
            if not screen.pull():
                break
        with svc.client() as client:
            after = flat_events(events_pages(client))
        exit_event = next((e for e in after if e['type'] == 'execution.exit.observed'), None)
        svc.finish()
    write = lambda stem, value: (into / f'{stem}.json').write_text(json.dumps(value, indent=1) + '\n')
    write(f'walk-{name}-waiting', dict(kind='walk', events=waiting))
    write(f'walk-{name}-answered', dict(kind='walk', events=after))
    write(f'blocks-{name}', dict(kind='blocks', harness=name, reads=screen.recorded,
                                 exit=exit_event))


def record_desk(out, into):
    """Two runs whose arrival and deadline orders disagree; the caller
    answers one and PIO lapses the other. Then a decline PIO makes itself."""
    from approval_desk import ASKS, DECLINES, events_of, service
    from opencode_host_matrix import poll
    with service(out, 'parity-desk', permission_request=ASKS) as case:
        case.start()
        for identity, seconds in (('run-1', 300), ('run-2', 8)):
            case.submit(identity=identity, delivery_timeout=seconds)
            poll(lambda i=identity: case.inspect(i),
                 lambda v: v['runtime'] == 'requires_action' and v['delivery'] == 'acknowledged',
                 seconds=120)
        both = events_of(case)
        with case.client() as client:
            waiting_round, cursor = board_round(client)
        view = case.inspect('run-1')
        case.respond(view['runtime_detail']['action_id'], 'deny', view['revision'],
                     identity='run-1')
        poll(lambda: case.inspect('run-2'), lambda v: v['runtime'] == 'exited', seconds=200)
        settled = events_of(case)
        with case.client() as client:
            settled_round, _ = board_round(client, cursor)
        case.finish()
    with service(out, 'parity-decline', permission_request=DECLINES) as case:
        case.start()
        case.submit(identity='run-3')
        poll(lambda: case.inspect('run-3'), lambda v: v['runtime'] == 'exited', seconds=200)
        declined = events_of(case)
        case.finish()
    for stem, events in (('walk-desk-two-waiting', both), ('walk-desk-settled', settled),
                         ('walk-desk-declined', declined)):
        (into / f'{stem}.json').write_text(json.dumps(dict(kind='walk', events=events),
                                                      indent=1) + '\n')
    (into / 'board-desk.json').write_text(json.dumps(
        dict(kind='board', rounds=[waiting_round, settled_round]), indent=1) + '\n')


def record_codex_settlements(out, into):
    """The two decisions no one sent: Codex settling a request itself
    (`harness`) and a request that ended with its turn (`nobody`)."""
    import codex_host_matrix as codex
    for stem, scenario, submit in (
            ('walk-codex-harness', {'approval': 'command', 'approval_settles_itself_ms': 500,
                                    'delay_ms': 5000}, dict(delivery=2)),
            ('walk-codex-nobody', {'approval': 'command', 'delay_ms': 100},
             dict(deadline=4, delivery=120))):
        case = codex.Case(out, f'parity-{stem}', scenario=scenario)
        try:
            case.start()
            case.submit(**submit)
            codex.poll(lambda: case.inspect()['result'],
                       lambda v: v['runtime'] == 'exited', seconds=60)
            with case.client() as client:
                events = flat_events(events_pages(client))
                final, _ = board_round(client)
        finally:
            case.close()
        (into / f'{stem}.json').write_text(json.dumps(dict(kind='walk', events=events),
                                                      indent=1) + '\n')
        (into / f'board-{stem[5:]}.json').write_text(json.dumps(
            dict(kind='board', rounds=[final]), indent=1) + '\n')


def record(out, into):
    into.mkdir(parents=True, exist_ok=True)
    work = out / 'services'
    work.mkdir(parents=True, exist_ok=True)
    record_board(work, into)
    record_board(work, into, retain=4)
    for name in ('opencode', 'claude', 'codex'):
        record_harness(work, into, name)
    record_desk(work, into)
    record_codex_settlements(work, into)
    for path in recordings(into):
        scrub(path)
        text = path.read_text()
        for decoded in [text] + [base64.b64decode(r).decode(errors='replace')
                                 for r in json.loads(text).get('reads', [])]:
            assert '/Users/' not in decoded and '/home/' not in decoded, \
                f'{path.name} carries a home path'


# A case's private temporary root, however the platform spells /tmp.
TEMPORARY = re.compile(r'(?:/private)?/tmp/pio-[A-Za-z0-9_.-]+')


def scrub(path):
    """One name for every case's temporary root, in the recording and inside
    the transcript bytes it carries, so a committed fixture names no
    machine's /tmp. Each read is rewritten on its own, so where the reads
    were cut is kept."""
    recording = json.loads(TEMPORARY.sub('/tmp/pio-fixture', path.read_text()))
    if 'reads' in recording:
        recording['reads'] = [
            # latin-1 maps bytes one to one, so a character a read cut in
            # half survives the round trip.
            base64.b64encode(TEMPORARY.sub('/tmp/pio-fixture',
                                           base64.b64decode(r).decode('latin-1'))
                             .encode('latin-1')).decode()
            for r in recording['reads']]
    path.write_text(json.dumps(recording, indent=1) + '\n')


# --- mutants ------------------------------------------------------------------

def mutant_binary(name, scratch):
    """The fold example built from a clean worktree of HEAD with one edit."""
    relative, old, new, _ = MUTANTS[name]
    tree = scratch / 'tree'
    subprocess.run(['git', 'worktree', 'add', '--detach', '--force', str(tree), 'HEAD'],
                   cwd=ROOT, check=True, capture_output=True)
    source = tree / relative
    text = source.read_text()
    assert text.count(old) == 1, f'mutant {name}: the edit no longer applies to {relative}'
    source.write_text(text.replace(old, new))
    # One target directory for every mutant build, inside the repository's
    # own (ignored) target: the dependencies compile once, not per mutant.
    return fold_binary(tree, ROOT / 'target/fold-mutant')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--record', action='store_true',
                        help='record fresh streams from the fake services and check those too')
    parser.add_argument('--write-fixtures', action='store_true',
                        help='with --record: replace the committed recordings with the fresh ones')
    parser.add_argument('--write-expected', action='store_true',
                        help="write the Python fold's answer beside each committed recording")
    parser.add_argument('--mutant', choices=sorted(MUTANTS))
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    scratch, binary = None, None
    try:
        if args.mutant:
            scratch = Path(tempfile.mkdtemp(prefix='pio-fpm-', dir='/tmp')).resolve()
            binary = mutant_binary(args.mutant, scratch)
        else:
            binary = fold_binary()
        paths = recordings(FIXTURES)
        if args.record:
            fresh = out / 'recorded'
            record(out, fresh)
            if args.write_fixtures:
                for old in FIXTURES.glob('*.json'):
                    old.unlink()
                for path in recordings(fresh):
                    shutil.copy(path, FIXTURES / path.name)
                paths = recordings(FIXTURES)
            else:
                paths += recordings(fresh)
        if args.write_expected:
            for path in recordings(FIXTURES):
                expected = python_fold(json.loads(path.read_text()))
                (FIXTURES / f'{path.stem}.expected.json').write_text(
                    json.dumps(expected, indent=1, sort_keys=True, ensure_ascii=False) + '\n')
        assert paths, f'no recordings in {FIXTURES}'
        results = compare(binary, paths, out)
    finally:
        if scratch:
            subprocess.run(['git', 'worktree', 'remove', '--force', str(scratch / 'tree')],
                           cwd=ROOT, capture_output=True)
            shutil.rmtree(scratch, ignore_errors=True)

    kinds = sorted({r['kind'] for r in results})
    record_ = dict(format='pio-fold-parity/1', mutant=args.mutant, kinds=kinds,
                   recordings=len(results), results=results)
    (out / 'fold-parity.json').write_text(json.dumps(record_, indent=2) + '\n')
    differing = [r for r in results if not r['identical']]
    for result in results:
        print(f"{'same' if result['identical'] else 'DIFF'}  {result['kind']:6}  "
              f"{result['recording']}" + (f"  at {result['first_difference']}"
                                          if result['first_difference'] else ''))
    if args.mutant:
        field = MUTANTS[args.mutant][3]
        named = [r for r in differing if field in r['fields']]
        if not named:
            raise SystemExit(f'mutant {args.mutant} SURVIVED: no recording differed at '
                             f'a {field!r} field ({len(differing)} differed elsewhere)')
        print(f'mutant {args.mutant} killed: {len(named)} recording(s) differ at {field!r}, '
              f'fields changed: {sorted({f for r in differing for f in r["fields"]})}')
        return
    if differing:
        raise SystemExit(f'{len(differing)} of {len(results)} recordings fold differently '
                         f'in Rust and Python')
    print(f'fold parity: {len(results)} recordings ({", ".join(kinds)}), the Rust and '
          'Python folds agree field for field')


if __name__ == '__main__':
    main()
