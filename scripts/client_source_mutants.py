#!/usr/bin/env python3
"""Source mutants for pio-client and the command line, checked by `cargo test`.

Each mutant edits a clean worktree of HEAD (see `source_mutant.py`) and the
named test must fail **for that reason**. Most go round the public-wire
boundary, which `crates/pio-boundary` holds structurally: the dependency
graph with every feature on, an allow-list, the source files rustc's own
dep-info says it compiled, and no symlink in the crate. B1 to B7 are the
verifier's bypasses (review of T1, rounds 1 and 2): the first cut's text
scans lost to B3 to B7. (The command line's own mutants, which need the
binary, are in `client_cli_matrix.py --mutant`.)

    client_source_mutants.py [--mutant NAME]...     (all of them by default)

- `direct-pio-core`, `transitive-pio-protocol`: pio-client depends on the
  service, directly or through pio-protocol;
- `B1-optional-feature`: an optional pio-core behind a feature, re-exported;
- `B2-path-include`, `B3-cfg-attr-path`, `B4-spaced-attribute`: a service
  source file compiled into pio-client by `#[path]`, by `cfg_attr`, and by
  `# [path]` with a space;
- `B5-symlinked-module`: a module file in pio-client that is a symlink to a
  service source file;
- `B6-helper-module`: a helper module under `client_cli/` that uses pio_core;
- `B7-glob-import`: the CLI crate's root imports pio_core, reached with
  `use super::*` (in pio-cli this was main.rs; the CLI now has a crate of
  its own, and its root cannot name pio_core);
- `cli-borrows-service`: the command line names pio_protocol;
- `cli-depends-on-core`: pio-client-cli declares pio-core;
- `ledger-lenient`: the caller ledger parses its request with serde_json,
  which takes a duplicate key the service refuses;
- `watch-notices-repeat`: a follower checks only events against its saved
  position, so a gap or an epoch change is repeated when it resumes;
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
CLI_TOML = 'crates/pio-client-cli/Cargo.toml'
CLI_LIB = 'crates/pio-client-cli/src/lib.rs'
CLI_MAIN = 'crates/pio-client-cli/src/client_cli.rs'
BOUNDARY = ['test', '--quiet', '-p', 'pio-boundary']
REACH = 'reaches PIO service internals through'
OUTSIDE = 'compiles source files from outside its directory'
UNBUILT = 'does not build on its own declared dependencies'
ENCODING = '../../pio-protocol/src/encoding.rs'
LEAK = '#[allow(dead_code)]\nmod leak;\n'

MUTANTS = {
    'direct-pio-core': (
        [(CLIENT_TOML, None, 'pio-core = { path = "../pio-core" }\n')],
        BOUNDARY, [REACH, '"pio-core"']),
    'transitive-pio-protocol': (
        [(CLIENT_TOML, None,
          'pio-protocol = { path = "../pio-protocol", default-features = false }\n')],
        BOUNDARY, [REACH, '"pio-protocol"', '"pio-host"']),
    # The review's bypasses, each tried on the crate it named.
    'B1-optional-feature': (
        [(CLIENT_TOML, None, 'pio-core = { path = "../pio-core", optional = true }\n\n'
                             '[features]\nleak = ["dep:pio-core"]\n'),
         (CLIENT_LIB, None, '#[cfg(feature = "leak")]\npub use pio_core;\n')],
        BOUNDARY, [REACH, 'outside its allow-list', 'pio-core (by path)']),
    'B2-path-include': (
        [(CLIENT_LIB, None, f'#[path = "{ENCODING}"]\n' + LEAK)],
        BOUNDARY, [OUTSIDE, 'pio-protocol/src/encoding.rs']),
    'B3-cfg-attr-path': (
        [(CLIENT_LIB, None, f'#[cfg_attr(all(), path = "{ENCODING}")]\n' + LEAK)],
        BOUNDARY, [OUTSIDE, 'pio-protocol/src/encoding.rs']),
    'B4-spaced-attribute': (
        [(CLIENT_LIB, None, f'# [path = "{ENCODING}"]\n' + LEAK)],
        BOUNDARY, [OUTSIDE, 'pio-protocol/src/encoding.rs']),
    'B5-symlinked-module': (
        [('crates/pio-client/src/leak.rs', source_mutant.SYMLINK, ENCODING),
         (CLIENT_LIB, None, LEAK)],
        BOUNDARY, ['has symlinks inside its directory', 'crates/pio-client/src/leak.rs',
                   OUTSIDE]),
    'B6-helper-module': (
        [(CLI_MAIN, None, '\nmod helper;\n'),
         ('crates/pio-client-cli/src/client_cli/helper.rs', None,
          '#[allow(unused_imports)]\nuse pio_core::digest;\n')],
        BOUNDARY, [UNBUILT, 'pio_core']),
    'B7-glob-import': (
        [(CLI_LIB, None, '#[allow(unused_imports)]\nuse pio_core::digest;\n'),
         (CLI_MAIN, 'use crate::ledger;\n',
          'use crate::ledger;\n#[allow(unused_imports)]\nuse super::*;\n')],
        BOUNDARY, [UNBUILT, 'pio_core']),
    'cli-borrows-service': (
        [(CLI_MAIN, 'use crate::ledger;\n',
          'use crate::ledger;\n#[allow(unused_imports)]\nuse pio_protocol as _service;\n')],
        BOUNDARY, [UNBUILT, 'pio_protocol']),
    'cli-depends-on-core': (
        [(CLI_TOML, '[dev-dependencies]', 'pio-core = { path = "../pio-core" }\n\n'
                                          '[dev-dependencies]')],
        BOUNDARY, [REACH, '"pio-core"', 'pio-core (by path)']),
    'ledger-lenient': (
        [('crates/pio-client-cli/src/ledger.rs', 'Ok(pio_client::encoding::parse(bytes)?)',
          'Ok(serde_json::from_slice(bytes)?)')],
        ['test', '--quiet', '-p', 'pio-client-cli', '--lib', 'ledger::'],
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
