import argparse
from contextlib import contextmanager
import hashlib
import importlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace

import harness_pilot as pilot


PROMPT_SURFACE_PATH = 'harness2/core/prompts.py'
PROMPT_SURFACE_PATCH_PATH = (Path(__file__).resolve().parents[1]
                             / 'research/harness2/patches/vega-native-prompt-surface.diff')
PROMPT_SURFACE_PATCH_SHA256 = '8db17ca7306a24cd954062bba4609a2551d495a195eecf58a3f4c8e1cff70d6d'


@contextmanager
def native_secret_scope(launcher):
    original = os.environ.pop('VEGA_HARNESS_API_KEY', None)
    if original is not None:
        launcher._secret = original
    try:
        yield
    finally:
        if original is not None and 'VEGA_HARNESS_API_KEY' not in os.environ:
            os.environ['VEGA_HARNESS_API_KEY'] = original


def _git(root, *arguments):
    result = subprocess.run(['git', '-C', str(root), *arguments], text=True,
                            capture_output=True, check=False)
    if result.returncode:
        raise ValueError('Provide an existing clean pinned Harness2 checkout')
    return result.stdout.strip()


def _git_bytes(root, *arguments):
    result = subprocess.run(['git', '-C', str(root), *arguments],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    if result.returncode:
        raise ValueError('Cannot verify the pinned Harness2 source')
    return result.stdout


def _verify_upstream_source(root):
    root = Path(root).resolve()
    if (root / '.env').exists():
        raise ValueError('Use an isolated Harness2 checkout without an auto-loaded .env file')
    if _git_bytes(root, 'rev-parse', 'HEAD').strip() != pilot.UPSTREAM_REVISION.encode():
        raise ValueError('Harness2 checkout must match the fixed upstream revision')
    patch_path = Path(PROMPT_SURFACE_PATCH_PATH)
    if patch_path.is_symlink() or not patch_path.is_file():
        raise ValueError('The approved native prompt compatibility patch is unavailable')
    patch = patch_path.read_bytes()
    if hashlib.sha256(patch).hexdigest() != PROMPT_SURFACE_PATCH_SHA256:
        raise ValueError('The approved native prompt compatibility patch changed')
    path = re.escape(PROMPT_SURFACE_PATH.encode())
    header = re.match(rb'\Adiff --git a/' + path + rb' b/' + path
                      + rb'\nindex ([0-9a-f]{40})\.\.([0-9a-f]{40}) 100644\n--- a/'
                      + path + rb'\n\+\+\+ b/' + path + rb'\n', patch)
    if header is None or patch.count(b'diff --git ') != 1:
        raise ValueError('The approved native prompt patch must change one regular source file')
    if _git_bytes(root, 'diff', '--cached', '--no-ext-diff', '--no-textconv', '--binary', '--full-index'):
        raise ValueError('Harness2 staged changes are not allowed')
    status = _git_bytes(root, 'status', '--porcelain=v1', '--untracked-files=all', '-z', '--', '.', ':!dependencies')
    if status != b' M ' + PROMPT_SURFACE_PATH.encode() + b'\0':
        raise ValueError('Install only the approved native prompt patch; other source changes are not allowed')
    difference = _git_bytes(root, 'diff', '--no-ext-diff', '--no-textconv', '--no-color',
                            '--binary', '--full-index', '--unified=3', '--diff-algorithm=myers',
                            '--src-prefix=a/', '--dst-prefix=b/', 'HEAD', '--')
    if difference != patch:
        raise ValueError('Harness2 differs from the exact approved native prompt patch')
    if _git_bytes(root, 'rev-parse', 'HEAD:' + PROMPT_SURFACE_PATH).strip() != header[1]:
        raise ValueError('The native prompt patch does not match its pinned base source')
    source_path = root / PROMPT_SURFACE_PATH
    if source_path.is_symlink() or not source_path.is_file() or not source_path.resolve().is_relative_to(root):
        raise ValueError('The patched native prompt source is not a regular checkout file')
    source = source_path.read_bytes()
    blob = hashlib.sha1(b'blob ' + str(len(source)).encode() + b'\0' + source).hexdigest()
    if blob.encode() != header[2]:
        raise ValueError('The actual native prompt source bytes do not match the approved patch')
    return {'upstream_revision': pilot.UPSTREAM_REVISION,
            'prompt_surface_patch_sha256': hashlib.sha256(patch).hexdigest(),
            'prompt_surface_source_sha256': hashlib.sha256(source).hexdigest()}


def _load_upstream(root):
    root = Path(root).resolve()
    source_identity = _verify_upstream_source(root)
    existing = sys.modules.get('harness2')
    if existing is not None:
        source = getattr(existing, '__file__', None)
        if source is None or not Path(source).resolve().is_relative_to(root):
            raise ValueError('A different Harness2 installation is already imported')
    cached_prompts = sys.modules.get('harness2.core.prompts')
    if (cached_prompts is not None
            and getattr(cached_prompts, '_vega_upstream_source', None) != source_identity):
        raise ValueError('Cached native prompt code has no matching source identity; use a fresh process')
    sys.path.insert(0, str(root))
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec = importlib.import_module('harness2.core.spec')
        tree = importlib.import_module('harness2.core.tree')
        base = importlib.import_module('harness2.substrate.base')
        proposer = importlib.import_module('harness2.core.proposer')
        schedule = importlib.import_module('harness2.core.schedule')
        opencode = importlib.import_module('harness2.substrate.opencode')
        prompts = importlib.import_module('harness2.core.prompts')
    except ImportError as error:
        raise ValueError('Install the pinned upstream dependencies in the isolated research environment') from error
    finally:
        sys.dont_write_bytecode = previous
        sys.path.remove(str(root))
    for module in (spec, tree, base, proposer, schedule, opencode, prompts):
        if not Path(module.__file__).resolve().is_relative_to(root):
            raise ValueError('Harness2 imported source outside the pinned checkout')
    if Path(prompts.__file__).resolve() != root / PROMPT_SURFACE_PATH:
        raise ValueError('Native prompts did not load from the approved source file')
    if _verify_upstream_source(root) != source_identity:
        raise ValueError('Native prompt source changed during import')
    prompts._vega_upstream_source = dict(source_identity)
    return SimpleNamespace(spec=spec, tree=tree, base=base, proposer=proposer,
                           schedule=schedule, opencode=opencode, prompts=prompts,
                           upstream_source=source_identity)


def _opencode_profile(path, channel_model):
    if not isinstance(channel_model, str) or '/' not in channel_model:
        raise ValueError('Provide an explicit provider-prefixed OpenCode model')
    provider, model = channel_model.split('/', 1)
    if model != pilot.DEFAULT_MODEL['id']:
        raise ValueError('OpenCode proposer and selector must use gpt-6.1-sol')
    document = pilot._read_json(path)
    if not isinstance(document, dict):
        raise ValueError('OpenCode configuration must be a JSON object')
    text = pilot.canonical_json(document)
    if 'VEGA_HARNESS_API_KEY' in text:
        raise ValueError('OpenCode must use a separately configured channel credential')

    def credential_fields(value):
        if isinstance(value, dict):
            for key, child in value.items():
                if key.lower().replace('_', '') in ('apikey', 'authorization', 'token', 'secret'):
                    if not isinstance(child, str) or not child.startswith('{env:') or not child.endswith('}'):
                        raise ValueError('OpenCode credentials must use explicit environment references')
                credential_fields(child)
        elif isinstance(value, list):
            for child in value:
                credential_fields(child)

    credential_fields(document)
    entry = document.get('provider', {}).get(provider, {}).get('models', {}).get(model)
    if not isinstance(entry, dict) or not isinstance(entry.get('variants', {}).get('max'), dict):
        raise ValueError('Explicit OpenCode profile must declare the model and max variant')
    variant = entry['variants']['max']
    if variant.get('reasoningEffort') != 'max':
        raise ValueError('The OpenCode max variant must explicitly select max reasoning effort')
    pricing = entry.get('cost', {})
    if any(type(pricing.get(key)) not in (int, float) or not math.isfinite(pricing[key])
           or pricing[key] <= 0 for key in ('input', 'output')):
        raise ValueError('Configure explicit positive OpenCode input/output pricing before live search')
    return document


class VegaNativeSubstrate:
    name = 'vega_native'

    def __init__(self, adapter, api):
        self.adapter = adapter
        self.api = api
        self._surface = api.tree.SurfaceSpec(
            name=self.name, always_on=('strategy.md',), always_on_max=24000,
            always_on_cap_reason='Native strategy shares the bounded context budget',
            skill_dir='skills', skill_name_rule='must_match',
            trigger_hint='Name the approved skill in strategy.md when it should be loaded',
            autotrigger_file='strategy.md', trigger_line='When {desc}, load the `{name}` skill.\n',
            subagent_requires_mode=False, plugin_dir=None, hooks_file=None,
            reject_empty_always_on=True)

    def surface(self):
        return self._surface

    def materialise(self, tree_dir, spec, ctx):
        if spec.install_mode != 'workspace_agents_md' or ctx:
            raise ValueError('Native harness staging accepts guidance only, without integration files')
        tree = pilot.load_harness(tree_dir)
        self.adapter.validate_tree(tree)
        staged = Path(tempfile.mkdtemp(prefix='vega-h2-guidance-'))
        try:
            for relative, text in tree.items():
                path = staged / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding='utf-8')
        except BaseException:
            shutil.rmtree(staged)
            raise
        return self.api.base.Install(staging_dir=staged, fingerprint=pilot.digest(tree),
                                     extras={'native_guidance_protocol': '1'})

    def execute(self, task_ref, install, run_ids, timeout):
        if type(timeout) is not int or timeout <= 0:
            raise ValueError('A positive fixed native timeout is required')
        if timeout * 1000 != self.adapter.limits['timeout_ms']:
            raise ValueError('Upstream timeout cannot change the fixed native limits')
        return [self.adapter._native_execute(task_ref, install.staging_dir, run_id)
                for run_id in run_ids]

    def read_trajectory(self, stream_path, *, obs_trunc, max_chars, args_trunc=400):
        return self.adapter._trajectory(pilot._read_json(stream_path), obs_trunc,
                                        max_chars, args_trunc)

    def preflight(self):
        self.adapter.preflight()


class VegaAdapter:
    def __init__(self, *, api, upstream_root, catalog_path, baseline_dir, runs_root,
                 launcher, trusted_skill_approvals, opencode_config, channel_model,
                 isolated_config_root, model, limits, fixed_policy,
                 channel_steps=120, channel_timeout=3600):
        self.api = api
        self.upstream_root = Path(upstream_root).resolve()
        self.catalog_path = Path(catalog_path).resolve()
        self.catalog = pilot.load_catalog(catalog_path)
        self._tasks = {task['task_id']: task for task in self.catalog['tasks']
                       if task['split'] == 'optimization'}
        self._baseline = Path(baseline_dir).resolve()
        self.runs_root = Path(runs_root).resolve()
        self.launcher = launcher
        self.trusted_skill_approvals = dict(trusted_skill_approvals)
        self.opencode_config = Path(opencode_config).resolve()
        self.opencode_document = _opencode_profile(self.opencode_config, channel_model)
        self.isolated_config_root = Path(isolated_config_root).resolve()
        if self.isolated_config_root.is_symlink() or not self.isolated_config_root.is_dir():
            raise ValueError('Provide a dedicated empty isolated OpenCode configuration directory')
        if any(self.isolated_config_root.iterdir()):
            raise ValueError('The isolated OpenCode configuration directory must be empty')
        self.model = pilot._model(model)
        self.limits = pilot._positive_limits(limits)
        if self.limits['timeout_ms'] % 1000:
            raise ValueError('Harness2 native timeout must be an integer number of seconds')
        self.fixed_policy = fixed_policy
        if type(channel_steps) is not int or channel_steps <= 0 or type(channel_timeout) is not int or channel_timeout <= 0:
            raise ValueError('Bounded proposer and selector steps and timeout are required')
        channel = api.spec.ChannelSpec('opencode', channel_model, effort='max',
                                       steps=channel_steps, timeout=channel_timeout)
        prompts = api.spec.BenchPrompts(
            work='Read provided task inputs and produce the requested text or JSON files using bounded native tools.',
            domain_word='task family',
            inv_levers='Edit strategy.md and skill trigger guidance. Keep approved skill body bytes unchanged.',
            dom_levers='Improve the reading, recovery and verification workflow within the fixed tools and limits.',
            yardstick='The requested files must accurately follow the task and preserve fixed source inputs.',
            deliverable_word='requested file',
            domain_guidance='Only strategy.md and approved skills/<slug>/SKILL.md paths are allowed. No scripts, hooks, configuration or delegation.',
            selection_checks='Read every requested file and its source inputs. Distinguish an execution failure from a valid final artifact.')
        self.spec = api.spec.BenchSpec(
            name='vega_microtasks', runs_root=self.runs_root / 'method',
            install_mode='workspace_agents_md', prompts=prompts,
            channels={'proposer': channel, 'selector': channel}, allow_subagents=False,
            leak_globs=('oracle*', '*expected*', '*evaluation*', 'input.json',
                        'manifest.json', 'native.json', 'tasks.json'), obs_trunc=50000)
        self.substrate = VegaNativeSubstrate(self, api)
        self._public_owner = tempfile.TemporaryDirectory(prefix='vega-h2-public-')
        self._public = pilot.stage_public_tasks(self.catalog, Path(self._public_owner.name) / 'inputs',
                                               'optimization')
        self._native_runs = self.runs_root / 'native_trials'
        self.validate_tree(pilot.load_harness(self._baseline))

    def close(self):
        self._public_owner.cleanup()

    def _task(self, task_id):
        if task_id not in self._tasks:
            raise ValueError('Optimization cannot access holdout or unknown tasks')
        return self._tasks[task_id]

    def tasks(self, split='all'):
        if split not in ('all', 'train', 'optimization'):
            raise ValueError('This search adapter exposes only the optimization split')
        return list(self._tasks)

    def domain_of(self, task_id):
        return self._task(task_id)['domain']

    def task_prompt(self, task_id):
        return self._task(task_id)['task_prompt']

    def task_files(self, task_id):
        self._task(task_id)
        return self._public[task_id]

    def base_harness(self):
        return self._baseline

    def validate_tree(self, tree):
        pilot.validate_tree(tree)
        pilot._harness_payload(tree, self.trusted_skill_approvals)

    def preflight(self):
        source_identity = _verify_upstream_source(self.upstream_root)
        if getattr(self.api, 'upstream_source', source_identity) != source_identity:
            raise ValueError('Native prompt source differs from the loaded framework')
        cached = sys.modules.get('harness2.core.prompts')
        if (cached is not None and (cached is not self.api.prompts
                or getattr(cached, '_vega_upstream_source', None) != source_identity)):
            raise ValueError('Cached native prompt code differs from its verified source')
        try:
            prompts = self.api.prompts
            profile = self.spec.prompts
            surface = self.substrate.surface()
            rendered = {
                'INV': prompts.propose(profile, 'INV', surface, k=1, index=1),
                'DOM': prompts.propose(profile, 'DOM', surface, k=1, index=1),
                'aggregate': prompts.aggregate(profile, surface, k=1, index=1),
                'compose': prompts.compose_round(profile, surface, t=1, k=1),
            }
            if any(not isinstance(text, str) or not text.strip() for text in rendered.values()):
                raise ValueError('A native prompt template returned no text')
        except Exception as error:
            raise ValueError('Native prompt template preflight failed: ' + type(error).__name__) from error
        self.api.opencode.OpencodeSubstrate().preflight()
        if pilot.digest(pilot._read_json(self.opencode_config)) != pilot.digest(self.opencode_document):
            raise ValueError('OpenCode configuration changed')
        task = next(iter(self._tasks.values()))
        request = pilot.make_request(task, self._baseline, self.model, self.limits,
                                     self.fixed_policy, self.trusted_skill_approvals)
        self.launcher.prepare(request)
        if self.launcher.preflight.get('pricing_configured') is not True:
            raise ValueError('Configure explicit native pricing before a paid optimization search')
        return {'status': 'ready', 'transport_verified': False, 'live_calls': 0,
                'provider_identity': pilot.launcher_identity(self.launcher),
                **source_identity, 'native_prompt_templates_sha256': pilot.digest(rendered),
                'opencode_config_sha256': pilot.digest(self.opencode_document)}

    def execute(self, task_id, harness_dir, run_id, *, timeout=None):
        self.launcher.raise_if_cancelled()
        fixed_seconds = self.limits['timeout_ms'] // 1000
        if timeout is not None and timeout != fixed_seconds:
            raise ValueError('Search cannot change the fixed native deadline')
        install = self.substrate.materialise(harness_dir or self._baseline, self.spec, {})
        try:
            return self.substrate.execute(task_id, install, [run_id], fixed_seconds)[0]
        finally:
            shutil.rmtree(install.staging_dir)

    def _native_execute(self, task_id, harness_dir, upstream_run_id):
        self.launcher.raise_if_cancelled()
        task = self._task(task_id)
        identifier = 'h2-' + pilot.digest(str(upstream_run_id))
        directory = pilot.run_trial(task=task, harness_dir=harness_dir, runs_root=self._native_runs,
                                    run_id=identifier, model=self.model, limits=self.limits,
                                    fixed_policy=self.fixed_policy,
                                    trusted_skill_approvals=self.trusted_skill_approvals,
                                    launcher=self.launcher)
        alias = directory / 'upstream_alias.json'
        document = {'upstream_run_id': str(upstream_run_id), 'native_run_id': identifier}
        if alias.exists():
            if pilot._read_json(alias) != document:
                raise ValueError('Upstream run identity conflicts with the native trial')
        else:
            pilot._write_json(alias, document)
        if pilot.read_run(directory)['native']['terminal'] == 'cancelled':
            self.launcher.request_cancel()
        self.launcher.raise_if_cancelled()
        return directory

    def delivered(self, run_dir):
        try:
            record = pilot.read_run(run_dir)
            native = record['native']
            task = self._task(native['task_id'])
            if native['terminal'] != 'end':
                return False
            for relative, rule in task['oracle']['artifacts'].items():
                text = native['artifacts'].get(relative)
                if not isinstance(text, str) or not text.strip():
                    return False
                if rule['kind'] == 'json':
                    json.loads(text)
            return True
        except (OSError, ValueError, KeyError):
            return False

    def artifacts_intact(self, run_dir):
        try:
            pilot.read_run(run_dir)
            return True
        except (OSError, ValueError):
            return False

    @staticmethod
    def _trajectory(native, obs_trunc, max_chars, args_trunc):
        def clip(value, limit):
            text = value if isinstance(value, str) else pilot.canonical_json(value)
            return text if len(text) <= limit else text[:limit] + ' [truncated]'

        lines = []
        for event in native.get('trace', []):
            if not isinstance(event, dict):
                continue
            kind = event.get('event')
            if kind == 'tool_proposed':
                lines.append('[ACTION] ' + str(event.get('tool', 'tool')) + '(' +
                             clip(event.get('input'), args_trunc) + ')')
            elif kind == 'tool_finished':
                prefix = '[OBSERVATION]' if event.get('status') == 'success' else '[OBSERVATION ERROR]'
                lines.append(prefix + ' ' + clip(event.get('output', event.get('status')), obs_trunc))
            elif kind == 'skill_activation':
                lines.append('[OBSERVATION] skill ' + str(event.get('name')) + ' ' + str(event.get('status')))
        if native.get('final_text'):
            lines.append('[ASSISTANT] ' + clip(native['final_text'], obs_trunc))
        if native.get('terminal') != 'end':
            lines.append('[ERROR] native terminal=' + str(native.get('terminal')))
        return clip('\n'.join(lines) or '(no native tool events)', max_chars)

    def trajectory(self, run_dir, *, obs_trunc):
        native = pilot.read_run(run_dir)['native']
        return self._trajectory(native, obs_trunc, 500000, 2000)

    def deliverable_files(self, run_dir):
        return dict(pilot.read_run(run_dir)['native']['artifacts'])

    def deliverable_text(self, run_dir):
        return '\n\n'.join('[FILE ' + relative + ']\n' + text
                           for relative, text in self.deliverable_files(run_dir).items())

    def score(self, run_dir, task_id):
        native = pilot.read_run(run_dir)['native']
        if native['task_id'] != task_id:
            raise ValueError('Cannot apply a private oracle to another task')
        passed = pilot.evaluate_result(self._task(task_id), native)['delivered']
        return {'n_passed': int(passed), 'n_criteria': 1, 'all_pass': passed}


def build_adapter(*, upstream_root, catalog_path, baseline_dir, runs_root, native_binary,
                  provider_config, trusted_skill_approvals, opencode_config,
                  channel_model, isolated_config_root, model=None, limits=None,
                  fixed_policy=pilot.FIXED_POLICY, channel_steps=120, channel_timeout=3600):
    launcher = pilot.NativeProcessLauncher(native_binary, provider_config)
    with native_secret_scope(launcher):
        api = _load_upstream(upstream_root)
        return VegaAdapter(api=api, upstream_root=upstream_root, catalog_path=catalog_path,
                           baseline_dir=baseline_dir, runs_root=runs_root, launcher=launcher,
                           trusted_skill_approvals=trusted_skill_approvals,
                           opencode_config=opencode_config, channel_model=channel_model,
                           isolated_config_root=isolated_config_root,
                           model=pilot.DEFAULT_MODEL if model is None else model,
                           limits=pilot.DEFAULT_LIMITS if limits is None else limits,
                           fixed_policy=fixed_policy, channel_steps=channel_steps,
                           channel_timeout=channel_timeout)


def _native_records(adapter):
    records = []
    if not adapter._native_runs.is_dir():
        return records
    for path in sorted(adapter._native_runs.iterdir()):
        if not path.is_dir():
            continue
        try:
            records.append(pilot.read_run(path))
        except (ValueError, OSError):
            records.append({'native': None, 'evaluation': {'delivered': False,
                            'errors': ['incomplete_native_trial']}})
    return records


def _audited_proposer(adapter, role, ledger, budget_usd):
    lock = threading.Lock()
    base = adapter.api.proposer.Proposer

    class AuditedProposer(base):
        def env(self):
            adapter.launcher.raise_if_cancelled()
            environment = super().env()
            environment.pop('VEGA_HARNESS_API_KEY', None)
            environment['OPENCODE_CONFIG'] = str(adapter.opencode_config)
            environment['XDG_CONFIG_HOME'] = str(adapter.isolated_config_root)
            return environment

        def _harvest(self, temporary, sub, label, cache_key=''):
            source = temporary / sub
            if source.exists() or source.is_symlink():
                tree = pilot.load_harness(source)
                adapter.validate_tree(tree)
            return super()._harvest(temporary, sub, label, cache_key)

        def call(self, mission, context_dir, label, **kwargs):
            adapter.launcher.raise_if_cancelled()
            records = _native_records(adapter)
            lower_bound = pilot.aggregate_results(records)['known_cost_usd']
            with lock:
                lower_bound += sum(entry['reported_cost_usd'] for entry in ledger
                                   if type(entry.get('reported_cost_usd')) in (int, float))
            if lower_bound >= budget_usd:
                raise adapter.api.proposer.ProposerError('Observed study cost reached the operator budget', attempts=0)
            reply = None
            failure = None
            try:
                reply = super().call(mission, context_dir, label, **kwargs)
                adapter.launcher.raise_if_cancelled()
                return reply
            except adapter.api.proposer.ProposerError as error:
                failure = error
                raise
            finally:
                session_role = 'reflection' if label.startswith('experience.') else role
                observed = reply if reply is not None else failure
                with lock:
                    if observed is not None:
                        amount = observed.cost_usd
                        reported = float(amount) if type(amount) in (int, float) and math.isfinite(amount) and amount >= 0 else None
                        ledger.append({'role': session_role, 'label': label, 'model': self.spec.model,
                                       'reasoning_effort': self.spec.effort,
                                       'ok': bool(getattr(observed, 'ok', False)),
                                       'cached': bool(getattr(observed, 'cached', False)),
                                       'attempts': observed.attempts,
                                       'reported_cost_usd': reported, 'cost_microcents': None,
                                       'pricing_known': False,
                                       'cost_source': 'upstream_reply_without_billing_provenance'})

    return AuditedProposer(adapter.spec.channels[role],
                           audit_dir=adapter.runs_root / 'channel_audit' / role, attempts=1)


def _spend_control(path, adapter, budget_usd, provider_identity):
    document = pilot._read_json(path)
    if (not isinstance(document, dict) or document.get('schema_version') != 1
            or document.get('kind') != 'externally_enforced_study_quota'
            or document.get('currency') != 'USD'
            or not isinstance(document.get('reference'), str) or not document['reference'].strip()
            or document.get('native_provider_identity') != provider_identity
            or document.get('opencode_config_sha256') != pilot.digest(adapter.opencode_document)
            or not isinstance(document.get('covers_roles'), list)
            or any(not isinstance(role, str) for role in document['covers_roles'])
            or set(document['covers_roles']) != {'solver', 'proposer', 'selector', 'reflection'}):
        raise ValueError('Supply a route-bound externally enforced study quota record, not an acknowledgement')
    amount = document.get('limit_usd')
    if type(amount) not in (int, float) or not math.isfinite(amount) or amount <= 0 or amount > budget_usd:
        raise ValueError('External study quota must be positive and at most the authorized budget')
    return document


def optimize(adapter, *, library_root, budget_usd, isolation_label,
             external_spend_control, k=1, seed=7):
    pilot._positive_budget(budget_usd)
    if not isolation_label or not external_spend_control:
        raise ValueError('Provide the operator isolation label and external spend-control reference')
    if type(k) is not int or k <= 0:
        raise ValueError('k must be a positive integer')
    adapter.launcher.capture_secret()
    with adapter.launcher.cancellation_scope(), native_secret_scope(adapter.launcher):
        preflight = adapter.preflight()
        quota = _spend_control(external_spend_control, adapter, budget_usd,
                               preflight['provider_identity'])
        root = adapter.runs_root
        library_root = Path(library_root)
        if root.is_symlink() or (root.exists() and any(root.iterdir())):
            raise ValueError('Provide a new or empty study runs directory; existing evidence will not be overwritten')
        if library_root.is_symlink() or (library_root.exists() and any(library_root.iterdir())):
            raise ValueError('Provide a new or empty study library directory')
        root.mkdir(parents=True, exist_ok=True)
        state = root / 'search_identity.json'
        if state.exists():
            raise ValueError('Search namespace already exists; inspect its fixed evidence instead of replaying it')
        identity = {'schema_version': 1, 'upstream_revision': preflight['upstream_revision'],
                    'prompt_surface_patch_sha256': preflight['prompt_surface_patch_sha256'],
                    'prompt_surface_source_sha256': preflight['prompt_surface_source_sha256'],
                    'native_prompt_templates_sha256': preflight['native_prompt_templates_sha256'],
                    'catalog_sha256': pilot.digest(adapter.catalog),
                    'baseline_sha256': pilot.digest(pilot.load_harness(adapter.base_harness())),
                    'model': adapter.model, 'limits': adapter.limits,
                    'provider_identity': preflight['provider_identity'],
                    'opencode_config_sha256': preflight['opencode_config_sha256'],
                    'channel_model': adapter.spec.channels['proposer'].model,
                    'channel_effort': 'max', 'k': k, 'seed': seed,
                    'budget_usd': budget_usd, 'operator_isolation_label': isolation_label,
                    'external_spend_control': quota,
                    'external_control_provenance': 'operator_supplied_external_quota_record',
                    'external_control_verified_by_bridge': False}
        with state.open('x', encoding='utf-8') as output:
            output.write(pilot.canonical_json(identity) + '\n')
        ledger = []
        proposer = _audited_proposer(adapter, 'proposer', ledger, budget_usd)
        selector = _audited_proposer(adapter, 'selector', ledger, budget_usd)
        try:
            adapter.launcher.raise_if_cancelled()
            report = adapter.api.schedule.run_sweep(
                k, adapter=adapter, prop=proposer, selector_prop=selector,
                runs_root=adapter.spec.runs_root, library_root=library_root,
                split='train', modes=('agg',), seed=seed, domain_parallel=1,
                pool_policy='finals')
        except pilot.StudyCancelled:
            report = {'status': 'cancelled', 'domains': [], 'partial_results_on_disk': True}
        records = _native_records(adapter)
        for record in records:
            native = record.get('native') or {}
            metric = native.get('metrics', {})
            ledger.append({'role': 'solver', 'run_id': native.get('run_id'),
                           'terminal': native.get('terminal', 'controller_failure'),
                           'cost_microcents': metric.get('cost_microcents'),
                           'pricing_known': metric.get('pricing_known', False),
                           'usage_complete': metric.get('usage_complete', False)})
        selections = []
        for path in sorted(adapter.spec.runs_root.glob('*/*/summary.json')):
            summary = pilot._read_json(path)
            for selection in summary.get('selections', {}).values():
                chosen = selection.get('chosen_run_dir')
                if selection.get('ok') and chosen:
                    task_id = summary['task_id']
                    score = adapter.score(Path(chosen), task_id)
                    selections.append({'task_id': task_id, 'native_run_dir': chosen,
                                       'selected_harness': str(path.parent / 'harnesses' / selection['chosen_label']),
                                       'post_selection_oracle': score})
        ledger.append({'role': 'judge', 'cost_microcents': 0, 'pricing_known': True,
                       'kind': 'private_deterministic_oracle', 'model_calls': 0})
        status = ('completed' if all(
            row.get('failed') == 0 and not row.get('error') for row in report.get('domains', []))
            and report.get('domains') else 'partial')
        if adapter.launcher.cancel_requested:
            status = 'cancelled'
        result = {'schema_version': 1, 'status': status,
                  'evidence': 'native_process_and_pinned_upstream', 'identity': identity,
                  'upstream_report': report, 'rollout_summary': pilot.aggregate_results(records),
                  'search_ledger': {'complete': False, 'entries': ledger},
                  'search_cost_usd': None, 'currency_cap_is_hard': False,
                  'cancellation': {'requested': adapter.launcher.cancel_requested,
                                   'native_process_exit_awaited': True,
                                   'native_cancelled_results': sum(
                                       (record.get('native') or {}).get('terminal') == 'cancelled'
                                       for record in records),
                                   'incomplete_native_attempts': sum(record.get('native') is None
                                                                     for record in records),
                                   'inflight_opencode': 'awaited_existing_session_exit_or_upstream_timeout'},
                  'selected_candidates': selections, 'frozen_selection': 'operator_required',
                  'holdout_executed': False}
        pilot._write_json(root / 'search_report.json', result)
        return result


def _parser():
    parser = argparse.ArgumentParser(description='Optional pinned Harness2 native Vega search bridge')
    parser.add_argument('--upstream-root', type=Path)
    parser.add_argument('--catalog', type=Path, default=pilot.DATA_ROOT / 'tasks.json')
    parser.add_argument('--baseline', type=Path, default=pilot.DATA_ROOT / 'baseline')
    parser.add_argument('--runs-root', type=Path)
    parser.add_argument('--library-root', type=Path)
    parser.add_argument('--native-binary', type=Path)
    parser.add_argument('--provider-config', type=Path)
    parser.add_argument('--skill-approvals', type=Path)
    parser.add_argument('--opencode-config', type=Path)
    parser.add_argument('--channel-model')
    parser.add_argument('--isolated-config-root', type=Path)
    parser.add_argument('--limits', type=Path)
    parser.add_argument('--fixed-policy', type=Path)
    parser.add_argument('--k', type=int, default=1)
    parser.add_argument('--seed', type=int, default=7)
    parser.add_argument('--budget-usd', type=float)
    parser.add_argument('--isolation-label')
    parser.add_argument('--external-spend-control', type=Path)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--output', type=Path)
    return parser


def main(argv=None):
    args = _parser().parse_args(argv)
    try:
        if type(args.k) is not int or args.k <= 0:
            raise ValueError('k must be positive')
        catalog = pilot.load_catalog(args.catalog)
        count = sum(task['split'] == 'optimization' for task in catalog['tasks'])
        required = ('upstream_root', 'runs_root', 'library_root', 'native_binary', 'provider_config',
                    'skill_approvals', 'opencode_config', 'channel_model', 'isolated_config_root',
                    'budget_usd', 'isolation_label', 'external_spend_control')
        missing = [key for key in required if getattr(args, key) is None]
        if not args.execute or missing:
            result = {'schema_version': 1, 'status': 'not_run', 'model': pilot.DEFAULT_MODEL,
                      'upstream_revision': pilot.UPSTREAM_REVISION, 'optimization_tasks': count,
                      'planned_native_rollouts': count * 4 * args.k,
                      'planned_improvement_stages': count * 3 * args.k,
                      'reflection_and_selection_calls': 'additional; inspect upstream plan',
                      'holdout_native_rollouts_default': 48, 'missing_prerequisites': missing,
                      'search_cost_usd': None, 'live_calls': 0}
        else:
            limits = pilot._read_json(args.limits) if args.limits else dict(pilot.DEFAULT_LIMITS)
            policy = args.fixed_policy.read_text(encoding='utf-8') if args.fixed_policy else pilot.FIXED_POLICY
            adapter = build_adapter(
                upstream_root=args.upstream_root, catalog_path=args.catalog,
                baseline_dir=args.baseline, runs_root=args.runs_root,
                native_binary=args.native_binary, provider_config=args.provider_config,
                trusted_skill_approvals=pilot._read_json(args.skill_approvals),
                opencode_config=args.opencode_config, channel_model=args.channel_model,
                isolated_config_root=args.isolated_config_root, limits=limits,
                fixed_policy=policy)
            try:
                result = optimize(adapter, library_root=args.library_root, budget_usd=args.budget_usd,
                                  isolation_label=args.isolation_label,
                                  external_spend_control=args.external_spend_control,
                                  k=args.k, seed=args.seed)
            finally:
                adapter.close()
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            pilot._write_json(args.output, result)
        else:
            print(pilot.canonical_json(result))
        return 0
    except pilot.StudyCancelled:
        print(pilot.canonical_json({'status': 'cancelled', 'native_result_obtained': False}))
        return 130
    except (ValueError, RuntimeError, OSError) as error:
        print('error: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
