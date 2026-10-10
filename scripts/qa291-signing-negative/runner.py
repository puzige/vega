import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

FIXTURE = Path(__file__).resolve().parent
REPOSITORY = FIXTURE.parent.parent
ERRORS = {
    'missing': 'update signing key is missing',
    'wrong-public': 'update signing key does not match repository public key',
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_sources():
    freeze = json.loads((FIXTURE / 'freeze.json').read_text())
    for relative, expected in freeze['files'].items():
        if digest(REPOSITORY / relative) != expected:
            raise RuntimeError(f'Frozen source changed: {relative}')
    production = tomllib.loads((REPOSITORY / 'Cargo.lock').read_text())['package']
    candidate = tomllib.loads((FIXTURE / 'Cargo.lock').read_text())['package']
    identities = {(item['name'], item['version'], item.get('source'), item.get('checksum'))
                  for item in production}
    for item in candidate:
        if item['name'] == 'qa291-signing-negative' and 'source' not in item:
            continue
        identity = (item['name'], item['version'], item.get('source'), item.get('checksum'))
        if identity not in identities:
            raise RuntimeError(f'Dependency differs from production lock: {item["name"]}')
    return freeze


def owned_root(path):
    root = path.absolute()
    runner_temp = Path(os.environ['RUNNER_TEMP']).resolve()
    if root.resolve() != root or not root.is_relative_to(runner_temp) or root == runner_temp:
        raise RuntimeError('Evidence must be an owned directory under RUNNER_TEMP')
    return root


def child_environment(build=False):
    allowed = ('PATH', 'TMPDIR')
    if build:
        allowed += ('HOME', 'CARGO_HOME', 'RUSTUP_HOME', 'SDKROOT', 'MACOSX_DEPLOYMENT_TARGET')
    return {name: os.environ[name] for name in allowed if name in os.environ}


def write_json(path, value):
    with path.open('x') as output:
        json.dump(value, output, indent=2, sort_keys=True)
        output.write('\n')


def build(root, offline):
    freeze = verify_sources()
    for relative in ('xtask/src/sign_update.rs', 'assets/update-public-key.hex', 'Cargo.lock'):
        print(f'Frozen {relative} SHA256={freeze["files"][relative]}', flush=True)
    root.mkdir(parents=True, exist_ok=True)
    write_json(root / 'build-reservation.json', {'freeze': freeze})
    commands = []
    if not offline:
        commands.append(['cargo', 'fetch', '--locked', '--manifest-path', str(FIXTURE / 'Cargo.toml')])
    commands.append(['cargo', 'build', '--offline', '--locked', '--manifest-path',
                     str(FIXTURE / 'Cargo.toml'), '--target-dir', str(root / 'target')])
    for index, command in enumerate(commands):
        result = subprocess.run(command, cwd=root, env=child_environment(build=True),
                                capture_output=True, timeout=900)
        stdout = root / f'build-{index}.stdout'
        stderr = root / f'build-{index}.stderr'
        stdout.write_bytes(result.stdout)
        stderr.write_bytes(result.stderr)
        write_json(root / f'build-{index}.json', {
            'command': command, 'exit_code': result.returncode,
            'stdout_sha256': digest(stdout), 'stderr_sha256': digest(stderr),
        })
        print(result.stderr.decode(), end='')
        if result.returncode:
            return result.returncode
    verify_sources()
    write_json(root / 'build-complete.json', {
        'status': 'COMPILED_ONLY', 'commands': len(commands),
        'binary_sha256': digest(root / 'target/debug/qa291-signing-negative'),
    })
    print('COMPILED_ONLY: no signing cases executed')
    return 0


def case(root, name):
    verify_sources()
    if not (root / 'build-complete.json').is_file():
        raise RuntimeError('Compiled evidence is missing')
    compiled = json.loads((root / 'build-complete.json').read_text())
    if digest(root / 'target/debug/qa291-signing-negative') != compiled['binary_sha256']:
        raise RuntimeError('Compiled executable changed')
    case_root = root / name
    case_root.mkdir()
    owned = case_root / 'owned'
    owned.mkdir()
    command = [str(root / 'target/debug/qa291-signing-negative'), name, str(owned)]
    result = subprocess.run(command, cwd=owned, env=child_environment(),
                            capture_output=True, timeout=15)
    stdout = case_root / 'stdout'
    stderr = case_root / 'stderr'
    stdout.write_bytes(result.stdout)
    stderr.write_bytes(result.stderr)
    write_json(case_root / 'result.json', {
        'case': name, 'command': command, 'exit_code': result.returncode,
        'stdout_sha256': digest(stdout), 'stderr_sha256': digest(stderr),
    })
    print(result.stdout.decode(), end='')
    print(result.stderr.decode(), end='', file=sys.stderr)
    return result.returncode


def verify(root):
    verify_sources()
    for name, expected_error in ERRORS.items():
        case_root = root / name
        result = json.loads((case_root / 'result.json').read_text())
        stdout = case_root / 'stdout'
        stderr = case_root / 'stderr'
        outcome = json.loads(stdout.read_text())
        expected = {'case': name, 'error': expected_error,
                    'workspace_root_calls': 0, 'owned_entries': 0}
        if result['exit_code'] != 1 or outcome != expected:
            raise RuntimeError(f'Production rejection assertion failed: {name}')
        if stderr.read_text() != f'Error: {expected_error}\n':
            raise RuntimeError(f'Unexpected stderr: {name}')
        if digest(stdout) != result['stdout_sha256'] or digest(stderr) != result['stderr_sha256']:
            raise RuntimeError(f'Raw result changed: {name}')
        if list((case_root / 'owned').iterdir()):
            raise RuntimeError(f'Unexpected owned output: {name}')
        print(f'{name}: PASS; production exit=1; workspace_root=0; output=0; raw hashes verified')
        print(json.dumps({key: result[key] for key in
                          ('case', 'exit_code', 'stdout_sha256', 'stderr_sha256')}, sort_keys=True))
    write_json(root / 'verified.json', {'status': 'PASS', 'cases': list(ERRORS)})
    return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=('build', 'case', 'verify'))
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--case', choices=tuple(ERRORS))
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    root = owned_root(args.evidence)
    if args.mode == 'build':
        return build(root, args.offline)
    if args.mode == 'case':
        if args.case is None:
            raise RuntimeError('Case selection is required')
        return case(root, args.case)
    return verify(root)


if __name__ == '__main__':
    sys.exit(main())
