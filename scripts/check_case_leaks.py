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
import re
import shutil
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import case_cleanup

# Where the suites put their temporary directories. Two of them pass no
# `dir=`, so they land in `$TMPDIR` — on macOS that is under `/var/folders`,
# not `/tmp`, and a list of two roots missed them.
ROOTS = ['/tmp', '/private/tmp', tempfile.gettempdir()]
# Everything this repo creates is named `pio-…`, so the rule is the shape
# rather than a list that has to be kept in step with the scripts. Two names
# predate the convention and are named here because they cannot be inferred.
SHAPE = re.compile(r'^pio-')
EXCEPTIONS = ('oc-selftest-', 'private-path-')
SCRIPTS = Path(__file__).resolve().parent
LITERAL = re.compile(r'''mkdtemp\(\s*prefix\s*=\s*['"]([^'"]+)['"]''')


def covered(name):
    return bool(SHAPE.match(name)) or name.startswith(EXCEPTIONS)


def declared_prefixes():
    """Every prefix a script beside this one actually creates."""
    found = {}
    for script in sorted(SCRIPTS.glob('*.py')):
        for prefix in LITERAL.findall(script.read_text()):
            found.setdefault(prefix, script.name)
    return found


def uncovered():
    """Prefixes this check would not recognise if they leaked.

    The list of prefixes used to be maintained by hand, and it had already
    fallen behind by three. Deriving the answer from the scripts means the
    next one cannot slip past: a prefix that does not fit the shape has to be
    named as an exception, deliberately.
    """
    return {prefix: script for prefix, script in declared_prefixes().items()
            if not covered(prefix)}


def stores():
    """Every store directory still on disk, deduplicated by real path."""
    found = {}
    for root in ROOTS:
        if not os.path.isdir(root):
            continue
        for entry in sorted(os.listdir(root)):
            if not covered(entry):
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
    # Two different failures, reported apart. A prefix nothing recognises is
    # not a leak yet — it is a leak this check would miss.
    missing = uncovered()
    if missing:
        print(f'case stores: {len(missing)} prefix(es) this check would not see')
        for prefix, script in sorted(missing.items()):
            print(f'  {prefix!r} from {script}')
        print('\nName it in EXCEPTIONS, or rename it to the pio- shape.')
        return 1
    leaked = stores()
    if not leaked:
        print(f'case stores: none left behind '
              f'({len(declared_prefixes())} prefixes covered)')
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

    # And the coverage half: a prefix no rule recognises fails the check,
    # which is what stops the next one slipping past.
    global EXCEPTIONS
    kept, EXCEPTIONS = EXCEPTIONS, ()
    try:
        assert uncovered(), 'dropping the exceptions left nothing uncovered'
        assert check() == 1, 'the check passed with an unrecognised prefix'
    finally:
        EXCEPTIONS = kept
    assert not uncovered(), f'these prefixes are not covered: {uncovered()}'
    print('check_case_leaks selftest: a planted store and an unknown prefix '
          'each fail the check')
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
