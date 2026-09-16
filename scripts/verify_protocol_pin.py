#!/usr/bin/env python3
"""Verify downloaded Protocol release assets; does not run PIO or conformance."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(assets, pin):
    for name, expected in pin['assets'].items():
        require(digest(assets / name) == expected, f'asset checksum mismatch: {name}')
    listed = {}
    for line in (assets / 'SHA256SUMS').read_text().splitlines():
        expected, name = line.split(None, 1)
        require(name not in listed, f'duplicate asset: {name}')
        listed[name] = expected
    require(listed == {k: v for k, v in pin['assets'].items() if k != 'SHA256SUMS'},
            'SHA256SUMS differs from pinned asset set')
    manifest = json.loads((assets / 'release-manifest.json').read_text())
    for key in ('tag', 'source_commit', 'source_tree'):
        require(manifest[key] == pin[key], f'manifest {key} mismatch')
    require(manifest['tested_head_tree'] == pin['source_tree'], 'tested tree mismatch')
    require(manifest['normative_inventory']['listing_sha256'] == pin['inventory_listing_sha256'],
            'inventory listing mismatch')
    require(manifest['bundle']['sha256'] == pin['assets'][manifest['bundle']['asset']],
            'manifest bundle checksum mismatch')
    with tempfile.TemporaryDirectory(prefix='pio-protocol-verify-') as work:
        base = Path(work)
        with tarfile.open(assets / manifest['bundle']['asset']) as archive:
            archive.extractall(base, filter='data')
        root = (base / 'combraton-protocol-0.1.0').resolve()
        checksums = root / 'BUNDLE-SHA256SUMS'
        require(checksums.read_bytes() == (assets / manifest['bundle']['file_checksums']).read_bytes(),
                'internal/external bundle lists differ')
        count = 0
        for line in checksums.read_text().splitlines():
            expected, name = line.split(None, 1)
            path = (root / name).resolve()
            require(path.is_relative_to(root), f'unsafe bundle path: {name}')
            require(digest(path) == expected, f'bundle checksum mismatch: {name}')
            count += 1
        # The released verifier is executed only after all its bundle bytes were checked.
        result = subprocess.run([sys.executable, 'scripts/release_inventory.py', '--verify'],
                                cwd=root, capture_output=True, text=True, timeout=60)
        require(result.returncode == 0, 'released inventory check failed: ' + result.stdout + result.stderr)
        inventory = json.loads((root / manifest['normative_inventory']['path']).read_text())
        require(len(inventory['files']) == manifest['normative_inventory']['files'],
                'inventory file count mismatch')
        return {'result': 'pass', 'scope': 'release integrity only',
                'tag': pin['tag'], 'source_commit': pin['source_commit'],
                'assets_verified': len(pin['assets']), 'bundle_files_verified': count,
                'normative_files_verified': len(inventory['files']),
                'inventory_listing_sha256': pin['inventory_listing_sha256'],
                'released_inventory_exit': result.returncode}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True, help='Downloaded release asset directory')
    args = parser.parse_args()
    pin = json.loads((Path(__file__).resolve().parents[1] / 'protocol.lock.json').read_text())
    try:
        print(json.dumps(verify(args.assets.resolve(), pin), indent=2))
    except (OSError, ValueError, KeyError, tarfile.TarError, subprocess.SubprocessError) as error:
        print(f'Protocol pin verification failed: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
