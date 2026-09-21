#!/usr/bin/env python3
"""Bound Cargo processes by repository capacity and canonical target identity.

Locks are flock open-file descriptions passed only to a guardian. The guardian
waits for Cargo without leaking descriptors to compiler cache daemons. Never
unlink lock files: the guardian keeps permits after outer-wrapper death.
"""
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


class CoordinationError(Exception):
    pass


def git_path(flag):
    return Path(subprocess.check_output(['git', 'rev-parse', flag], text=True).strip()).resolve()


def canonical(value):
    return Path(value).expanduser().resolve()


def digest(value):
    return hashlib.sha256(str(value).encode()).hexdigest()


def target_path(args, common, worktree):
    values = []
    before_separator = args[:args.index('--')] if '--' in args else args
    for index, arg in enumerate(before_separator):
        if arg in {'--config', '--manifest-path', '-C'} or arg.startswith(('--config=', '--manifest-path=', '-C')):
            raise CoordinationError('alternate roots/config overrides are unsupported; run from the intended worktree')
        if arg == '--target-dir':
            if index + 1 == len(before_separator):
                raise CoordinationError('--target-dir requires a path')
            values.append(canonical(before_separator[index + 1]))
        elif arg.startswith('--target-dir='):
            if not arg.partition('=')[2]:
                raise CoordinationError('--target-dir requires a path')
            values.append(canonical(arg.partition('=')[2]))
        # Inline config can otherwise override our lock's target identity.
        elif 'target-dir' in arg and (arg.startswith('--config=') or '=' in arg):
            raise CoordinationError('configure target through CARGO_TARGET_DIR or --target-dir')
    if os.environ.get('CARGO_TARGET_DIR'):
        values.append(canonical(os.environ['CARGO_TARGET_DIR']))
    if values and any(value != values[0] for value in values):
        raise CoordinationError('conflicting explicit target directories')
    return values[0] if values else common / 'vega-build' / 'targets' / digest(worktree)


def bind_target(target, worktree, state):
    """Persist cache identity while holding its target lock, including after clean."""
    owners = state / 'owners'
    owners.mkdir(exist_ok=True)
    record = owners / (digest(target) + '.json')
    expected = {'target': str(target), 'worktree': str(worktree)}
    if record.exists():
        try:
            actual = json.loads(record.read_text())
        except (ValueError, OSError) as error:
            raise CoordinationError('unreadable target ownership record') from error
        if actual != expected:
            raise CoordinationError('target belongs to a different worktree; choose a fresh target')
        return
    if target.exists() and (not target.is_dir() or any(target.iterdir())):
        raise CoordinationError('nonempty unbound target cannot be adopted; choose a fresh target')
    temporary = record.with_suffix(f'.{os.getpid()}.tmp')
    temporary.write_text(json.dumps(expected, sort_keys=True))
    os.replace(temporary, record)


def command_kind(args):
    # Unknown global options/subcommands/aliases retain test exclusion. Normal
    # toolchain overrides are supported without losing the fmt fast path.
    words = args[1:] if args and args[0].startswith('+') else args
    return words[0] if words else ''


class Lock:
    def __init__(self, path, owner):
        self.path = path
        self.owner = owner
        self.fd = None

    def acquire(self):
        fd = os.open(self.path, os.O_CREAT | os.O_RDWR, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            os.close(fd)
            if error.errno in (errno.EACCES, errno.EAGAIN):
                return False
            raise
        self.fd = fd
        payload = json.dumps(self.owner, sort_keys=True).encode()
        os.ftruncate(fd, 0)
        os.write(fd, payload)
        return True

    def close(self):
        if self.fd is not None:
            os.close(self.fd)
            self.fd = None

    def holder(self):
        try:
            return self.path.read_text()
        except OSError:
            return '(owner record unavailable)'


def status(lock_dir, release=False):
    live = False
    for path in sorted(lock_dir.glob('*.lock')):
        fd = os.open(path, os.O_RDWR)
        try:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                live = True
                print(f'HELD {path.name}: {path.read_text()}')
            else:
                # The inode is permanent. Unlinking lets another caller acquire
                # a new inode while an existing waiter still uses the old one.
                if release:
                    os.ftruncate(fd, 0)
        finally:
            os.close(fd)
    if release and live:
        raise CoordinationError('refusing to release live locks; kernel owns their lifetime')
    print('cargo-coordinate: ' + ('active owners above' if live else 'free (kernel locks released)'))


def guardian(args):
    """Own inherited permits until Cargo has terminated, independent of caller."""
    child = None
    received = None
    signal_at = None

    def forward(signum, _frame):
        nonlocal received, signal_at
        received = signum
        if signal_at is None:
            signal_at = time.monotonic()
        if child is not None:
            try:
                os.killpg(child.pid, signum)
            except ProcessLookupError:
                pass

    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, forward)
    try:
        # Do not pass advisory descriptors into Cargo: rustc wrappers can spawn
        # persistent daemons that would otherwise retain permits indefinitely.
        child = subprocess.Popen(['cargo', *args], close_fds=True, start_new_session=True)
        if received:
            forward(received, None)
        while child.poll() is None:
            if signal_at is not None and time.monotonic() - signal_at > 3:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            time.sleep(0.05)
        result = child.wait()
        if received:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        return 128 + received if received else (128 - result if result < 0 else result)
    finally:
        if child is not None and child.poll() is None:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()


def run(args, wait, common, worktree):
    kind = command_kind(args)
    if kind == 'fmt':
        return subprocess.call(['cargo', *args])
    target = target_path(args, common, worktree)
    state = common / 'vega-build'
    lock_dir = state / 'locks'
    lock_dir.mkdir(parents=True, exist_ok=True)
    owner = {'pid': os.getpid(), 'worktree': str(worktree), 'command': args,
             'target': str(target), 'started_at': time.time()}
    # Fixed repository-wide capacity cannot disagree between callers. Two Cargo
    # invocations, each at most half the host by default. Explicit jobs win.
    slots = [Lock(lock_dir / f'build-{i}.lock', owner) for i in range(2)]
    required = []
    if kind not in {'build', 'b', 'check', 'c', 'clippy', 'clean', 'doc', 'd', 'fetch', 'metadata', 'tree'}:
        required.append(Lock(lock_dir / 'tests.lock', owner))
    required.append(Lock(lock_dir / f'target-{digest(target)}.lock', owner))
    acquired = []
    queue_start = time.monotonic()
    child = None
    received = None
    signal_at = None

    def handle_signal(signum, _frame):
        nonlocal received, signal_at
        received = signum
        if signal_at is None:
            signal_at = time.monotonic()
        if child is not None:
            try:
                os.killpg(child.pid, signum)
            except ProcessLookupError:
                pass

    previous = {sig: signal.signal(sig, handle_signal) for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP)}
    printed = set()

    def acquire_one(candidates):
        while True:
            if received:
                raise CoordinationError(f'interrupted while queuing (signal {received})')
            for lock in candidates:
                if lock.acquire():
                    acquired.append(lock)
                    return
            label = ', '.join(lock.path.name for lock in candidates)
            if not wait:
                raise CoordinationError(f'busy: {label}; retry with --wait\n' + candidates[0].holder())
            if label not in printed:
                print(f'cargo-coordinate: waiting for {label}', file=sys.stderr, flush=True)
                printed.add(label)
            time.sleep(0.1)

    try:
        # Test exclusion before capacity: a waiting test never occupies a build
        # slot. Same-target ownership also precedes scarce capacity.
        for lock in required:
            acquire_one([lock])
        bind_target(target, worktree, state)
        acquire_one(slots)
        queued = time.monotonic() - queue_start
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(target)
        # Recent Cargo can place intermediate artifacts outside target-dir.
        # Pin that cache too, overriding repository/user/env build-dir config.
        env['CARGO_BUILD_BUILD_DIR'] = str(target)
        env.setdefault('CARGO_BUILD_JOBS', str(max(1, (os.cpu_count() or 2) // 2)))
        print(f'cargo-coordinate: queue_seconds={queued:.3f} target={target}', file=sys.stderr, flush=True)
        started = time.monotonic()
        child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), '--_guardian', *args], env=env, pass_fds=tuple(lock.fd for lock in acquired), start_new_session=True)
        if received:
            handle_signal(received, None)
        while child.poll() is None:
            # Guardian owns signal escalation and reaping. Killing it here would
            # release permits while its independently grouped Cargo still runs.
            time.sleep(0.05)
        result = child.wait()
        result = 128 + received if received else (128 - result if result < 0 else result)
        print(f'cargo-coordinate: execution_seconds={time.monotonic()-started:.3f} exit={result}', file=sys.stderr, flush=True)
        return result
    finally:
        if child is not None and child.poll() is None:
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            child.wait()
        for lock in reversed(acquired):
            lock.close()
        for sig, handler in previous.items():
            signal.signal(sig, handler)


def main():
    args = sys.argv[1:]
    if args and args[0] == '--_guardian':
        return guardian(args[1:])
    wait = False
    if args and args[0] in ('-h', '--help'):
        print('Usage: cargo-lock.sh [--wait] <cargo arguments> | --status | --release | --target-path')
        return 0
    if args and args[0] == '--wait':
        wait = True
        args.pop(0)
    if not args:
        raise CoordinationError('pass a cargo command, --status, --release, or --target-path')
    common, worktree = git_path('--git-common-dir'), git_path('--show-toplevel')
    if args[0] == '--target-path':
        print(target_path(args[1:], common, worktree))
        return 0
    if args[0] in ('--status', '--release'):
        if len(args) != 1:
            raise CoordinationError('status/release do not accept cargo arguments')
        status(common / 'vega-build' / 'locks', release=args[0] == '--release')
        return 0
    return run(args, wait, common, worktree)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (CoordinationError, OSError, subprocess.CalledProcessError) as error:
        print(f'cargo-coordinate: {error}', file=sys.stderr)
        sys.exit(1)
