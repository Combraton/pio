#!/usr/bin/env python3
"""Small, dependency-free documentation checks; not product verification."""
import argparse
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit


def check(root, workspace=None):
    errors = []
    checked = 0
    cross_skipped = 0
    for name in ('README.md', 'AGENTS.md', 'CLAUDE.md', 'docs/README.md', 'docs/VERIFICATION.md'):
        if not (root / name).is_file():
            errors.append(f'Missing entrypoint: {name}')
    claude = root / 'CLAUDE.md'
    if claude.exists() and not re.search(r'^@AGENTS\.md\s*$', claude.read_text(), re.M):
        errors.append('CLAUDE.md must import the local AGENTS.md')
    paths = sorted(p for p in root.rglob('*.md') if not any(part in {'.git', '.worktrees', 'node_modules', 'target', '.venv'} for part in p.relative_to(root).parts))
    for path in paths:
        content = path.read_text(encoding='utf-8')
        rel = path.relative_to(root)
        # Match ordinary fenced Markdown blocks without treating prose as executable.
        opened = None
        prose = []
        for number, line in enumerate(content.splitlines(), 1):
            fence = re.match(r'^\s{0,3}(`{3,}|~{3,})(.*)$', line)
            if fence:
                marker, rest = fence.groups()
                if opened is None:
                    opened = (marker[0], len(marker), number)
                elif marker[0] == opened[0] and len(marker) >= opened[1] and not rest.strip():
                    opened = None
                continue
            if opened is None:
                prose.append(line)
        if opened:
            errors.append(f'{rel}:{opened[2]}: unclosed code fence')
        for match in re.finditer(r'\]\(([^)]+)\)', '\n'.join(prose)):
            target = match.group(1).strip()
            if target.startswith('<') and target.endswith('>'):
                target = target[1:-1]
            parts = urlsplit(target)
            if not parts.path or parts.scheme == 'mailto':
                continue
            if parts.scheme in ('http', 'https'):
                cross = re.fullmatch(r'/Combraton/(combraton|pio|cbr|protocol|benchmarks)/blob/main/(.+)', parts.path)
                if parts.netloc == 'github.com' and cross:
                    if workspace:
                        destination = workspace / cross[1] / unquote(cross[2])
                    else:
                        cross_skipped += 1
                        continue
                else:
                    continue
            elif parts.scheme:
                errors.append(f'{rel}: unsupported local link scheme {parts.scheme}')
                continue
            else:
                destination = path.parent / unquote(parts.path)
                if parts.path.startswith('/'):
                    errors.append(f'{rel}: absolute machine path in link')
                    continue
            checked += 1
            if not destination.exists():
                errors.append(f'{rel}: missing link target {target}')
        if re.search(r'/(?:Users|Volumes)/', content):
            errors.append(f'{rel}: private machine path in published Markdown')
    return errors, len(paths), checked, cross_skipped


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, help='Optional parent containing all five clones, for cross-repository file links')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    errors, files, links, skipped = check(root, args.workspace.resolve() if args.workspace else None)
    for error in errors:
        print(error, file=sys.stderr)
    print(f'{root.name}: {files} Markdown files, {links} file links, {len(errors)} errors; {skipped} cross-repository links not checked')
    print('Checks entrypoints, local imports, ordinary file links and fences. Does not validate remote URLs, fragments, Mermaid syntax or product behavior.')
    return 1 if errors else 0


if __name__ == '__main__':
    raise SystemExit(main())
