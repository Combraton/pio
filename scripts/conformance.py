#!/usr/bin/env python3
"""Build the pinned released runner and preserve its unmodified PIO results."""
import argparse
import hashlib
from collections import Counter
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile

from verify_protocol_pin import verify

ROOT = Path(__file__).resolve().parents[1]


def run(argv, **kwargs):
    print('+ ' + ' '.join(map(str, argv)), flush=True)
    return subprocess.run(argv, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, help='Optional already downloaded assets; still verified')
    parser.add_argument('--out', type=Path, default=ROOT / 'target/conformance')
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        raise SystemExit('Output directory already exists; choose a fresh --out to preserve evidence')
    out.mkdir(parents=True)
    pin = json.loads((ROOT / 'protocol.lock.json').read_text())
    with tempfile.TemporaryDirectory(prefix='pio-conformance-') as directory:
        work = Path(directory)
        assets = args.assets.resolve() if args.assets else work / 'assets'
        if not args.assets:
            assets.mkdir()
            run(['gh', 'release', 'download', pin['tag'], '--repo', 'Combraton/protocol',
                 '--dir', str(assets)])
        receipt = verify(assets, pin)
        (out / 'pin-verification.json').write_text(json.dumps(receipt, indent=2) + '\n')
        with tarfile.open(assets / 'combraton-protocol-0.1.0-source.tar.gz') as archive:
            archive.extractall(work / 'source', filter='data')
        source = work / 'source/combraton-protocol-0.1.0'
        # These are unchanged contract schemas, not a fork of Protocol's provider.
        vendor = ROOT / 'crates/pio-protocol/vendor'
        schema_files = sorted(vendor.rglob('*.json'))
        for path in schema_files:
            released = source / path.relative_to(vendor)
            if path.read_bytes() != released.read_bytes():
                raise SystemExit(f'Vendored Protocol schema differs from pinned archive: {path.relative_to(vendor)}')
        (out / 'schema-verification.json').write_text(json.dumps({
            'source_commit': pin['source_commit'], 'unchanged_schema_files': len(schema_files)
        }, indent=2) + '\n')
        build = ROOT / 'target/protocol-runner'
        env = {**os.environ, 'CARGO_TARGET_DIR': str(build)}
        run(['cargo', 'build', '--locked', '-p', 'combraton-conformance',
             '--manifest-path', str(source / 'Cargo.toml')], env=env)
        binary = build / 'debug/combraton-conformance'
        run([str(binary), 'self-test', '--repo', str(source)])
        run([str(binary), 'check-fixtures', '--repo', str(source)])
        participant = json.loads((ROOT / 'conformance/participant.json').read_text())
        # {repo} belongs to Protocol; do not accidentally launch its reference provider.
        participant['launch']['argv'] = [
            arg.replace('{pio_binary}', str(ROOT / 'target/debug/pio'))
            for arg in participant['launch']['argv']]
        descriptor = out / 'participant.json'
        descriptor.write_text(json.dumps(participant, indent=2) + '\n')
        shutil.copyfile(ROOT / 'conformance/m1-target.json', out / 'm1-target.json')
        head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        pio_digest = hashlib.sha256((ROOT / 'target/debug/pio').read_bytes()).hexdigest()
        environment = {'pio_binary_sha256': pio_digest, 'pio_head': head, 'pio_dirty': bool(subprocess.check_output(
            ['git', 'status', '--porcelain'], cwd=ROOT)), 'os': platform.system(),
            'arch': platform.machine(), 'rustc': subprocess.check_output(
                ['rustc', '--version'], text=True).strip(), 'execution_source': 'fake-host',
            'real_adapter': False}
        (out / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
        result = subprocess.run([str(binary), 'run', '--repo', str(source),
                                 '--participant', str(descriptor), '--out', str(out)])
        if not (out / 'manifest.json').is_file():
            return result.returncode or 1
        manifest = json.loads((out / 'manifest.json').read_text())
        summary = dict(Counter(item['outcome'] for item in manifest['results']))
        fixture_directories = {
            json.loads(path.read_text())['id']: path.relative_to(source / 'conformance/fixtures').parts[0]
            for path in (source / 'conformance/fixtures').rglob('*.json')
        }
        by_directory = {}
        for item in manifest['results']:
            directory = fixture_directories[item['fixture']]
            counts = by_directory.setdefault(directory, Counter())
            counts[item['outcome']] += 1
        # Preserve runner outcome classes. This additional list includes every excluded fixture.
        limits = [{'fixture': item['fixture'], 'outcome': item['outcome'], 'reason': item['reason']}
                  for item in manifest['results'] if item['outcome'] in ('unsupported', 'skipped')]
        report = {'classes': summary, 'by_directory': by_directory,
                  'runner_exit': result.returncode, 'coverage_limits': limits,
                  'real_adapter': False, 'note': 'Unsupported and skipped are not passes. This is a journal-backed fake Execution checkpoint, not M1 acceptance or a real adapter. Supplemental PIO fixtures are counted separately.'}
        (out / 'pio-report.json').write_text(json.dumps(report, indent=2) + '\n')
        supplemental = subprocess.run([str(binary), 'run', '--repo', str(source),
                                       '--participant', str(descriptor), '--fixtures',
                                       str(ROOT / 'conformance/regressions'),
                                       '--out', str(out / 'pio-regressions')])
        if (out / 'pio-regressions/manifest.json').is_file():
            extra = json.loads((out / 'pio-regressions/manifest.json').read_text())
            report['supplemental_pio_classes'] = dict(Counter(item['outcome'] for item in extra['results']))
        report['supplemental_runner_exit'] = supplemental.returncode
        (out / 'pio-report.json').write_text(json.dumps(report, indent=2) + '\n')
        if hashlib.sha256((ROOT / 'target/debug/pio').read_bytes()).hexdigest() != pio_digest:
            raise SystemExit('PIO binary changed during verification; results are not a single-build receipt')
        print(json.dumps({'classes': summary, 'runner_exit': result.returncode,
                          'supplemental_pio_classes': report.get('supplemental_pio_classes')}, indent=2))
        return result.returncode or supplemental.returncode


if __name__ == '__main__':
    sys.exit(main())
