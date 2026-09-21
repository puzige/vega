"""Production wrapper E2E; only contention/signal cases use executable fixtures."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]


class CoordinatorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='vega-coordinator-')
        self.root = Path(self.temp.name)
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        self.command(['git', 'init', '-q', str(self.repo)], cwd=self.root)
        self.command(['git', 'config', 'user.email', 'test@example.invalid'])
        self.command(['git', 'config', 'user.name', 'Test'])
        (self.repo / 'scripts').mkdir()
        for name in ('cargo-coordinate.py', 'cargo-lock.sh', 'cargo-share-target.sh'):
            shutil.copy2(SCRIPTS / name, self.repo / 'scripts' / name)
        self.command(['git', 'add', '.'])
        self.command(['git', 'commit', '-qm', 'fixture'])
        self.children = []
        self.env = os.environ.copy()
        for key in ('CARGO_TARGET_DIR', 'CARGO_BUILD_BUILD_DIR'):
            self.env.pop(key, None)

    def tearDown(self):
        for child in self.children:
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=6)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
            if child.stdout:
                child.stdout.close()
            if child.stderr:
                child.stderr.close()
        self.temp.cleanup()

    def command(self, args, cwd=None, **kwargs):
        return subprocess.run(args, cwd=cwd or self.repo, check=True, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)

    def invoke(self, *args, repo=None, env=None, check=False):
        repo = repo or self.repo
        return subprocess.run([str(repo / 'scripts/cargo-lock.sh'), *args], cwd=repo,
                              env=env or self.env, text=True, capture_output=True, check=check)

    def start(self, *args, repo=None, env=None):
        repo = repo or self.repo
        child = subprocess.Popen([str(repo / 'scripts/cargo-lock.sh'), *args], cwd=repo,
                                 env=env or self.env, text=True,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.children.append(child)
        return child

    def until(self, predicate, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(0.02)
        self.fail('condition timed out')

    def fixture_env(self, name):
        bin_dir = self.root / 'bin'
        bin_dir.mkdir(exist_ok=True)
        fixture = bin_dir / 'cargo'
        fixture.write_text('''#!/usr/bin/env python3
import json,os,pathlib,signal,sys,time,subprocess
p=pathlib.Path(os.environ['FIXTURE_MARK'])
if sys.argv[1]=='fmt': sys.exit(0)
if os.environ.get('SPAWN_DAEMON'):
 d=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'],close_fds=False,start_new_session=True,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 pathlib.Path(str(p)+'.daemon').write_text(str(d.pid))
p.write_text(json.dumps({'pid':os.getpid(),'target':os.environ['CARGO_TARGET_DIR'],'build':os.environ['CARGO_BUILD_BUILD_DIR']}))
if os.environ.get('IGNORE_TERM'): signal.signal(signal.SIGTERM,signal.SIG_IGN)
while not pathlib.Path(str(p)+'.release').exists(): time.sleep(.02)
sys.exit(int(os.environ.get('FIXTURE_EXIT','0')))
''')
        fixture.chmod(0o755)
        return dict(self.env, PATH=str(bin_dir) + os.pathsep + self.env['PATH'],
                    FIXTURE_MARK=str(self.root / name))

    def release(self, name):
        (self.root / (name + '.release')).touch()

    def test_real_worktrees_compile_concurrently_without_artifact_mix(self):
        (self.repo / 'Cargo.toml').write_text('[package]\nname="tiny"\nversion="0.1.0"\nedition="2021"\n')
        (self.repo / 'src').mkdir()
        (self.repo / 'src/main.rs').write_text('fn main() { println!("A"); }')
        (self.repo / 'build.rs').write_text('''use std::{fs,env,time::{SystemTime,UNIX_EPOCH,Duration},thread};
fn main(){let p=env::var("BUILD_MARK").unwrap();let now=||SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();fs::write(format!("{}.start",p),now().to_string()).unwrap();thread::sleep(Duration::from_secs(1));fs::write(format!("{}.end",p),now().to_string()).unwrap();}''')
        self.command(['git', 'add', '.'])
        self.command(['git', 'commit', '-qm', 'tiny real cargo crate'])
        peer = self.root / 'peer'
        self.command(['git', 'worktree', 'add', '-qb', 'peer', str(peer)])
        (peer / 'src/main.rs').write_text('fn main() { println!("B"); }')
        starts = []
        for label, repo in (('a', self.repo), ('b', peer)):
            env = dict(self.env, BUILD_MARK=str(self.root / label))
            starts.append(self.start('--wait', 'build', '--offline', repo=repo, env=env))
        for child in starts:
            stdout, stderr = child.communicate(timeout=60)
            self.assertEqual(child.returncode, 0, stdout + stderr)
        intervals = [[float((self.root / (label + suffix)).read_text()) for suffix in ('.start', '.end')] for label in ('a', 'b')]
        self.assertLess(max(interval[0] for interval in intervals), min(interval[1] for interval in intervals))
        targets = [self.invoke('--target-path', repo=repo, check=True).stdout.strip() for repo in (self.repo, peer)]
        self.assertNotEqual(*targets)
        for target, value in zip(targets, ('A', 'B')):
            self.assertEqual(self.command([str(Path(target) / 'debug/tiny')]).stdout.strip(), value)

    def test_explicit_target_alias_serializes_fast_fail_and_wait(self):
        target = self.root / 'target'
        target.mkdir()
        alias = self.root / 'alias'
        alias.symlink_to(target, target_is_directory=True)
        first = self.start('build', '--target-dir', str(target), env=self.fixture_env('first'))
        self.until(lambda: (self.root / 'first').exists())
        self.assertNotEqual(self.invoke('build', '--target-dir', str(alias), env=self.fixture_env('second')).returncode, 0)
        second = self.start('--wait', 'build', '--target-dir', str(alias), env=self.fixture_env('second'))
        time.sleep(.15)
        self.assertFalse((self.root / 'second').exists())
        self.release('first')
        self.until(lambda: (self.root / 'second').exists())
        self.release('second')
        self.assertEqual(first.wait(timeout=5), 0)
        self.assertEqual(second.wait(timeout=5), 0)

    def test_test_waiter_does_not_take_capacity_and_fmt_bypasses(self):
        a = self.start('test', '--target-dir', str(self.root/'a'), env=self.fixture_env('a'))
        self.until(lambda: (self.root/'a').exists())
        b = self.start('--wait', 'test', '--target-dir', str(self.root/'b'), env=self.fixture_env('b'))
        self.assertEqual(self.invoke('fmt', env=self.fixture_env('fmt')).returncode, 0)
        c = self.start('build', '--target-dir', str(self.root/'c'), env=self.fixture_env('c'))
        self.until(lambda: (self.root/'c').exists())
        self.assertFalse((self.root/'b').exists())
        third = self.invoke('build', '--target-dir', str(self.root/'d'), env=self.fixture_env('d'))
        self.assertNotEqual(third.returncode, 0)
        self.assertFalse((self.root/'d').exists())
        self.release('a')
        self.until(lambda: (self.root/'b').exists())
        self.release('b')
        self.release('c')
        for child in (a,b,c): self.assertEqual(child.wait(timeout=5), 0)

    def test_sigkill_wrapper_keeps_child_locks_and_release_cannot_break(self):
        env = self.fixture_env('held')
        child = self.start('build', env=env)
        self.until(lambda: (self.root/'held').exists())
        child.kill()
        child.wait(timeout=5)
        try:
            self.assertIn('HELD', self.invoke('--status').stdout)
            self.assertNotEqual(self.invoke('--release').returncode, 0)
            self.assertNotEqual(self.invoke('build', env=self.fixture_env('blocked')).returncode, 0)
        finally:
            self.release('held')
        self.until(lambda: 'free' in self.invoke('--status').stdout)
        self.assertEqual(self.invoke('--release').returncode, 0)

    def test_signal_terminates_stubborn_child_before_permit_release(self):
        env = dict(self.fixture_env('term'), IGNORE_TERM='1')
        child = self.start('build', env=env)
        self.until(lambda: (self.root/'term').exists())
        child.terminate()
        time.sleep(.2)
        self.assertIn('HELD', self.invoke('--status').stdout)
        self.assertEqual(child.wait(timeout=8), 128 + signal.SIGTERM)
        self.assertIn('free', self.invoke('--status').stdout)

    def test_exit_status_and_intermediate_cache_config_are_preserved(self):
        env = dict(self.fixture_env('fail'), FIXTURE_EXIT='7', CARGO_BUILD_BUILD_DIR=str(self.root/'wrong'))
        self.release('fail')
        result = self.invoke('build', env=env)
        self.assertEqual(result.returncode, 7)
        mark = json.loads((self.root/'fail').read_text())
        self.assertEqual(mark['target'], mark['build'])
        for arg in ('--config=build.target-dir="wrong"', '--manifest-path=else/Cargo.toml', '-Celse'):
            self.assertNotEqual(self.invoke('build', arg, env=env).returncode, 0)

    def test_compiler_daemon_cannot_retain_guardian_lock_descriptors(self):
        env = dict(self.fixture_env('daemon'), SPAWN_DAEMON='1')
        self.release('daemon')
        daemon = None
        try:
            self.assertEqual(self.invoke('clippy', env=env).returncode, 0)
            daemon = int((self.root/'daemon.daemon').read_text())
            os.kill(daemon, 0)
            self.assertIn('free', self.invoke('--status').stdout)
            self.release('next')
            self.assertEqual(self.invoke('test', env=self.fixture_env('next')).returncode, 0)
        finally:
            if daemon is not None:
                try:
                    os.kill(daemon, signal.SIGTERM)
                except ProcessLookupError:
                    pass

    def test_cache_binding_rejects_divergent_worktree_and_legacy_cache(self):
        target = self.root / 'bound'
        self.release('owner')
        self.assertEqual(self.invoke('build', '--target-dir', str(target), env=self.fixture_env('owner')).returncode, 0)
        peer = self.root / 'peer'
        self.command(['git', 'worktree', 'add', '-qb', 'peer', str(peer)])
        result = self.invoke('build', '--target-dir', str(target), repo=peer, env=self.fixture_env('peer'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('different worktree', result.stderr)
        legacy = self.root / 'legacy-cache'
        legacy.mkdir()
        (legacy / 'sentinel').write_text('old artifacts')
        result = self.invoke('build', '--target-dir', str(legacy), env=self.fixture_env('legacy'))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('nonempty unbound target', result.stderr)
        self.assertEqual((legacy / 'sentinel').read_text(), 'old artifacts')

    def test_migration_never_modifies_existing_target(self):
        legacy = self.root/'legacy'
        legacy.mkdir()
        (legacy/'sentinel').write_text('preserve')
        (self.repo/'target').symlink_to(legacy, target_is_directory=True)
        for option in ([], ['--status'], ['--unshare']):
            self.command([str(self.repo/'scripts/cargo-share-target.sh'), *option])
        self.assertTrue((self.repo/'target').is_symlink())
        self.assertEqual((legacy/'sentinel').read_text(), 'preserve')


if __name__ == '__main__':
    unittest.main()
