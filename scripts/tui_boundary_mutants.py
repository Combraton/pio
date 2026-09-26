#!/usr/bin/env python3
"""Source mutants for the screen's boundary, checked by `cargo test`.

`crates/pio-tui/tests/boundary.rs` holds M4 rule 1 for `pio tui` by what the
toolchain knows: the dependency graph, rustc's dep-info (every file compiled
in), and no symlink in the crate. Each mutant edits a clean worktree of HEAD
(`source_mutant.py`) so that pio-tui reaches a service crate one way, and the
boundary test must fail **for that reason**. Three of the ways are the ones
that went round the old text scan: a module path in a `cfg_attr`, a path
with a space after `#`, and a symlinked module. The leaked file is a real
service source (`pio-host/src/script.rs`), chosen because it compiles on
pio-tui's own dependencies, so the build succeeds and only the boundary can
object.

    tui_boundary_mutants.py [--mutant NAME]...     (all of them by default)

- `depends-on-core`: pio-tui depends on pio-core by path;
- `path-include`: a service source compiled in with a module path;
- `cfg-attr-path`: the same module path, inside `cfg_attr`;
- `spaced-path`: the same, written `# [path = ...]`;
- `include-macro`: the same file pulled in with `include!`;
- `symlinked-module`: `src/leak.rs` is a symlink to it.
"""
import argparse
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import source_mutant

TOML = 'crates/pio-tui/Cargo.toml'
LIB = 'crates/pio-tui/src/lib.rs'
LEAK = '../../pio-host/src/script.rs'
TEST = ['test', '--quiet', '-p', 'pio-tui', '--test', 'boundary']
GRAPH = 'reaches workspace crates other than pio-client'
COMPILED = 'rustc compiled files from outside crates/pio-tui'
LINK = 'holds a symlink'


def module(attribute):
    return (LIB, None, f'\n{attribute}\n#[allow(dead_code, unused_imports)]\nmod leak;\n')


MUTANTS = {
    'depends-on-core': ([(TOML, None, 'pio-core = { path = "../pio-core" }\n')], None,
                        [GRAPH, '"pio-core"', '(by path)']),
    'path-include': ([module(f'#[path = "{LEAK}"]')], None, [COMPILED, 'script.rs']),
    'cfg-attr-path': ([module(f'#[cfg_attr(all(), path = "{LEAK}")]')], None,
                      [COMPILED, 'script.rs']),
    'spaced-path': ([module(f'# [path = "{LEAK}"]')], None, [COMPILED, 'script.rs']),
    'include-macro': ([(LIB, None, '\n#[allow(dead_code, unused_imports)]\nmod leak {\n'
                        f'    include!("{LEAK}");\n}}\n')], None, [COMPILED, 'script.rs']),
    'symlinked-module': ([(LIB, None, '\n#[allow(dead_code, unused_imports)]\nmod leak;\n')],
                         lambda tree: os.symlink(
                             LEAK.replace('../../', '../../'),
                             tree / 'crates/pio-tui/src/leak.rs'),
                         [LINK, 'leak.rs']),
}


def run(name):
    edits, setup, needles = MUTANTS[name]
    with source_mutant.mutated(edits) as tree:
        if setup:
            setup(tree)
        # Not --locked: a mutant that adds a dependency changes the graph,
        # which is the point.
        result = source_mutant.cargo(tree, *TEST, check=False)
    output = result.stdout + result.stderr
    if result.returncode == 0:
        raise SystemExit(f'mutant {name} SURVIVED: {" ".join(TEST)} passed')
    if 'error[E' in output or 'could not compile' in output:
        raise SystemExit(f'mutant {name} did not build, so it proves nothing:\n{output[-3000:]}')
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
