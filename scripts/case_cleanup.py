"""Release a test case's store: kill what it started, then remove it.

Two things went wrong before this existed, both found by inspection rather
than by a failing test.

**Stores were retained.** Every matrix called `close()` in a `finally`, but
`close()` harvested evidence and left the store behind, so a clean run left
one directory per attempt on `/tmp` — 54 from the Codex matrix, 72 from the
public matrix, 54 diagnostic, 3 from caller recovery.

**Processes outlived the case.** A host that does not exit — a mutant that
never escalates, say — is detached from the matrix, so closing the case left
the host and its harness child running. Removing the directory underneath a
live process is worse than leaving it.

So cleanup kills every process whose command line names the store root, waits,
kills harder, and **asserts none remain** before removing anything. The filter
is the store root itself, which is a private temporary directory, so this can
never reach a process the matrix did not start.
"""
import shutil
import subprocess
import time


def processes_under(root):
    """Every process whose command line names this store root."""
    table = subprocess.run(['ps', '-axww', '-o', 'pid=', '-o', 'command='],
                           capture_output=True, text=True).stdout
    return [line.strip() for line in table.splitlines()
            if str(root) in line and 'ps -axww' not in line]


def terminate_under(root, seconds=5):
    """Stop everything this case started. Returns any survivor, which is a
    failure: a store must not be removed while a process still uses it."""
    for signal in ('-TERM', '-KILL'):
        found = processes_under(root)
        if not found:
            return []
        for line in found:
            pid = line.split(maxsplit=1)[0]
            subprocess.run(['kill', signal, pid], capture_output=True)
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and processes_under(root):
            time.sleep(0.05)
    return processes_under(root)


def release(root, remove=True):
    """Kill, assert nothing survives, then remove the store."""
    survivors = terminate_under(root)
    assert not survivors, f'processes survived cleanup of {root}: {survivors}'
    if remove:
        shutil.rmtree(root, ignore_errors=True)
