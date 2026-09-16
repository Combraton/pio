#!/usr/bin/env python3
"""Build the pinned released runner and preserve its unmodified PIO results."""
import argparse
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
        environment = {'pio_head': head, 'pio_dirty': bool(subprocess.check_output(
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
        # Preserve runner outcome classes. This additional list includes every excluded fixture.
        limits = [{'fixture': item['fixture'], 'outcome': item['outcome'], 'reason': item['reason']}
                  for item in manifest['results'] if item['outcome'] in ('unsupported', 'skipped')]
        report = {'classes': summary, 'runner_exit': result.returncode, 'coverage_limits': limits,
                  'real_adapter': False, 'note': 'Unsupported and skipped are not passes. Empty skeleton claims do not establish M1 acceptance.'}
        (out / 'pio-report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({'classes': summary, 'runner_exit': result.returncode}, indent=2))
        return result.returncode


if __name__ == '__main__':
    sys.exit(main())
