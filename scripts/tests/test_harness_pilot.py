import copy
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import harness_pilot as pilot


DATA = Path(__file__).resolve().parents[2] / 'research' / 'harness2'
MODEL = {'id': 'gpt-6.1-sol', 'reasoning_effort': 'max'}
LIMITS = {'timeout_ms': 30000, 'turn_limit': 12, 'max_output_tokens': 2048,
          'max_total_tokens': 24000}
POLICY = 'Read seed inputs. Write only the declared outputs. No external execution.'


def digest(value):
    if not isinstance(value, str):
        value = json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
    return hashlib.sha256(value.encode('utf-8')).hexdigest()


def approvals(directory):
    return {path.parent.name: digest(path.read_text(encoding='utf-8'))
            for path in directory.glob('skills/*/SKILL.md')}


class NativeBoundary:
    def __init__(self, artifacts=None, terminal='end', trace=None, on_call=None):
        self.artifacts = artifacts if artifacts is not None else {
            'answer.json': '{"owner":"Ada","fallback":"Milo"}\n'}
        self.terminal = terminal
        self.trace = trace or []
        self.on_call = on_call
        self.config_identity = digest('explicit-fixture-route-profile-v1')
        self.calls = []

    def __call__(self, request, trial_root, run_id):
        self.calls.append((copy.deepcopy(request), trial_root, run_id))
        trial_root.mkdir()
        workspace = trial_root / 'workspace'
        workspace.mkdir()
        for relative, text in request['seed_files'].items():
            target = workspace / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding='utf-8')
        for relative, text in self.artifacts.items():
            target = workspace / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding='utf-8')
        if self.on_call:
            self.on_call(request, trial_root, run_id)
        return {
            'schema_version': 1, 'backend': 'vega_native_runtime',
            'task_id': request['task_id'], 'run_id': run_id,
            'terminal': self.terminal, 'final_text': 'Finished.',
            'artifacts': copy.deepcopy(self.artifacts), 'trace': copy.deepcopy(self.trace),
            'metrics': {'primary_model_requests': 3, 'tool_proposals': 4,
                        'tool_executions': 3, 'rejected_tool_calls': 1,
                        'repeated_tool_proposals': 1, 'input_tokens': 100,
                        'output_tokens': 20, 'cost_microcents': 1000000,
                        'pricing_known': True, 'usage_complete': True},
            'hashes': {
                'task': digest({key: request[key] for key in ('task_id', 'task_prompt')}),
                'seed': digest(request['seed_files']), 'policy': digest(request['fixed_policy']),
                'harness': digest(request['harness']), 'model': digest(request['model']),
                'instructions': digest('fixed native instructions'), 'request': digest(request),
                'config': self.config_identity},
            'wall_time_ms': 1200, 'failure_category': None}


class PilotTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.catalog_path = self.root / 'tasks.json'
        shutil.copyfile(DATA / 'tasks.json', self.catalog_path)
        self.catalog = pilot.load_catalog(self.catalog_path)
        self.task = copy.deepcopy(self.catalog['tasks'][0])
        self.baseline = self.root / 'baseline'
        self.frozen = self.root / 'frozen'
        shutil.copytree(DATA / 'baseline', self.baseline)
        shutil.copytree(DATA / 'baseline', self.frozen)
        (self.frozen / 'strategy.md').write_text(
            (self.frozen / 'strategy.md').read_text() + 'Verify every output once.\n')
        self.library = self.root / 'library'
        self.library.mkdir()
        (self.library / 'retrieval.json').write_text('{"lessons":["check source revision"]}\n')
        self.trusted = approvals(self.baseline)
        self.boundary = NativeBoundary()

    def run_trial(self, **overrides):
        arguments = dict(task=self.task, harness_dir=self.baseline,
                         runs_root=self.root / 'runs', run_id='sample-1',
                         model=MODEL, limits=LIMITS, fixed_policy=POLICY,
                         trusted_skill_approvals=self.trusted, launcher=self.boundary)
        arguments.update(overrides)
        return pilot.run_trial(**arguments)

    def holdout_arguments(self):
        return dict(catalog_path=self.catalog_path, baseline_dir=self.baseline,
                    frozen_dir=self.frozen, library_dir=self.library, model=MODEL,
                    limits=LIMITS, fixed_policy=POLICY,
                    trusted_skill_approvals=self.trusted,
                    provider_identity=self.boundary.config_identity)

    def plan(self):
        return pilot.freeze_plan(**self.holdout_arguments(), repetitions=3)

    def holdout(self, plan, **overrides):
        arguments = dict(self.holdout_arguments(), plan=plan,
                         runs_root=self.root / 'holdout-runs', launcher=self.boundary)
        arguments.update(overrides)
        return pilot.run_holdout(**arguments)

    def test_catalog_has_fixed_disjoint_balanced_original_tasks(self):
        tasks = self.catalog['tasks']
        self.assertEqual(self.catalog['dataset_version'], 'vega-microtasks-v1')
        self.assertEqual(len(tasks), 24)
        self.assertEqual(len({task['task_id'] for task in tasks}), 24)
        self.assertEqual({task['domain'] for task in tasks},
                         {'retrieval', 'file_edit', 'recovery', 'skills'})
        for domain in ('retrieval', 'file_edit', 'recovery', 'skills'):
            subset = [task for task in tasks if task['domain'] == domain]
            self.assertEqual(sum(task['split'] == 'optimization' for task in subset), 4)
            self.assertEqual(sum(task['split'] == 'holdout' for task in subset), 2)
            for task in subset:
                self.assertEqual(task['source'], 'original-synthetic-vega-microtasks-v1')
                self.assertTrue(task['oracle']['artifacts'])
                self.assertEqual(set(task['writable_paths']), set(task['oracle']['artifacts']))
                self.assertFalse(set(task['seed_files']) & set(task['writable_paths']))

    def test_catalog_rejects_duplicate_split_membership_and_seed_escape(self):
        for mutation in ('duplicate', 'escape'):
            with self.subTest(mutation=mutation):
                document = copy.deepcopy(self.catalog)
                if mutation == 'duplicate':
                    document['tasks'][-1]['task_id'] = document['tasks'][0]['task_id']
                else:
                    document['tasks'][0]['seed_files']['../private.txt'] = 'outside'
                self.catalog_path.write_text(json.dumps(document))
                with self.assertRaises(ValueError):
                    pilot.load_catalog(self.catalog_path)

    def test_public_staging_excludes_private_oracle_and_other_split(self):
        private = copy.deepcopy(self.catalog)
        for task in private['tasks']:
            task['oracle']['private_marker'] = 'PRIVATE_ORACLE_CANARY_8137'
        staged = pilot.stage_public_tasks(private, self.root / 'public', 'optimization')
        expected = {task['task_id'] for task in private['tasks']
                    if task['split'] == 'optimization'}
        self.assertEqual(set(staged), expected)
        self.assertEqual(len(staged), 16)
        for task_id, directory in staged.items():
            files = [path for path in directory.rglob('*') if path.is_file()]
            self.assertTrue(files)
            for path in files:
                self.assertFalse(path.is_symlink())
                text = path.read_text(encoding='utf-8')
                self.assertNotIn('PRIVATE_ORACLE_CANARY_8137', text)
                self.assertNotIn('"oracle"', text)
                for task in private['tasks']:
                    if task['split'] == 'holdout':
                        self.assertNotIn(task['task_id'], text)
            document = json.loads((directory / 'task.json').read_text())
            self.assertEqual(document['task_id'], task_id)
            self.assertEqual(set(document),
                             {'task_id', 'task_prompt', 'seed_files', 'writable_paths'})

    def test_surface_accepts_guidance_and_rejects_executable_or_escape_paths(self):
        tree = pilot.load_harness(self.baseline)
        pilot.validate_tree(tree)
        self.assertIn('strategy.md', tree)
        for relative in ('scripts/run.py', 'skills/record-format/scripts/run.py',
                         'hooks.json', 'config.toml', 'sub_agents/a/AGENT.md',
                         'plugin/a.ts', '../strategy.md', '/strategy.md',
                         'skills/../strategy.md', 'skills\\a\\SKILL.md'):
            with self.subTest(path=relative), self.assertRaises(ValueError):
                pilot.validate_tree(dict(tree, **{relative: 'untrusted'}))

    def test_symlinked_candidate_rejected_before_launch(self):
        outside = self.root / 'outside.md'
        outside.write_text('untrusted external guidance')
        (self.baseline / 'strategy.md').unlink()
        (self.baseline / 'strategy.md').symlink_to(outside)
        with self.assertRaises(ValueError):
            self.run_trial()
        self.assertEqual(self.boundary.calls, [])
        self.assertEqual(outside.read_text(), 'untrusted external guidance')

    def test_off_surface_disk_file_cannot_be_silently_filtered_before_launch(self):
        path = self.baseline / 'scripts' / 'run.py'
        path.parent.mkdir()
        path.write_text('raise RuntimeError("must never execute")\n')
        with self.assertRaises(ValueError):
            self.run_trial()
        self.assertEqual(self.boundary.calls, [])

    def test_stale_skill_approval_is_not_replaced_by_candidate_text(self):
        with self.assertRaises(ValueError):
            self.run_trial(trusted_skill_approvals={})
        path = self.baseline / 'skills' / 'record-format' / 'SKILL.md'
        path.write_text(path.read_text() + 'Candidate claims it is approved.\n')
        with self.assertRaises(ValueError):
            self.run_trial()
        self.assertEqual(self.boundary.calls, [])
        self.assertNotEqual(self.trusted['record-format'], digest(path.read_text()))

    def test_public_native_request_is_bounded_and_preserves_private_seed(self):
        before = copy.deepcopy(self.task)
        run_dir = self.run_trial()
        request, trial_root, run_id = self.boundary.calls[0]
        self.assertEqual(set(request), {
            'schema_version', 'task_id', 'task_prompt', 'seed_files', 'writable_paths',
            'fixed_policy', 'harness', 'model', 'limits', 'trusted_skill_approvals'})
        self.assertEqual(request['model'], MODEL)
        self.assertEqual(request['limits'], LIMITS)
        self.assertEqual(request['trusted_skill_approvals'], self.trusted)
        self.assertEqual(self.task, before)
        self.assertFalse(run_dir.is_relative_to(trial_root))
        self.assertNotIn('oracle', request)
        first_seed = next(iter(before['seed_files']))
        self.assertEqual((trial_root / 'workspace' / first_seed).read_text(),
                         before['seed_files'][first_seed])
        self.assertEqual(pilot.read_run(run_dir)['evaluation']['delivered'], True)

    def test_run_id_reuse_is_idempotent_and_independent_samples_start_clean(self):
        first = self.run_trial()
        self.assertEqual(self.run_trial(), first)
        self.assertEqual(len(self.boundary.calls), 1)
        second = self.run_trial(run_id='sample-2')
        self.assertNotEqual(first, second)
        self.assertEqual(len(self.boundary.calls), 2)
        first_root = self.boundary.calls[0][1]
        second_root = self.boundary.calls[1][1]
        self.assertNotEqual(first_root, second_root)
        seed_path = next(iter(self.task['seed_files']))
        (first_root / 'workspace' / seed_path).write_text('first sample altered')
        self.assertEqual((second_root / 'workspace' / seed_path).read_text(),
                         self.task['seed_files'][seed_path])

    def test_reused_run_id_refuses_input_or_provider_conflict_without_reexecution(self):
        self.run_trial()
        changes = [dict(model={**MODEL, 'reasoning_effort': 'high'}),
                   dict(limits={**LIMITS, 'turn_limit': 9}),
                   dict(fixed_policy=POLICY + 'Changed.'),
                   dict(task={**self.task, 'task_prompt': 'different task'})]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.run_trial(**change)
        self.boundary.config_identity = digest('different-provider-profile')
        with self.assertRaises(ValueError):
            self.run_trial()
        self.assertEqual(len(self.boundary.calls), 1)

    def test_incomplete_existing_run_and_unsafe_run_ids_never_reexecute(self):
        (self.root / 'runs' / 'sample-1').mkdir(parents=True)
        for run_id in ('sample-1', '../outside', '/absolute', 'a/b'):
            with self.subTest(run_id=run_id), self.assertRaises(ValueError):
                self.run_trial(run_id=run_id)
        self.assertEqual(self.boundary.calls, [])

    def test_wrong_or_missing_artifact_does_not_pass_on_natural_end(self):
        for artifacts in ({'answer.json': '{"owner":"Milo","fallback":"Ada"}'},
                          {}, {'answer.json': 'not json'}):
            with self.subTest(artifacts=artifacts):
                boundary = NativeBoundary(artifacts=artifacts)
                result = boundary(self.request_fixture(), self.root / f'wrong-{len(artifacts)}-{len(str(artifacts))}', 'wrong')
                evaluation = pilot.evaluate_result(self.task, result)
                self.assertFalse(evaluation['delivered'])
                self.assertTrue(evaluation['errors'])

    def request_fixture(self):
        return {'schema_version': 1, 'task_id': self.task['task_id'],
                'task_prompt': self.task['task_prompt'], 'seed_files': self.task['seed_files'],
                'writable_paths': self.task['writable_paths'], 'fixed_policy': POLICY,
                'harness': {'strategy': 'Read before write.', 'skills': {}},
                'model': MODEL, 'limits': LIMITS, 'trusted_skill_approvals': {}}

    def test_non_end_native_terminals_never_become_delivery_success(self):
        for terminal in ('length', 'turn_limit', 'cancelled', 'timeout', 'failed',
                         'budget_exhausted'):
            with self.subTest(terminal=terminal):
                result = self.boundary(self.request_fixture(), self.root / terminal, terminal)
                result['terminal'] = terminal
                self.assertFalse(pilot.evaluate_result(self.task, result)['delivered'])

    def test_skill_oracle_requires_real_loaded_event_and_correct_artifact(self):
        task = next(task for task in self.catalog['tasks'] if task['task_id'] == 'decision-record')
        native = {'schema_version': 1, 'backend': 'vega_native_runtime',
                  'task_id': task['task_id'], 'terminal': 'end',
                  'artifacts': {'answer.json': '{"task":"rotate demo queue","decision":"approve","owner":"Ada"}'},
                  'final_text': 'I used record-format successfully.', 'trace': []}
        self.assertFalse(pilot.evaluate_result(task, native)['delivered'])
        native['trace'] = [{'event': 'skill_activation', 'name': 'record-format',
                            'status': 'loaded', 'content_sha256': self.trusted['record-format'],
                            'origin': 'model'}]
        self.assertTrue(pilot.evaluate_result(task, native)['delivered'])
        native['artifacts']['answer.json'] = '{"task":"wrong","decision":"approve","owner":"Ada"}'
        self.assertFalse(pilot.evaluate_result(task, native)['delivered'])

    def test_result_identity_mismatch_is_refused_instead_of_cached(self):
        def wrong_identity(request, trial_root, run_id):
            result = self.boundary(request, trial_root, run_id)
            result['hashes']['model'] = digest({'id': 'other', 'reasoning_effort': 'high'})
            return result
        wrong_identity.config_identity = self.boundary.config_identity
        with self.assertRaises(ValueError):
            self.run_trial(launcher=wrong_identity)

    def test_accounting_includes_failed_trials_and_all_search_roles(self):
        good = {'evaluation': {'delivered': True}, 'native': {
            'terminal': 'end', 'wall_time_ms': 1000, 'metrics': {
                'cost_microcents': 1000000, 'pricing_known': True, 'usage_complete': True}}}
        failed = copy.deepcopy(good)
        failed['evaluation']['delivered'] = False
        failed['native']['terminal'] = 'failed'
        failed['native']['metrics']['cost_microcents'] = 2000000
        search = {'complete': True, 'entries': [
            {'role': role, 'cost_microcents': 1000000, 'pricing_known': True}
            for role in ('proposer', 'selector', 'reflection', 'judge', 'solver')]}
        summary = pilot.aggregate_results([good, failed], search_ledger=search)
        self.assertEqual(summary['trials'], 2)
        self.assertEqual(summary['successes'], 1)
        self.assertEqual(summary['success_rate'], 0.5)
        self.assertAlmostEqual(summary['cost_usd'], 3.0)
        self.assertAlmostEqual(summary['search_cost_usd'], 5.0)
        self.assertAlmostEqual(summary['total_cost_usd'], 8.0)
        self.assertAlmostEqual(summary['cost_per_success_usd'], 3.0)

    def test_unknown_prices_and_no_success_remain_unknown_not_zero(self):
        records = [{'evaluation': {'delivered': False}, 'native': {
            'terminal': 'failed', 'wall_time_ms': 500, 'metrics': {
                'cost_microcents': 0, 'pricing_known': False, 'usage_complete': False}}},
                   {'evaluation': {'delivered': True}, 'native': {
                       'terminal': 'end', 'wall_time_ms': 200, 'metrics': {
                           'cost_microcents': 1000000, 'pricing_known': True,
                           'usage_complete': True}}}]
        summary = pilot.aggregate_results(records, search_ledger=None)
        self.assertEqual(summary['trials'], 2)
        self.assertEqual(summary['unknown_cost_trials'], 1)
        for key in ('cost_usd', 'search_cost_usd', 'total_cost_usd', 'cost_per_success_usd'):
            self.assertIsNone(summary[key])
        self.assertIsNone(pilot.aggregate_results(records[:1])['cost_per_success_usd'])
        self.assertIsNone(pilot.aggregate_results([])['success_rate'])

    def test_holdout_plan_is_truthfully_planned_without_executing_models(self):
        plan = self.plan()
        self.assertEqual(plan['status'], 'planned')
        self.assertEqual(plan['planned_trials'], 48)
        self.assertEqual(self.boundary.calls, [])

    def test_frozen_holdout_runs_only_paired_native_trials_without_library_mutation(self):
        self.boundary.artifacts = {}
        plan = self.plan()
        before = (self.library / 'retrieval.json').read_bytes()
        report = self.holdout(plan)
        self.assertEqual(report['status'], 'completed')
        self.assertEqual(len(report['trials']), 48)
        self.assertEqual(len(self.boundary.calls), 48)
        holdout_ids = {task['task_id'] for task in self.catalog['tasks']
                       if task['split'] == 'holdout'}
        self.assertEqual({request['task_id'] for request, _, _ in self.boundary.calls}, holdout_ids)
        for task_id in holdout_ids:
            trials = [trial for trial in report['trials'] if trial['task_id'] == task_id]
            self.assertEqual({(trial['arm'], trial['repeat']) for trial in trials},
                             {(arm, repeat) for arm in ('baseline', 'frozen') for repeat in range(3)})
        self.assertEqual((self.library / 'retrieval.json').read_bytes(), before)
        self.assertNotIn('harness2.core.schedule', sys.modules)
        self.assertNotIn('harness2.core.proposer', sys.modules)

    def test_frozen_holdout_rejects_model_limits_tasks_or_library_drift_before_launch(self):
        plan = self.plan()
        for change in (dict(model={**MODEL, 'reasoning_effort': 'high'}),
                       dict(limits={**LIMITS, 'turn_limit': 11}),
                       dict(provider_identity=digest('other-route'))):
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.holdout(plan, **change)
        original = self.catalog_path.read_bytes()
        modified = copy.deepcopy(self.catalog)
        modified['tasks'][0]['task_prompt'] += 'Changed source contract.'
        self.catalog_path.write_text(json.dumps(modified))
        with self.assertRaises(ValueError):
            self.holdout(plan)
        self.catalog_path.write_bytes(original)
        (self.library / 'retrieval.json').write_text('{"lessons":["changed"]}')
        with self.assertRaises(ValueError):
            self.holdout(plan)
        self.assertEqual(self.boundary.calls, [])

    def test_frozen_holdout_detects_inflight_tree_change_and_refuses_success_report(self):
        plan = self.plan()
        self.boundary.on_call = lambda *_: (self.frozen / 'strategy.md').write_text('changed during run')
        with self.assertRaises(ValueError):
            self.holdout(plan)
        self.assertEqual(len(self.boundary.calls), 1)


if __name__ == '__main__':
    unittest.main()
