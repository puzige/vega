"""Real Git/Cargo boundaries, with owned fixtures and no Vega compilation."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2]


class VerificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='vega-verify-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'repo'
        self.root.mkdir()
        self.env = os.environ.copy()
        for key in subprocess.check_output(['git', 'rev-parse', '--local-env-vars'], text=True).splitlines():
            self.env.pop(key, None)
        self.env.update(GIT_CONFIG_GLOBAL=str(Path(self.temp.name) / 'gitconfig'), GIT_CONFIG_NOSYSTEM='1')
        self.cmd('git', 'init', '-b', 'master')
        self.cmd('git', 'config', 'user.name', 'Fixture')
        self.cmd('git', 'config', 'user.email', 'fixture@example.invalid')
        shutil.copytree(SOURCE / 'scripts', self.root / 'scripts', ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copytree(SOURCE / '.githooks', self.root / '.githooks')
        (self.root / '.gitignore').write_text('target/\n')
        (self.root / 'Cargo.toml').write_text('[workspace]\nmembers=["core", "app"]\nresolver="2"\n')
        for name in ('core', 'app'):
            (self.root / name / 'src').mkdir(parents=True)
            manifest = f'[package]\nname="fixture_{name}"\nversion="0.1.0"\nedition="2021"\n'
            if name == 'app':
                manifest += '[dependencies]\nfixture_core={path="../core"}\n'
            (self.root / name / 'Cargo.toml').write_text(manifest)
            (self.root / name / 'src/lib.rs').write_text('pub fn value() -> u8 {\n    1\n}\n')
        self.cmd('cargo', 'generate-lockfile', '--offline')
        self.cmd('git', 'add', '.')
        self.cmd('git', 'commit', '-m', 'fixture')
        self.cmd('git', 'update-ref', 'refs/remotes/origin/master', 'HEAD')

    def cmd(self, *args, ok=True, env=None):
        result = subprocess.run(args, cwd=self.root, env=env or self.env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60)
        if ok:
            self.assertEqual(result.returncode, 0, result.stdout)
        return result

    def verify(self, *args, **kwargs):
        return self.cmd('python3', '-B', 'scripts/verify.py', *args, **kwargs)

    def test_scope_reverse_dependencies_unknown_and_docs(self):
        (self.root / 'core/src/lib.rs').write_text('pub fn value() -> u8 {\n    2\n}\n')
        self.assertEqual(json.loads(self.verify('--plan').stdout)['packages'], ['fixture_app', 'fixture_core'])
        self.cmd('git', 'checkout', '--', 'core/src/lib.rs')
        (self.root / 'notes.md').write_text('Documentation\n')
        self.assertEqual(json.loads(self.verify('--plan').stdout)['packages'], [])
        (self.root / 'scripts/unknown.swift').write_text('print("x")')
        self.assertNotEqual(self.verify('--plan', ok=False).returncode, 0)
        self.assertEqual(len(json.loads(self.verify('--plan', '--full').stdout)['packages']), 2)
        (self.root / 'scripts/unknown.swift').unlink()
        (self.root / 'core/README.md').write_text('Included crate documentation\n')
        self.assertEqual(json.loads(self.verify('--plan').stdout)['packages'], ['fixture_app', 'fixture_core'])
        (self.root / 'core/README.md').unlink()
        (self.root / 'scripts/verify.py').write_text((self.root / 'scripts/verify.py').read_text() + '\n')
        self.assertTrue(json.loads(self.verify('--plan').stdout)['tooling'])
        (self.root / 'Cargo.toml').write_text((self.root / 'Cargo.toml').read_text() + '# shared change\n')
        self.assertNotEqual(self.verify('--plan', ok=False).returncode, 0)


    def test_real_verification_reuse_and_invalidations(self):
        (self.root / 'notes.md').write_text('Documentation\n')
        first = self.verify()
        self.assertIn('verify: passed', first.stdout)
        self.assertIn('reused', self.verify().stdout)
        # Commit changes source metadata, not source content; evidence stays valid.
        self.cmd('git', 'add', 'notes.md')
        self.cmd('git', 'commit', '-m', 'docs')
        self.assertIn('reused', self.verify().stdout)
        env = dict(self.env, RUSTFLAGS='--cfg fixture_changed')
        self.assertNotIn('reused', self.verify(env=env).stdout)
        evidence = self.root / '.git/vega-verification'
        for log in evidence.glob('*/0.log'):
            log.write_text('tampered')
        self.assertNotIn('reused', self.verify().stdout)
        (self.root / 'notes.md').write_text('Changed\n')
        self.assertNotIn('reused', self.verify().stdout)
        self.cmd('git', 'update-ref', 'refs/remotes/origin/master', 'HEAD')
        self.assertNotIn('reused', self.verify().stdout)

    def test_real_push_hook_reuses_and_rejects_dirty_and_non_head(self):
        bare = Path(self.temp.name) / 'remote.git'
        self.cmd('git', 'init', '--bare', str(bare))
        self.cmd('git', 'remote', 'add', 'origin', str(bare))
        self.cmd('git', 'config', 'core.hooksPath', '.githooks')
        (self.root / 'notes.md').write_text('Push evidence\n')
        self.cmd('git', '-c', 'core.hooksPath=/dev/null', 'add', 'notes.md')
        self.cmd('git', '-c', 'core.hooksPath=/dev/null', 'commit', '-m', 'docs')
        # Hook prepends rustup PATH: verification must use the same environment.
        self.verify()
        pushed = self.cmd('git', 'push', 'origin', 'HEAD:refs/heads/feature')
        records = [json.loads(p.read_text())['identity'] for p in (self.root / '.git/vega-verification').glob('*/result.json')]
        differences = {key for r in records for key in r['environment'] if any(other['environment'].get(key) != r['environment'].get(key) for other in records)}
        self.assertIn('reused', pushed.stdout + repr(differences))
        (self.root / 'notes.md').write_text('Dirty\n')
        self.assertNotEqual(self.cmd('git', 'push', 'origin', 'HEAD:refs/heads/dirty', ok=False).returncode, 0)
        self.cmd('git', 'checkout', '--', 'notes.md')
        self.cmd('git', 'branch', 'old', 'HEAD~1')
        self.assertNotEqual(self.cmd('git', 'push', 'origin', 'old', ok=False).returncode, 0)
        self.cmd('git', 'push', 'origin', ':refs/heads/feature')

    def test_real_cargo_source_edit_during_verification_rejected(self):
        # Real build script mutates another source input during clippy.
        (self.root / 'core/build.rs').write_text('fn main() {\n    std::fs::write("../notes.md", "mutated\\n").unwrap();\n}\n')
        (self.root / 'notes.md').write_text('original\n')
        result = self.verify(ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('changed during verification', result.stdout)
        records = list((self.root / '.git/vega-verification').glob('*/result.json'))
        self.assertTrue(all(not json.loads(p.read_text())['success'] for p in records))

    def test_internal_skill_directory_symlink_and_hidden_input(self):
        skills = self.root / '.agents/skills/example'
        skills.mkdir(parents=True)
        (skills / 'SKILL.md').write_text('Tracked skill\n')
        links = self.root / '.claude/skills'
        links.mkdir(parents=True)
        (links / 'example').symlink_to('../../.agents/skills/example')
        self.cmd('git', 'add', '.agents', '.claude')
        self.cmd('git', 'commit', '-m', 'skill links')
        self.cmd('git', 'update-ref', 'refs/remotes/origin/master', 'HEAD')
        self.assertIn('passed', self.verify().stdout)
        self.assertIn('reused', self.verify().stdout)
        (skills / 'SKILL.md').write_text('Changed skill\n')
        self.assertNotIn('reused', self.verify().stdout)
        with (self.root / '.git/info/exclude').open('a') as exclude:
            exclude.write('hidden.txt\n')
        (skills / 'hidden.txt').write_text('Ignored input\n')
        rejected = self.verify(ok=False)
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn('Unfingerprinted source symlink input', rejected.stdout)

    def test_real_cargo_json_reports_managed_release_executable(self):
        (self.root / 'app/src/main.rs').write_text('fn main() { println!("artifact"); }\n')
        target = Path(self.cmd('scripts/cargo-lock.sh', '--target-path').stdout.strip())
        output = self.cmd('scripts/cargo-lock.sh', 'build', '--release', '-p', 'fixture_app', '--message-format=json-render-diagnostics')
        messages = [json.loads(line) for line in output.stdout.splitlines() if line.startswith('{')]
        bins = [Path(m['executable']) for m in messages if m.get('reason') == 'compiler-artifact' and m.get('executable')]
        self.assertEqual(bins, [target / 'release/fixture_app'])
        self.assertEqual(self.cmd(str(bins[0])).stdout.strip(), 'artifact')
        self.assertFalse((self.root / 'target/release/fixture_app').exists())

    def test_failed_and_incomplete_evidence_never_reused(self):
        (self.root / 'core/src/lib.rs').write_text('invalid Rust\n')
        result = self.verify(ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('reused', self.verify(ok=False).stdout)
        records = list((self.root / '.git/vega-verification').glob('*/result.json'))
        self.assertTrue(records)
        self.assertTrue(all(not json.loads(p.read_text())['success'] for p in records))


if __name__ == '__main__':
    unittest.main()
