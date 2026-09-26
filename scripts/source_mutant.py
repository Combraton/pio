#!/usr/bin/env python3
"""A source mutant: a clean worktree of HEAD with named edits, built apart.

The mutant scripts for M4 T1 (`fold_parity.py`, `client_source_mutants.py`,
`client_cli_matrix.py --mutant`) break one thing in the source and require a
named check to fail. They never touch the checkout they run from: the edit
goes into a worktree of HEAD at one fixed place under the ignored `target/`,
built into one fixed target directory, so the dependencies and the crates the
edit does not touch compile once and are reused by every later mutant. Only
one mutant may be built at a time.
"""
import contextlib
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TREE = ROOT / 'target/mutant-tree'
TARGET = ROOT / 'target/mutant-target'
SYMLINK = object()


def _remove_tree():
    subprocess.run(['git', 'worktree', 'remove', '--force', str(TREE)], cwd=ROOT,
                   capture_output=True)
    subprocess.run(['git', 'worktree', 'prune'], cwd=ROOT, capture_output=True)
    shutil.rmtree(TREE, ignore_errors=True)


@contextlib.contextmanager
def mutated(edits):
    """`edits` is a list of `(relative path, old, new)`. `old` must occur
    exactly once; `old=None` appends `new` to the file, creating it if it
    does not exist; `old=SYMLINK` makes the path a symlink to `new`."""
    _remove_tree()
    TREE.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(['git', 'worktree', 'add', '--detach', '--force', str(TREE), 'HEAD'],
                   cwd=ROOT, check=True, capture_output=True)
    try:
        for relative, old, new in edits:
            path = TREE / relative
            if old is SYMLINK:
                path.parent.mkdir(parents=True, exist_ok=True)
                os.symlink(new, path)
                continue
            if old is None and not path.exists():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(new)
                continue
            text = path.read_text()
            if old is None:
                text += new
            else:
                assert text.count(old) == 1, \
                    f'the mutant edit no longer applies once to {relative}: {old!r}'
                text = text.replace(old, new)
            path.write_text(text)
        yield TREE
    finally:
        _remove_tree()


def cargo(tree, *args, check=True):
    """Cargo in the mutated tree, into the shared mutant target directory."""
    env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET))
    return subprocess.run(['cargo', *args], cwd=tree, env=env, check=check,
                          capture_output=not check, text=True)
