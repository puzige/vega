"""Regression cases for shadow selection; no Cargo build required."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("selection", Path(__file__).with_name("test_selection.py"))
selection = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(selection)


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.root = Path('/fixture')
        self.metadata = {
            'workspace_root': str(self.root), 'workspace_members': ['core', 'service', 'app', 'other'],
            'packages': [{'id': n, 'name': n, 'manifest_path': f'/fixture/crates/{n}/Cargo.toml'}
                         for n in ['core', 'service', 'app', 'other']],
            'resolve': {'nodes': [{'id': n, 'dependencies': d} for n, d in
                                 [('core', []), ('service', ['core']), ('app', ['service']), ('other', [])]]},
        }
        self.inventory = {'test-count': 3, 'rust-suites': {'service': {
            'package-name': 'service', 'status': 'listed',
            'testcases': {n: {'ignored': False, 'filter-match': {'status': 'matches'}} for n in ['slow', 'representative', 'new_test']}}}}
        self.rules = {'version': 1, 'groups': [{'id': 'faults', 'protects': 'safe mutation',
            'package': 'service', 'tests': ['slow'], 'affected_crates': ['service'],
            'trigger_paths': ['crates/app/src/commit/']}]}

    def analyze(self, changes):
        return selection.analyze(self.metadata, self.inventory, self.rules, changes)

    def test_transitive_reverse_dependencies(self):
        r = self.analyze([{'status': 'M', 'paths': ['crates/core/src/lib.rs']}])
        self.assertEqual(r['affected_crates'], ['app', 'core', 'service'])
        self.assertEqual(r['candidate_skip_count'], 0)

    def test_unlisted_and_new_tests_always_run(self):
        r = self.analyze([{'status': 'M', 'paths': ['crates/other/src/lib.rs']}])
        self.assertEqual(r['candidate_skip_count'], 1)
        self.assertEqual(r['would_run_count'], 2)
        self.assertEqual(r['actual_execution'], 'full')

    def test_explicit_caller_trigger(self):
        r = self.analyze([{'status': 'M', 'paths': ['crates/app/src/commit/handler.rs']}])
        self.assertEqual(r['candidate_skip_count'], 0)
        self.assertTrue(any('explicit' in x for x in r['groups'][0]['reasons']))

    def test_unknown_config_and_delete_rename_fail_full(self):
        for status, paths in [('M', ['mystery.txt']), ('M', ['Cargo.lock']),
                ('M', ['crates/other/Cargo.toml']), ('M', ['.github/ci/test_selection.py']),
                ('D', ['crates/core/src/old.rs']),
                ('R100', ['crates/core/src/old.rs', 'crates/other/src/new.rs'])]:
            with self.subTest(status=status, paths=paths):
                r = self.analyze([{'status': status, 'paths': paths}])
                self.assertTrue(r['fallback_reasons'])
                self.assertEqual(r['candidate_skip_count'], 0)
                self.assertEqual(r['changes'][0]['paths'], paths)

    def test_empty_diff_fails_full(self):
        self.assertTrue(self.analyze([])['fallback_reasons'])

    def test_stale_rule_and_ignored_rule_fail_full(self):
        for mutation in ['rename', 'ignored']:
            with self.subTest(mutation=mutation):
                inventory = copy.deepcopy(self.inventory)
                if mutation == 'rename':
                    inventory['rust-suites']['service']['testcases']['renamed'] = inventory['rust-suites']['service']['testcases'].pop('slow')
                else:
                    inventory['rust-suites']['service']['testcases']['slow']['ignored'] = True
                    inventory['rust-suites']['service']['testcases']['slow']['filter-match'] = {'status': 'mismatch', 'reason': 'ignored'}
                with self.assertRaises(ValueError):
                    selection.analyze(self.metadata, inventory, self.rules, [])

    def test_always_run_representative_and_duplicate_rules(self):
        self.rules['groups'][0]['always_run'] = True
        result = self.analyze([{'status': 'M', 'paths': ['crates/other/src/lib.rs']}])
        self.assertEqual(result['candidate_skip_count'], 0)
        self.rules['groups'][0]['tests'].append('slow')
        with self.assertRaises(ValueError):
            self.analyze([])

    def test_filtered_inventory_and_unknown_rule_keys_are_rejected(self):
        self.inventory['rust-suites']['service']['testcases']['new_test']['filter-match'] = {'status': 'mismatch', 'reason': 'expression'}
        with self.assertRaises(ValueError):
            self.analyze([])
        self.inventory['rust-suites']['service']['testcases']['new_test']['filter-match'] = {'status': 'matches'}
        self.rules['groups'][0]['always-run'] = True
        with self.assertRaises(ValueError):
            self.analyze([])

    def test_incomplete_external_dependency_graph_is_rejected(self):
        self.metadata['resolve']['nodes'][0]['dependencies'] = ['missing-external']
        with self.assertRaises(ValueError):
            self.analyze([])

    def test_bad_metadata_inventory_and_rules_are_rejected(self):
        for target, field, value in [('metadata', 'resolve', None), ('inventory', 'test-count', 999),
                                     ('rules', 'version', 2), ('rules', 'groups', [])]:
            args = [copy.deepcopy(self.metadata), copy.deepcopy(self.inventory), copy.deepcopy(self.rules), []]
            args[['metadata', 'inventory', 'rules'].index(target)][field] = value
            with self.subTest(target=target, field=field), self.assertRaises((ValueError, TypeError)):
                selection.analyze(*args)

    def test_git_diff_preserves_both_rename_paths_and_unusual_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            def git(*args):
                return subprocess.check_output(['git', '-C', tmp, *args])
            git('init', '-q'); git('config', 'user.email', 'test@example.invalid'); git('config', 'user.name', 'Test')
            old = 'old\n`name`.rs'; new = 'new\tname.rs'
            (root / old).write_text('unique content\n')
            git('add', '.'); git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD').decode().strip()
            git('mv', old, new); git('commit', '-qm', 'rename')
            head = git('rev-parse', 'HEAD').decode().strip()
            self.assertEqual(selection.read_changes(root, base, head), [{'status': 'R100', 'paths': [old, new]}])
            with self.assertRaises(ValueError):
                selection.read_changes(root, '--bad-ref', head)

    def test_markdown_escapes_untrusted_paths(self):
        r = self.analyze([{'status': 'M', 'paths': ['docs/<script>\n`x`.md']}])
        text = selection.summary(r)
        self.assertNotIn('<script>', text)
        self.assertNotIn('```', text)

    def test_cli_failure_writes_full_fallback_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'report.json'
            result = subprocess.run(['python3', str(Path(selection.__file__)), '--base', 'bad', '--head', 'bad', '--output', str(path)], capture_output=True)
            self.assertEqual(result.returncode, 0)
            report = json.loads(path.read_text())
            self.assertEqual(report['actual_execution'], 'full')
            self.assertEqual(report['candidate_skip_count'], 0)
            self.assertTrue(report['fallback_reasons'])


if __name__ == '__main__':
    unittest.main()
