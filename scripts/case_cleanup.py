"""Release a test case's store: kill what it started, then remove it.

Three things went wrong before this reached its current shape, each found by
someone looking rather than by a failing test.

**Stores were retained.** Every matrix called `close()` in a `finally`, but
`close()` harvested evidence and left the directory behind, so a clean run left
one directory per attempt on `/tmp`.

**Processes outlived the case.** A host that does not exit is detached from the
matrix, so closing the case left it running.

**The harness child outlived the host.** Killing only the processes whose
command line names the store missed the harness itself: `pio claude fake-cli`
is spawned with the arguments a real Claude Code would get, and none of them
names the store. The host calls `setsid`, so it leads its own process group and
the harness is inside it — the group is what must be killed, not one process.

**And a walk alone is not enough.** Once the host is killed, the harness is
reparented to init: it names no store and descends from nothing that does, so
it becomes invisible to any search that starts from the store. The process
group outlives the host, so the groups seen while it was alive are remembered,
and a remembered group is killed only if a process it recorded is still alive
under the same group with the same command line — which is what keeps a reused
group id from reaching something else.

So cleanup finds every process that names the store, takes their **process
groups** and their descendants, signals the groups, and **asserts both the
groups and the store are empty** before removing anything. A store removed
while a process still uses it is worse than one left behind.
"""
import os
import shutil
import signal
import subprocess
import time

# A root must be under one of these to be removable. A store lives in a private
# temporary directory or a run tree the caller registered.
PERMITTED_PREFIXES = ['/tmp', '/private/tmp', '/var/folders', '/private/var/folders']


def permit_prefix(prefix):
    """Register a run tree, such as a live runner's private root."""
    prefix = os.path.realpath(prefix)
    if prefix not in PERMITTED_PREFIXES:
        PERMITTED_PREFIXES.append(prefix)


def _table():
    out = subprocess.run(['ps', '-axww', '-o', 'pid=', '-o', 'ppid=', '-o', 'pgid=',
                          '-o', 'command='], capture_output=True, text=True).stdout
    rows = []
    for line in out.splitlines():
        parts = line.split(maxsplit=3)
        if len(parts) == 4 and parts[0].isdigit():
            rows.append((int(parts[0]), int(parts[1]), int(parts[2]), parts[3]))
    return rows


# Groups seen for a root while its host was alive, with the processes that
# were in them. A remembered group is how an orphaned harness is found.
_SEEN = {}


def processes_under(root):
    """Every process whose command line names this store root, plus every
    descendant of one, plus anything still alive in a group seen earlier.

    The harness child names no store, so the walk finds it while the host
    lives; the remembered group finds it once the host is gone.
    """
    rows = _table()
    named = {pid for pid, _, _, command in rows if str(root) in command}
    children = {}
    for pid, ppid, _, _ in rows:
        children.setdefault(ppid, []).append(pid)
    found, queue = set(named), list(named)
    while queue:
        for child in children.get(queue.pop(), []):
            if child not in found:
                found.add(child)
                queue.append(child)
    live = {(pid, pgid): command for pid, _, pgid, command in rows}
    seen = _SEEN.setdefault(str(root), {})
    for pid, _, pgid, command in rows:
        if pid in found and pgid not in (os.getpgid(0), 0):
            seen.setdefault(pgid, {})[pid] = command
    # A remembered process counts only while it is still the same process in
    # the same group, so a reused pid or group id cannot pull in a stranger.
    for pgid, members in seen.items():
        for pid, command in members.items():
            if live.get((pid, pgid)) == command:
                found.add(pid)
                for other, other_pgid, _ in [(p, g, c) for (p, g), c in live.items()]:
                    if other_pgid == pgid:
                        found.add(other)
    return [(pid, pgid, command) for pid, _, pgid, command in rows if pid in found]


def groups_under(root):
    """The process groups a case owns. Never this process's own group."""
    mine = os.getpgid(0)
    groups = {pgid for _, pgid, _ in processes_under(root) if pgid not in (mine, 0)}
    return groups | {g for g in _SEEN.get(str(root), {})
                     if g not in (mine, 0) and _group_alive(g, _SEEN[str(root)][g])}


def _group_alive(pgid, members):
    """True while a remembered member is still that process in that group."""
    live = {(pid, gid): command for pid, _, gid, command in _table()}
    return any(live.get((pid, pgid)) == command for pid, command in members.items())


def terminate_under(root, seconds=5):
    """Stop everything this case started. Returns any survivor, which is a
    failure rather than something to tidy away."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        if not processes_under(root):
            return []
        # The host leads its own session, so the group carries the harness too.
        for pgid in groups_under(root):
            try:
                os.killpg(pgid, sig)
            except (ProcessLookupError, PermissionError):
                pass
        for pid, _, _ in processes_under(root):
            try:
                os.kill(pid, sig)
            except (ProcessLookupError, PermissionError):
                pass
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline and processes_under(root):
            time.sleep(0.05)
    return processes_under(root)


def _removable(root):
    """A root must be a private directory this run created. Anything else is
    refused rather than removed."""
    path = os.path.realpath(root)
    if not os.path.isdir(path) or os.path.islink(root):
        return False, 'not a directory, or a symlink'
    # Strictly *under* a permitted prefix, never the prefix itself. Counting
    # separators instead got this wrong across platforms: `/tmp/pio-x` has two
    # on Linux and three on macOS, where realpath prepends `/private`.
    if not any(path.startswith(p + os.sep) for p in PERMITTED_PREFIXES):
        return False, f'outside every permitted prefix: {PERMITTED_PREFIXES}'
    for forbidden in (os.path.realpath(os.path.expanduser('~')), os.getcwd()):
        if path == forbidden or forbidden.startswith(path + os.sep):
            return False, 'contains the home directory or the working directory'
    info = os.stat(path)
    if info.st_uid != os.geteuid() or info.st_mode & 0o077:
        return False, 'not owned privately by this user'
    return True, ''


def release(root, remove=True):
    """Validate the root, kill the case's process groups, assert nothing
    survives, then remove the store.

    **The guard runs first, and that ordering is the point.** Validating after
    killing means a wrong root — `/`, say — matches every command line on the
    machine and signals every process group before anything refuses it. Writing
    it the other way round killed this developer's own shell once, which is the
    cheapest possible demonstration.
    """
    allowed, reason = _removable(root)
    assert allowed, f'refusing to act on {root}: {reason}'
    survivors = terminate_under(root)
    assert not survivors, f'processes survived cleanup of {root}: {survivors}'
    assert not groups_under(root), f'process groups survived cleanup of {root}'
    if remove:
        shutil.rmtree(root, ignore_errors=True)


def _selftest():
    """Check the guard on both platforms' path shapes.

    `/tmp/pio-x` has two separators on Linux and three on macOS, where
    realpath prepends `/private`. Counting separators therefore passed
    locally and refused every store in CI, which is why this runs there.
    """
    import tempfile

    root = tempfile.mkdtemp(prefix='pio-selftest-', dir='/tmp')
    os.chmod(root, 0o700)
    allowed, reason = _removable(root)
    assert allowed, f'a private store under /tmp must be removable: {reason}'
    release(root)
    assert not os.path.exists(root), 'the store was not removed'

    for refused in ('/', '/tmp', '/private/tmp', '/usr', os.path.expanduser('~'), os.getcwd()):
        if not os.path.isdir(refused):
            continue
        allowed, _ = _removable(refused)
        assert not allowed, f'{refused} must never be removable'

    # A world-readable directory is not a private store.
    loose = tempfile.mkdtemp(prefix='pio-selftest-loose-', dir='/tmp')
    os.chmod(loose, 0o755)
    allowed, reason = _removable(loose)
    shutil.rmtree(loose, ignore_errors=True)
    assert not allowed, 'a world-readable directory must not be removable'
    print('case_cleanup selftest: guard behaves on this platform')


if __name__ == '__main__':
    _selftest()
