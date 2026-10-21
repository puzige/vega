import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import harness2_vega_adapter as bridge
import harness_pilot as pilot


DATA = Path(__file__).resolve().parents[2] / 'research' / 'harness2'
PROMPT_PATH = 'harness2/core/prompts.py'
BASE_SOURCE = b"CORE_POLICY = 'fixed'\n"
PATCHED_SOURCE = BASE_SOURCE + b"NATIVE_PROMPT_SURFACE = 'fixture'\n"
APPROVED_PATCH = (
    b'diff --git a/harness2/core/prompts.py b/harness2/core/prompts.py\n'
    b'index 07decf2e84e758673a074c99482b4cea80953fd9..0910fc79c5abe07706046a48868714ebca67533a 100644\n'
    b'--- a/harness2/core/prompts.py\n'
    b'+++ b/harness2/core/prompts.py\n'
    b'@@ -1 +1,2 @@\n'
    b" CORE_POLICY = 'fixed'\n"
    b"+NATIVE_PROMPT_SURFACE = 'fixture'\n")


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


class ExternalGit:
    def __init__(self):
        self.head = pilot.UPSTREAM_REVISION
        self.status = ' M ' + PROMPT_PATH + '\n'
        self.diff = APPROVED_PATCH.decode()
        self.index_diff = ''

    def read(self, root, *arguments):
        command, *options = arguments
        if command == 'rev-parse':
            if 'HEAD:' + PROMPT_PATH in options:
                return '07decf2e84e758673a074c99482b4cea80953fd9'
            return self.head
        if command == 'show':
            return BASE_SOURCE.decode()
        if command == 'status':
            return self.status.replace('\n', '\0') if '-z' in options else self.status
        if command == 'diff':
            value = self.index_diff if '--cached' in options or '--staged' in options else self.diff
            if '--name-only' in options:
                return PROMPT_PATH + '\n' if value else ''
            return value
        if command == 'ls-files':
            return 'harness2/core/injected.py\n' if '?? ' in self.status else ''
        raise AssertionError('Unexpected external Git operation: ' + repr(arguments))

    def run(self, command, **keywords):
        if Path(command[0]).name != 'git' or command[1] != '-C':
            raise AssertionError('Unexpected external process: ' + repr(command))
        output = self.read(Path(command[2]), *command[3:])
        text = keywords.get('text') or keywords.get('encoding')
        return subprocess.CompletedProcess(command, 0, output if text else output.encode(),
                                           '' if text else b'')


class NativeBoundary:
    def __init__(self):
        self.prepared = []
        self.live_calls = 0
        self.config_identity = pilot.digest('native-fixture-config')
        self.preflight = {'pricing_configured': True}

    def prepare(self, request):
        self.prepared.append(request)
        return self.preflight

    def __call__(self, *arguments, **keywords):
        self.live_calls += 1
        raise AssertionError('Prompt preflight must not execute a native trial')


class ExternalFramework:
    def __init__(self, root, failure=None):
        self.rendered = []
        self.paid_calls = []
        self.failure = failure
        self.modules = {}
        for name in ('core.spec', 'core.tree', 'substrate.base', 'core.proposer',
                     'core.schedule', 'substrate.opencode', 'core.prompts'):
            module = ModuleType('harness2.' + name)
            module.__file__ = str(root / ('harness2/' + name.replace('.', '/') + '.py'))
            self.modules[module.__name__] = module
        self.spec = self.modules['harness2.core.spec']
        self.spec.BenchPrompts = lambda **fields: SimpleNamespace(prompt_pack='adapt', **fields)
        self.spec.BenchSpec = lambda **fields: SimpleNamespace(**fields)
        self.spec.ChannelSpec = lambda kind, model, **fields: SimpleNamespace(kind=kind, model=model, **fields)
        self.tree = self.modules['harness2.core.tree']
        self.tree.SurfaceSpec = lambda **fields: SimpleNamespace(**fields)
        self.base = self.modules['harness2.substrate.base']
        self.proposer = self.modules['harness2.core.proposer']
        self.proposer.Proposer = lambda *args, **kwargs: SimpleNamespace(call=self.paid_call)
        self.schedule = self.modules['harness2.core.schedule']
        self.schedule.run_sweep = self.paid_call
        self.opencode = self.modules['harness2.substrate.opencode']
        self.opencode.OpencodeSubstrate = lambda: SimpleNamespace(preflight=lambda: None)
        self.prompts = self.modules['harness2.core.prompts']
        self.prompts.propose = lambda profile, dimension, surface, **kwargs: self.render(dimension, surface)
        self.prompts.aggregate = lambda profile, surface, **kwargs: self.render('aggregate', surface)
        self.prompts.compose_round = lambda profile, surface, **kwargs: self.render('compose', surface)

    def paid_call(self, *arguments, **keywords):
        self.paid_calls.append((arguments, keywords))
        raise AssertionError('Prompt preflight must not start an upstream model session')

    def render(self, stage, surface):
        if (surface.name != 'vega_native' or surface.always_on != ('strategy.md',)
                or surface.skill_dir != 'skills' or surface.plugin_dir is not None
                or surface.hooks_file is not None):
            raise ValueError('Framework received a different native editing surface')
        self.rendered.append(stage)
        if self.failure in ('missing', stage):
            raise ValueError('Native prompt template unavailable: ' + stage)
        return 'Native Vega strategy.md and fixed approved skills: ' + stage

    def import_module(self, name):
        return sys.modules.get(name, self.modules[name])

    def api(self):
        return SimpleNamespace(spec=self.spec, tree=self.tree, base=self.base,
                               proposer=self.proposer, schedule=self.schedule,
                               opencode=self.opencode, prompts=self.prompts)


class NativePromptSurfaceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.upstream = self.root / 'upstream'
        self.source = self.upstream / PROMPT_PATH
        self.source.parent.mkdir(parents=True)
        self.source.write_bytes(PATCHED_SOURCE)
        self.patch_path = self.root / 'approved-native-surface.patch'
        self.patch_path.write_bytes(APPROVED_PATCH)
        self.git = ExternalGit()
        for patcher in (
                mock.patch.object(bridge.subprocess, 'run', self.git.run),
                mock.patch.object(bridge, 'PROMPT_SURFACE_PATCH_PATH', self.patch_path, create=True),
                mock.patch.object(bridge, 'PROMPT_SURFACE_PATCH_SHA256', sha256(APPROVED_PATCH), create=True)):
            patcher.start()
            self.addCleanup(patcher.stop)

    def adapter(self, framework, launcher, api=None):
        profile = self.root / 'opencode.json'
        profile.write_text(pilot.canonical_json({'provider': {'fixture': {'models': {
            'gpt-6.1-sol': {'variants': {'max': {'reasoningEffort': 'max'}},
                            'cost': {'input': 2, 'output': 10}}}}}}))
        isolated = self.root / 'isolated'
        isolated.mkdir(exist_ok=True)
        trusted = {path.parent.name: pilot.digest(path.read_text())
                   for path in (DATA / 'baseline/skills').glob('*/SKILL.md')}
        adapter = bridge.VegaAdapter(
            api=framework.api() if api is None else api,
            upstream_root=self.upstream, catalog_path=DATA / 'tasks.json',
            baseline_dir=DATA / 'baseline', runs_root=self.root / 'runs', launcher=launcher,
            trusted_skill_approvals=trusted, opencode_config=profile,
            channel_model='fixture/gpt-6.1-sol', isolated_config_root=isolated,
            model=pilot.DEFAULT_MODEL, limits=pilot.DEFAULT_LIMITS, fixed_policy=pilot.FIXED_POLICY)
        self.addCleanup(adapter.close)
        return adapter

    def test_exact_approved_source_reports_base_patch_and_actual_source_identity(self):
        identity = bridge._verify_upstream_source(self.upstream)
        self.assertEqual(identity['upstream_revision'], pilot.UPSTREAM_REVISION)
        self.assertEqual(identity['prompt_surface_patch_sha256'], sha256(APPROVED_PATCH))
        self.assertEqual(identity['prompt_surface_source_sha256'], sha256(PATCHED_SOURCE))

    def test_source_admission_rejects_unapproved_patch_source_index_and_imports(self):
        mutations = ('head', 'missing_patch', 'patch_bytes', 'source_bytes',
                     'index', 'other_tracked', 'untracked_import')
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.git = ExternalGit()
                self.source.write_bytes(PATCHED_SOURCE)
                self.patch_path.write_bytes(APPROVED_PATCH)
                if mutation == 'head':
                    self.git.head = 'f' * 40
                elif mutation == 'missing_patch':
                    self.git.status = ''
                    self.git.diff = ''
                    self.source.write_bytes(BASE_SOURCE)
                elif mutation == 'patch_bytes':
                    self.patch_path.write_bytes(APPROVED_PATCH + b'\nchanged\n')
                elif mutation == 'source_bytes':
                    self.source.write_bytes(PATCHED_SOURCE + b"UNAPPROVED = True\n")
                elif mutation == 'index':
                    self.git.status = 'MM ' + PROMPT_PATH + '\n'
                    self.git.index_diff = APPROVED_PATCH.decode()
                elif mutation == 'other_tracked':
                    self.git.status += ' M harness2/core/selection.py\n'
                elif mutation == 'untracked_import':
                    self.git.status += '?? harness2/core/injected.py\n'
                with mock.patch.object(bridge.subprocess, 'run', self.git.run):
                    with self.assertRaises(ValueError):
                        bridge._verify_upstream_source(self.upstream)

    def test_missing_or_broken_templates_refuse_ready_before_native_prepare_or_model(self):
        for failure in ('missing', 'INV', 'DOM', 'aggregate', 'compose'):
            with self.subTest(template=failure):
                framework = ExternalFramework(self.upstream, failure=failure)
                launcher = NativeBoundary()
                adapter = self.adapter(framework, launcher)
                with self.assertRaisesRegex(ValueError, '(?i)(native|prompt|surface|template)'):
                    adapter.preflight()
                self.assertEqual(launcher.prepared, [])
                self.assertEqual(launcher.live_calls, 0)
                self.assertEqual(framework.paid_calls, [])

    def test_ready_requires_all_native_authoring_paths_and_preserves_source_provenance(self):
        framework = ExternalFramework(self.upstream)
        launcher = NativeBoundary()
        with mock.patch.object(bridge.importlib, 'import_module', framework.import_module):
            api = bridge._load_upstream(self.upstream)
            result = self.adapter(framework, launcher, api=api).preflight()
        self.assertEqual(result['status'], 'ready')
        self.assertEqual(set(framework.rendered), {'INV', 'DOM', 'aggregate', 'compose'})
        self.assertEqual(result['prompt_surface_patch_sha256'], sha256(APPROVED_PATCH))
        self.assertEqual(result['prompt_surface_source_sha256'], sha256(PATCHED_SOURCE))
        self.assertEqual(result['upstream_revision'], pilot.UPSTREAM_REVISION)
        self.assertEqual(result['provider_identity'], pilot.launcher_identity(launcher))
        self.assertEqual(len(launcher.prepared), 1)
        self.assertEqual(launcher.live_calls, 0)
        self.assertEqual(framework.paid_calls, [])

    def test_cached_old_prompt_module_cannot_reuse_the_current_source_path(self):
        identity = bridge._verify_upstream_source(self.upstream)
        self.assertEqual(identity['prompt_surface_source_sha256'], sha256(PATCHED_SOURCE))
        framework = ExternalFramework(self.upstream, failure='missing')
        cached = framework.prompts
        package = ModuleType('harness2')
        package.__file__ = str(self.upstream / 'harness2/__init__.py')
        launcher = NativeBoundary()
        with mock.patch.dict(sys.modules, {'harness2': package, 'harness2.core.prompts': cached}):
            with mock.patch.object(bridge.importlib, 'import_module', framework.import_module):
                with self.assertRaises(ValueError):
                    api = bridge._load_upstream(self.upstream)
                    self.adapter(framework, launcher, api=api).preflight()
        self.assertEqual(launcher.prepared, [])
        self.assertEqual(launcher.live_calls, 0)
        self.assertEqual(framework.paid_calls, [])


if __name__ == '__main__':
    unittest.main()
