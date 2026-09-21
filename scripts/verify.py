#!/usr/bin/env python3
"""Scoped local verification; successful, content-bound evidence is reusable."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import uuid
import unittest


class VerificationError(Exception):
    pass


def run(args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, stderr=subprocess.PIPE)


def git(*args):
    return run(['git', *args]).decode().strip()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def file_identity(path):
    if path.is_symlink():
        target = path.resolve()
        if target.is_dir():
            root = Path.cwd().resolve()
            if not target.is_relative_to(root):
                raise VerificationError(f'External directory source symlink: {path}')
            known = set(source_files())
            contents = {}
            for child in sorted(target.rglob('*')):
                if child.is_symlink():
                    # Nested links can hide cycles or ignored external inputs.
                    raise VerificationError(f'Nested source directory symlink: {child}')
                if child.is_file():
                    relative = str(child.relative_to(root))
                    if relative not in known:
                        raise VerificationError(f'Unfingerprinted source symlink input: {relative}')
                    contents[relative] = file_identity(child)
            return ['directory-symlink', os.readlink(path), contents]
        if not target.is_file():
            raise VerificationError(f'Unsupported dangling source symlink: {path}')
        return ['symlink', os.readlink(path), digest(target.read_bytes())]
    if not path.exists():
        return ['missing']
    if not path.is_file():
        raise VerificationError(f'Unsupported source input: {path}; submodules require separate verification')
    return [path.stat().st_mode & 0o777, digest(path.read_bytes())]


def source_files():
    return sorted(set(run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard']).decode().split('\0')) - {''})


def scope(base, full):
    changes = set(run(['git', 'diff', '--name-only', '-z', base]).decode().split('\0'))
    changes.update(run(['git', 'ls-files', '--others', '--exclude-standard', '-z']).decode().split('\0'))
    changes.discard('')
    metadata = json.loads(run(['cargo', 'metadata', '--no-deps', '--format-version=1', '--locked']))
    members = set(metadata['workspace_members'])
    packages = {p['name']: p for p in metadata['packages'] if p['id'] in members}
    roots = {n: Path(p['manifest_path']).parent.resolve() for n, p in packages.items()}
    selected, unknown = set(), []
    tooling = False
    for name in sorted(changes):
        path = Path(name)
        owners = [n for n, root in roots.items() if path.resolve().is_relative_to(root)]
        if owners:
            selected.update(owners)
        elif name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain', 'rust-toolchain.toml', '.gitmodules') or name.startswith('.cargo/'):
            unknown.append(name)
        elif name in ('scripts/verify.py', 'scripts/cargo-lock.sh', 'scripts/cargo-coordinate.py', 'scripts/cargo-share-target.sh', '.githooks/pre-push', '.githooks/pre-commit') or name.startswith('scripts/tests/'):
            tooling = True
        elif (len(path.parts) == 1 and path.suffix == '.md') or name.startswith(('docs/', '.agents/skills/')) or name in ('.gitignore', 'LICENSE'):
            # Documentation directories must not hide executable/build inputs.
            if path.suffix not in ('.md', '.txt', '.png', '.jpg', '.svg') and name not in ('.gitignore', 'LICENSE'):
                unknown.append(name)
        else:
            unknown.append(name)
    if unknown and not full:
        raise VerificationError('Scope requires explicit --full for: ' + ', '.join(unknown))
    if full:
        selected = set(packages)
    # All dependency kinds and target conditions conservatively count.
    while True:
        consumers = {n for n, p in packages.items() for d in p['dependencies']
                     if d.get('path') and any(Path(d['path']).resolve() == roots[s] for s in selected)}
        expanded = selected | consumers
        if expanded == selected:
            break
        selected = expanded
    commands = [['git', 'diff', '--check', base], ['scripts/cargo-lock.sh', '--wait', 'fmt', '--all', '--', '--check']]
    if tooling or full:
        commands += [[sys.executable, '-B', 'scripts/verify.py', '--syntax'],
                     [sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-p', 'test_*.py', '-v']]
    flags = [arg for name in sorted(selected) for arg in ('-p', name)]
    if flags:
        commands += [['scripts/cargo-lock.sh', '--wait', 'clippy', *flags, '--all-targets', '--', '-D', 'warnings'],
                     ['scripts/cargo-lock.sh', '--wait', 'test', *flags]]
    return {'packages': sorted(selected), 'tooling': tooling or full, 'full': full, 'commands': commands}


def identity(base, plan):
    # Keep only digests of environment values: credentials must not reach evidence.
    ignored = {'PWD', 'OLDPWD', 'SHLVL', '_', 'GIT_EXEC_PATH', 'GIT_PREFIX'} | set(git('rev-parse', '--local-env-vars').splitlines())
    env = {k: v for k, v in os.environ.items() if k not in ignored}
    return {'schema': 1, 'base': base, 'plan': plan,
            'source': digest(encoded({p: file_identity(Path(p)) for p in source_files()})),
            'environment': {key: digest(value.encode()) for key, value in env.items()},
            'tools': {cmd: run(cmd.split()).decode().strip() for cmd in ('cargo --version', 'rustc -vV', 'git --version')},
            'python': sys.version, 'platform': platform.platform(),
            'external_config': {str(p): file_identity(p) for root in (Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo'))), *(parent / '.cargo' for parent in (Path.cwd(), *Path.cwd().parents))) for p in (root / 'config', root / 'config.toml')},
            'verifier': digest(Path(__file__).read_bytes())}


def valid_evidence(folder, expected):
    try:
        data = json.loads((folder / 'result.json').read_text())
        if data['identity'] != expected or not data['success'] or len(data['steps']) != len(expected['plan']['commands']):
            return False
        for i, step in enumerate(data['steps']):
            if step['exit_code'] != 0 or step['command'] != expected['plan']['commands'][i]:
                return False
            if digest((folder / f'{i}.log').read_bytes()) != step['log_sha256']:
                return False
        return True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def syntax():
    if unittest.defaultTestLoader.discover('scripts/tests', pattern='test_*.py').countTestCases() == 0:
        raise VerificationError('Tooling regression discovery returned zero tests')
    for path in Path('scripts').rglob('*.py'):
        compile(path.read_bytes(), str(path), 'exec')
    for path in [*Path('scripts').glob('*.sh'), *Path('.githooks').glob('*')]:
        if path.is_file():
            subprocess.run(['sh', '-n', str(path)], check=True)


def push_head():
    records = [line.split() for line in sys.stdin if line.strip()]
    if any(len(r) != 4 for r in records):
        raise VerificationError('Malformed pre-push ref input')
    updates = [r for r in records if set(r[1]) != {'0'}]
    if not updates:
        return False
    if len(updates) != 1 or updates[0][1] != git('rev-parse', 'HEAD') or not (updates[0][0] == 'HEAD' or updates[0][0].startswith('refs/heads/')):
        raise VerificationError('Only a single current-HEAD branch push is supported; verify/push other refs separately')
    if git('status', '--porcelain', '--untracked-files=all'):
        raise VerificationError('Push requires clean current HEAD, including nonignored untracked files')
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', default='origin/master')
    parser.add_argument('--full', action='store_true')
    parser.add_argument('--plan', action='store_true')
    parser.add_argument('--pre-push', action='store_true')
    parser.add_argument('--syntax', action='store_true')
    args = parser.parse_args()
    os.chdir(git('rev-parse', '--show-toplevel'))
    rustup_bin = str(Path.home() / '.cargo/bin')
    git_exec = git('--exec-path')
    path_parts = [p for p in os.environ.get('PATH', '').split(':') if p not in (rustup_bin, git_exec)]
    os.environ['PATH'] = ':'.join([rustup_bin, *path_parts])
    if args.syntax:
        syntax()
        return 0
    if args.pre_push and not push_head():
        print('verify: ref deletion only; no build required')
        return 0
    base = git('merge-base', 'HEAD', args.base)
    common = Path(git('rev-parse', '--git-common-dir')).resolve()
    evidence = common / 'vega-verification'
    try:
        plan = scope(base, args.full)
    except VerificationError:
        if not args.pre_push:
            raise
        # Explicit --full may have already produced matching evidence. Never
        # silently launch a full workspace run from the push hook.
        plan = scope(base, True)
        expected = identity(base, plan)
        for folder in evidence.glob('*'):
            if valid_evidence(folder, expected):
                print(f'verify: reused explicit full evidence {folder.name}')
                return 0
        raise VerificationError('Run scripts/verify.py --full explicitly before pushing these shared/unknown inputs')
    if args.plan:
        print(json.dumps(plan, indent=2))
        return 0
    before = identity(base, plan)
    for folder in evidence.glob('*'):
        if valid_evidence(folder, before):
            print(f'verify: reused successful evidence {folder.name}')
            return 0
    folder = evidence / (time.strftime('%Y%m%dT%H%M%S') + '-' + uuid.uuid4().hex[:8])
    folder.mkdir(parents=True)
    result = {'identity': before, 'success': False, 'steps': [], 'started_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}
    try:
        for i, command in enumerate(plan['commands']):
            print('verify: ' + ' '.join(command), flush=True)
            start = time.monotonic()
            with (folder / f'{i}.log').open('wb') as log:
                code = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT).returncode
            raw = (folder / f'{i}.log').read_bytes()
            sys.stdout.buffer.write(raw)
            sys.stdout.flush()
            result['steps'].append({'command': command, 'exit_code': code,
                                    'elapsed_seconds': round(time.monotonic() - start, 3), 'log_sha256': digest(raw)})
            if code:
                raise VerificationError(f'Command failed ({code}); retained evidence: {folder}')
        if identity(base, scope(base, args.full)) != before:
            raise VerificationError('Source/environment/scope changed during verification; evidence rejected')
        result['success'] = True
    finally:
        (folder / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(f'verify: passed; evidence {folder}')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (VerificationError, subprocess.CalledProcessError, OSError) as error:
        print(f'verify: {error}', file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            sys.stderr.buffer.write(error.stderr)
        sys.exit(1)
