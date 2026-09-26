#!/usr/bin/env python3
"""The mutant for pio-client's public-wire boundary.

`crates/pio-client/tests/dependencies.rs` fails if pio-client reaches a PIO
service crate by any path. This proves that test can fail: in a clean
worktree of HEAD it gives pio-client the dependency the boundary forbids and
requires the test to fail **naming it**, so a test that passes for some other
reason (or never runs) does not count.

    client_boundary_mutant.py [--dependency pio-core|pio-protocol|pio-host]

`pio-protocol` is reached through its own dependency on pio-core as well, so
that variant also shows a transitive edge is caught.
"""
import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FORBIDDEN = {'pio-core': '{ path = "../pio-core" }',
             'pio-protocol': '{ path = "../pio-protocol", default-features = false }',
             'pio-host': '{ path = "../pio-host" }'}


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--dependency', choices=sorted(FORBIDDEN), default='pio-core')
    args = parser.parse_args()
    scratch = Path(tempfile.mkdtemp(prefix='pio-bm-', dir='/tmp')).resolve()
    tree = scratch / 'tree'
    try:
        subprocess.run(['git', 'worktree', 'add', '--detach', '--force', str(tree), 'HEAD'],
                       cwd=ROOT, check=True, capture_output=True)
        manifest = tree / 'crates/pio-client/Cargo.toml'
        manifest.write_text(manifest.read_text()
                            + f'{args.dependency} = {FORBIDDEN[args.dependency]}\n')
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / 'target/fold-mutant'))
        # Not --locked: the mutant changes the graph, which is the point.
        result = subprocess.run(['cargo', 'test', '--quiet', '-p', 'pio-client',
                                 '--test', 'dependencies'],
                                cwd=tree, env=env, capture_output=True, text=True)
    finally:
        subprocess.run(['git', 'worktree', 'remove', '--force', str(tree)], cwd=ROOT,
                       capture_output=True)
        shutil.rmtree(scratch, ignore_errors=True)
    output = result.stdout + result.stderr
    if result.returncode == 0:
        raise SystemExit(f'mutant {args.dependency} SURVIVED: the boundary test passed')
    said = [l.strip() for l in output.splitlines() if 'reaches PIO service internals' in l]
    if not any(f'"{args.dependency}"' in line for line in said):
        raise SystemExit(f'mutant {args.dependency}: the test failed, but not on the '
                         f'boundary:\n{output[-2000:]}')
    print(f'mutant {args.dependency} killed: {said[0]}')


if __name__ == '__main__':
    main()
