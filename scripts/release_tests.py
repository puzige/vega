import contextlib
import copy
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


class PublicationTransport:
    tag = 'v0.1.63'
    sha = '1' * 40

    def __init__(self, root):
        self.root = root
        self.module = None
        self.record = {
            'id': 101,
            'tag_name': self.tag,
            'draft': True,
            'prerelease': False,
            'assets': [],
        }
        self.after_refresh = {}
        self.metadata_reads = 0
        self.uploads = []
        self.publications = []
        self.events = []

    def git_output(self, *argv):
        self.events.append(('run', argv))
        if argv == ('git', 'rev-parse', 'HEAD'):
            return self.sha
        if argv == ('git', 'fetch', 'origin', 'master', '--tags'):
            return ''
        if argv == ('git', 'rev-parse', '--verify', f'refs/tags/{self.tag}^{{commit}}'):
            return self.sha
        raise AssertionError('Unexpected Git output boundary')

    def api(self, path, method='GET', data=None):
        self.events.append(('api', path, method))
        if path == 'releases?per_page=100&page=1' and method == 'GET':
            return [copy.deepcopy(self.record)]
        if path == 'releases/101' and method == 'GET':
            self.metadata_reads += 1
            if self.metadata_reads == 2:
                self.record.update(copy.deepcopy(self.after_refresh))
            return copy.deepcopy(self.record)
        if path == 'releases/101' and method == 'PATCH':
            self.publications.append(copy.deepcopy(data))
            self.record.update(copy.deepcopy(data))
            return copy.deepcopy(self.record)
        raise AssertionError('Unexpected GitHub API boundary')

    def subprocess_run(self, argv, **kwargs):
        if argv == ['git', 'merge-base', '--is-ancestor', self.sha, 'origin/master'] and not kwargs:
            self.events.append(('ancestor',))
            return subprocess.CompletedProcess(argv, 0)
        upload = ['gh', 'release', 'upload', self.tag, '--repo', 'fixture/vega', '--clobber',
                  *(str(Path('dist') / name) for name in self.module.ASSETS)]
        if argv == upload and kwargs == {'check': True}:
            self.events.append(('upload',))
            self.uploads.append(list(argv))
            self.record['assets'] = [
                {'name': name, 'state': 'uploaded', 'size': (self.root / 'dist' / name).stat().st_size}
                for name in self.module.ASSETS
            ]
            return subprocess.CompletedProcess(argv, 0)
        raise AssertionError('Unexpected subprocess boundary')


class ReleasePublicationGuardTests(unittest.TestCase):
    def setUp(self):
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        directory = self.stack.enter_context(tempfile.TemporaryDirectory(prefix='vega-release-tag-guard-'))
        self.root = Path(directory)
        original_cwd = Path.cwd()
        self.stack.callback(os.chdir, original_cwd)
        os.chdir(self.root)
        self.stack.enter_context(patch.object(os, 'environ', {
            'GITHUB_REPOSITORY': 'fixture/vega',
            'RELEASE_TAG': PublicationTransport.tag,
            'RELEASE_SHA': PublicationTransport.sha,
        }))
        self.stack.callback(setattr, sys, 'dont_write_bytecode', sys.dont_write_bytecode)
        sys.dont_write_bytecode = True
        self.stack.enter_context(patch.object(subprocess, 'run', side_effect=self.reject_external_execution))
        self.stack.enter_context(patch.object(subprocess, 'check_output', side_effect=self.reject_external_execution))
        self.stack.enter_context(patch.object(subprocess, 'Popen', side_effect=self.reject_external_execution))
        source = Path(__file__).resolve().with_name('release.py')
        spec = importlib.util.spec_from_file_location('release_under_test', source)
        self.release = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.release)
        self.transport = PublicationTransport(self.root)
        self.transport.module = self.release
        self.stack.enter_context(patch.object(self.release, 'run', side_effect=self.transport.git_output))
        self.stack.enter_context(patch.object(self.release, 'api', side_effect=self.transport.api))
        self.stack.enter_context(patch.object(subprocess, 'run', side_effect=self.transport.subprocess_run))
        self.stdout = self.stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
        (self.root / 'dist').mkdir()
        for name in self.release.ASSETS:
            (self.root / 'dist' / name).write_bytes(b'owned release asset\n')

    def reject_external_execution(self, *args, **kwargs):
        raise AssertionError('External execution is forbidden in publication UNIT tests')

    def assert_publication_rejected(self, expected_tag, expected_draft):
        error = None
        try:
            self.release.publish()
        except RuntimeError as caught:
            error = caught
        self.assertEqual(self.transport.metadata_reads, 2)
        self.assertEqual(len(self.transport.uploads), 1)
        self.assertEqual(self.transport.record['tag_name'], expected_tag)
        self.assertEqual(self.transport.publications, [], 'Changed refreshed identity must not publish')
        self.assertEqual(self.transport.record['draft'], expected_draft)
        self.assertTrue(self.release.complete(self.transport.record))
        self.assertIsInstance(error, RuntimeError)
        self.assertEqual(str(error), 'Draft asset upload incomplete or release changed externally')
        self.assertEqual(self.stdout.getvalue(), '')

    def test_sig08_tag_change_after_upload_is_rejected(self):
        self.transport.after_refresh = {'tag_name': 'v0.1.64'}
        self.assert_publication_rejected('v0.1.64', True)

    def test_sig08_draft_change_after_upload_is_rejected(self):
        self.transport.after_refresh = {'draft': False}
        self.assert_publication_rejected(self.transport.tag, False)

    def test_sig08_unchanged_identity_publishes_once(self):
        self.release.publish()
        self.assertEqual(self.transport.metadata_reads, 2)
        self.assertEqual(len(self.transport.uploads), 1)
        self.assertEqual(self.transport.publications, [{
            'draft': False, 'prerelease': False, 'make_latest': 'true',
        }])
        self.assertEqual(self.transport.record['tag_name'], self.transport.tag)
        self.assertFalse(self.transport.record['draft'])
        self.assertTrue(self.release.complete(self.transport.record))
        self.assertEqual(self.transport.events[-1], ('api', 'releases/101', 'PATCH'))
        self.assertEqual(self.stdout.getvalue(), f'Published {self.transport.tag}\n')


if __name__ == '__main__':
    unittest.main(verbosity=2)
