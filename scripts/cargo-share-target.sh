#!/bin/sh
# Legacy compatibility: inspect only, never delete caches or connect targets.
set -eu
case "${1:-}" in
  -h|--help)
    printf '%s\n' 'Usage: cargo-share-target.sh [--status|--unshare] [repo]' 'Read-only migration guidance; no target directories are changed.'
    exit 0 ;;
  --status|--unshare) shift ;;
esac
repo=${1:-.}
python3 - "$repo" <<'PY'
import hashlib
from pathlib import Path
import subprocess
import sys
repo = Path(sys.argv[1]).resolve()
common = Path(subprocess.check_output(['git', '-C', str(repo), 'rev-parse', '--git-common-dir'], text=True).strip())
if not common.is_absolute():
    common = repo / common
common = common.resolve()
raw = subprocess.check_output(['git', '-C', str(repo), 'worktree', 'list', '--porcelain', '-z'])
print('Read-only migration: legacy target directories and symlinks are retained unchanged.')
for field in raw.split(b'\0'):
    if not field.startswith(b'worktree '):
        continue
    worktree = Path(field[9:].decode()).resolve()
    target = worktree / 'target'
    identity = hashlib.sha256(str(worktree).encode()).hexdigest()
    print(f'worktree: {worktree}')
    print(f'  legacy target: {target.resolve() if target.exists() or target.is_symlink() else "absent"}')
    print(f'  managed target: {common / "vega-build" / "targets" / identity}')
print('Use scripts/cargo-lock.sh for builds/tests and --target-path for artifacts.')
print('Independent worktree caches are cold initially. Direct cargo bypasses coordination.')
print('No cache cleanup is automatic; only remove task-owned inactive targets after retaining evidence.')
PY
