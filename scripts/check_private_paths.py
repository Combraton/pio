#!/usr/bin/env python3
"""Refuse to commit an absolute home path.

This repository is public. A receipt, an event file, a log and a test fixture
are all published, and the owner's home directory is not ours to publish — nor
is what such a path discloses. The Claude receipts carried the full filesystem
path of every installed plugin from R2 onward, and an OpenCode receipt carried
the absolute path of the out-of-fixture marker.

The rule is a shape, not a name: nothing committed may contain `/Users/<name>/`
or `/home/<name>/`. Redaction happens at the recording boundary — `~`, or a
label — and this is the gate that proves it held.

**Every committed file is scanned**, not only Markdown. Binary files are read
as bytes and searched the same way, because a path in a PNG's metadata is
still a path.

`--selftest` plants a path in a scratch tree and proves the check fails on it.
A gate nobody has seen fail is not a gate.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

# The shape. A third component is required, so a bare `/Users` is left alone.
PATTERN = re.compile(rb'/(?:Users|home)/([A-Za-z0-9._-]+)/')

# Names that are deliberately fictional and carry no one's home. Each one is
# here because a test needs a path-shaped string, not because a real path was
# found and waved through.
FICTIONAL = {
    b'someone',   # crates/pio-codex: a projects table fixture
    b'example',
    b'user',
    b'you',
    b'runner',    # GitHub Actions' own checkout path, in workflow files
}

# Files that may carry the shape for a stated reason. This script itself does,
# because it has to name what it forbids.
def redact(value, home=None):
    """Rewrite absolute home paths out of a value before it is written.

    The Python half of `pio_core::redact_home`, for the artefacts the runners
    write themselves rather than the host. Object keys are rewritten too,
    because a settings map can key on a path.
    """
    import os

    home = os.path.expanduser('~') if home is None else home
    text_pattern = re.compile(r'/(?:Users|home)/([A-Za-z0-9._-]+)/')
    fictional = {name.decode() for name in FICTIONAL}

    def scrub(text):
        out = text.replace(home, '~') if home else text
        while True:
            match = text_pattern.search(out)
            if not match or match.group(1) in fictional:
                break
            out = out[:match.start()] + '~/' + out[match.end():]
        return out

    if isinstance(value, str):
        return scrub(value)
    if isinstance(value, list):
        return [redact(item, home) for item in value]
    if isinstance(value, dict):
        return {scrub(key): redact(item, home) for key, item in value.items()}
    return value


def tracked(root):
    listing = subprocess.run(['git', '-C', str(root), 'ls-files', '-z'],
                             capture_output=True, check=True).stdout
    return [name.decode() for name in listing.split(b'\0') if name]


def scan(root, names):
    findings = []
    for name in names:
        path = root / name
        try:
            body = path.read_bytes()
        except OSError:
            continue
        for match in PATTERN.finditer(body):
            if match.group(1) in FICTIONAL:
                continue
            line = body.count(b'\n', 0, match.start()) + 1
            findings.append((name, line, match.group(0).decode('utf-8', 'replace')))
    return findings


def selftest():
    """Plant a path and prove the check fails on it."""
    import os
    import tempfile

    # Assembled, never written out whole. A literal home path in this file
    # would be a home path in a committed file — which is the thing being
    # checked — and the check used to exempt itself to live with that, which
    # is how a real account name sat in a public repository. No exemption
    # now: the name is a placeholder, and the path is built from parts so it
    # exists only while the test runs.
    home = '/' + 'Users' + '/' + 'operator' + '/'
    scratch = Path(tempfile.mkdtemp(prefix='private-path-', dir='/tmp')).resolve()
    try:
        subprocess.run(['git', '-C', str(scratch), 'init', '-q'], check=True)
        (scratch / 'clean.json').write_text('{"source": "~/.config/opencode/opencode.jsonc"}\n')
        (scratch / 'fictional.rs').write_text('let p = "/Users/someone/existing";\n')
        subprocess.run(['git', '-C', str(scratch), 'add', '-A'], check=True,
                       capture_output=True)
        assert scan(scratch, tracked(scratch)) == [], 'a clean tree was reported'

        planted = scratch / 'receipt.json'
        planted.write_text(
            '{"target": "' + home + 'pio-m3b-live/outside/marker.txt"}\n')
        subprocess.run(['git', '-C', str(scratch), 'add', '-A'], check=True,
                       capture_output=True)
        found = scan(scratch, tracked(scratch))
        assert [f[0] for f in found] == ['receipt.json'], found
        assert found[0][2] == home, found

        # And a binary file, because a path in one is still a path.
        planted.unlink()
        (scratch / 'blob.bin').write_bytes(
            b'\x00\x01' + home.encode() + b'secret/\x00')
        subprocess.run(['git', '-C', str(scratch), 'add', '-A'], check=True,
                       capture_output=True)
        found = scan(scratch, tracked(scratch))
        assert [f[0] for f in found] == ['blob.bin'], found
        print('private-path check: a planted path is caught in text and in binary, '
              'a redacted one and a fictional one are not')
    finally:
        import shutil
        shutil.rmtree(scratch, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--selftest', action='store_true')
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return
    root = args.root.resolve()
    names = tracked(root)
    findings = scan(root, names)
    if findings:
        for name, line, text in findings[:40]:
            print(f'{name}:{line}: {text}')
        raise SystemExit(
            f'{len(findings)} absolute home path(s) in {len(set(f[0] for f in findings))} '
            f'committed file(s). Redact at the recording boundary, not here.')
    print(f'private paths: {len(names)} committed files, none carries a home path')


if __name__ == '__main__':
    main()
