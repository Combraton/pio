#!/usr/bin/env python3
"""Fail if any test case left its store behind.

A case's store holds a journal, a spool, host event files and the sockets a
daemon listens on. `case_cleanup.release` kills the case's process groups,
asserts nothing survives, and removes the directory — but only when something
calls it. Four stores survived on `/tmp` because two proof scripts called
`cleanup()` on the success path alone, so a failing assertion left the store,
the daemon and the harness behind, and a killed process left them for ever.

The scripts are fixed. This is the check that would have said so without
anyone looking, and it is deliberately blunt: **after the suites have run, a
store directory that still exists is a leak**, whether or not anything is
still running inside it. A store with live processes is the worse of the two
and is reported as such.

`--selftest` plants one and proves the check fails, because a gate that has
never failed is not known to work.
"""
import argparse
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup

# Where the suites put their stores, and the prefixes they use. Every one of
# these comes from a `mkdtemp(prefix=...)` in a script beside this one.
ROOTS = ['/tmp', '/private/tmp']
PREFIXES = ('pio-oc-', 'pio-cl-', 'pio-cx-', 'pio-board-', 'pio-caller-',
            'pio-pkg-', 'pio-fh-', 'pio-outside-', 'private-path-')


def stores():
    """Every store directory still on disk, deduplicated by real path."""
    found = {}
    for root in ROOTS:
        if not os.path.isdir(root):
            continue
        for entry in sorted(os.listdir(root)):
            if not entry.startswith(PREFIXES):
                continue
            path = os.path.join(root, entry)
            if os.path.isdir(path) and not os.path.islink(path):
                found[os.path.realpath(path)] = path
    return sorted(found.values())


def describe(path):
    age = time.time() - os.stat(path).st_mtime
    live = case_cleanup.processes_under(os.path.realpath(path))
    return (f'  {path}  (idle {age / 60:.0f} min, '
            f'{len(live)} process(es) still inside)'
            + ''.join(f'\n      pid {pid} pgid {pgid} {command[:90]}'
                      for pid, pgid, command in live))


def check():
    leaked = stores()
    if not leaked:
        print('case stores: none left behind')
        return 0
    print(f'case stores: {len(leaked)} left behind')
    for path in leaked:
        print(describe(path))
    print('\nA store survives when cleanup did not run. Every pass that starts '
          'a service must release it in a `finally`, not on the success path.')
    return 1


def selftest():
    """Plant a store and prove the check fails on it."""
    assert check() == 0, 'selftest needs a clean slate; release the stores above first'
    planted = tempfile.mkdtemp(prefix='pio-oc-', dir='/tmp')
    os.chmod(planted, 0o700)
    Path(planted, 'journal.sqlite3').write_bytes(b'not a database')
    try:
        assert check() == 1, 'the check passed with a planted store on disk'
        assert os.path.realpath(planted) in [os.path.realpath(p) for p in stores()], \
            'the planted store was not the one found'
    finally:
        shutil.rmtree(planted, ignore_errors=True)
    assert check() == 0, 'the check still fails once the planted store is gone'
    print('check_case_leaks selftest: a planted store fails the check')
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--release', action='store_true',
                        help='release what is left, for local use; never in CI')
    args = parser.parse_args()
    if args.selftest:
        raise SystemExit(selftest())
    if args.release:
        for path in stores():
            print(f'releasing {path}')
            case_cleanup.release(path)
    raise SystemExit(check())


if __name__ == '__main__':
    main()
