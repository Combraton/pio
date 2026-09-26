#!/usr/bin/env python3
"""Source mutants for pio-client and the command line, checked by `cargo test`.

Each mutant edits a clean worktree of HEAD (see `source_mutant.py`) and the
named test must fail **for that reason**. Most go round the public-wire
boundary: `crates/pio-client/tests/dependencies.rs` fails if pio-client, or
the command-line code on it, reaches a PIO service crate, and each of these
reaches one another way. The verifier's two bypasses of the first cut are B1
and B2. (The command line's own mutants, which need the binary, are in
`client_cli_matrix.py --mutant`.)

    client_source_mutants.py [--mutant NAME]...     (all of them by default)

- `direct-pio-core`: pio-client depends on pio-core;
- `transitive-pio-protocol`: on pio-protocol, which reaches pio-core too;
- `B1-optional-feature`: an optional pio-core behind a feature, re-exported;
- `B2-path-include`: a service source file compiled in with a module path;
- `cli-borrows-service`: the command-line code names pio_protocol;
- `ledger-lenient`: the caller ledger parses its request with serde_json,
  which takes a duplicate key the service refuses;
- `watch-notices-repeat`: a follower checks only events against its saved
  position, so a gap or an epoch change is repeated when it resumes inside a
  page;
- `watch-stop-keeps-page`: a follower stopped on a page's last item keeps
  its cursor at the page's start.
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import source_mutant

CLIENT_TOML = 'crates/pio-client/Cargo.toml'
CLIENT_LIB = 'crates/pio-client/src/lib.rs'
BOUNDARY = ['test', '--quiet', '-p', 'pio-client', '--test', 'dependencies']
REACH = 'reaches PIO service internals through'

MUTANTS = {
    'direct-pio-core': (
        [(CLIENT_TOML, None, 'pio-core = { path = "../pio-core" }\n')],
        BOUNDARY, [REACH, '"pio-core"']),
    'transitive-pio-protocol': (
        [(CLIENT_TOML, None,
          'pio-protocol = { path = "../pio-protocol", default-features = false }\n')],
        BOUNDARY, [REACH, '"pio-protocol"', '"pio-host"']),
    'B1-optional-feature': (
        [(CLIENT_TOML, None, 'pio-core = { path = "../pio-core", optional = true }\n\n'
                             '[features]\nleak = ["dep:pio-core"]\n'),
         (CLIENT_LIB, None, '#[cfg(feature = "leak")]\npub use pio_core;\n')],
        BOUNDARY, [REACH, 'outside its allow-list', 'pio-core (by path)']),
    'B2-path-include': (
        [(CLIENT_LIB, None, '#[' + 'path = "../../pio-protocol/src/encoding.rs"]\n'
                            '#[allow(dead_code)]\nmod leak;\n')],
        BOUNDARY, ['sources reach outside the crate', 'encoding.rs']),
    'cli-borrows-service': (
        [('crates/pio-cli/src/client_cli.rs', 'use crate::ledger;\n',
          'use crate::ledger;\n#[allow(unused_imports)]\nuse pio_protocol as _service;\n')],
        BOUNDARY, ['the command line reaches past the public client', 'pio_protocol']),
    'ledger-lenient': (
        [('crates/pio-cli/src/ledger.rs', 'Ok(pio_client::encoding::parse(bytes)?)',
          'Ok(serde_json::from_slice(bytes)?)')],
        ['test', '--quiet', '-p', 'pio-cli', '--bin', 'pio', 'ledger::'],
        ['a_request_file_is_parsed_as_strictly_as_the_service_parses_it', 'FAILED']),
    'watch-notices-repeat': (
        [('crates/pio-client/src/watch.rs',
          '        if !state.is_new(item) {\n            continue;\n        }\n'
          '        let keep_going',
          '        if item.get("event").is_some() && !state.is_new(item) {\n'
          '            continue;\n        }\n        let keep_going')],
        ['test', '--quiet', '-p', 'pio-client', '--lib', 'watch::'],
        ['a_follower_that_stops_inside_a_page_repeats_nothing_on_resuming', 'FAILED']),
    'watch-stop-keeps-page': (
        [('crates/pio-client/src/watch.rs', 'if stop && index + 1 < items.len() {',
          'if stop {')],
        ['test', '--quiet', '-p', 'pio-client', '--lib', 'watch::'],
        ['a_stop_on_the_last_item_moves_past_the_page', 'FAILED']),
}


def run(name):
    edits, test, needles = MUTANTS[name]
    with source_mutant.mutated(edits) as tree:
        # Not --locked: a mutant that adds a dependency changes the graph,
        # which is the point.
        result = source_mutant.cargo(tree, *test, check=False)
    output = result.stdout + result.stderr
    if result.returncode == 0:
        raise SystemExit(f'mutant {name} SURVIVED: {" ".join(test)} passed')
    missing = [n for n in needles if n not in output]
    if missing:
        raise SystemExit(f'mutant {name}: the test failed, but not for the named reason '
                         f'(missing {missing}):\n{output[-3000:]}')
    said = next((l.strip() for l in output.splitlines() if needles[0] in l), needles[0])
    print(f'mutant {name} killed: {said[:300]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--mutant', choices=sorted(MUTANTS), action='append')
    args = parser.parse_args()
    for name in args.mutant or list(MUTANTS):
        run(name)


if __name__ == '__main__':
    main()
