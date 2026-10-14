import os
from pathlib import Path
import stat
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import delta_update
import release


def archive(path, files):
    with zipfile.ZipFile(path, 'w', zipfile.ZIP_DEFLATED) as output:
        for name, (data, mode) in files.items():
            entry = zipfile.ZipInfo('Vega.app/' + name)
            entry.create_system = 3
            entry.external_attr = (stat.S_IFREG | mode) << 16
            entry.compress_type = zipfile.ZIP_DEFLATED
            output.writestr(entry, data)


class DeltaTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.old = self.root / 'old.zip'
        self.full = self.root / delta_update.ARCHIVE
        self.files = {'Contents/MacOS/vega': (b'old', 0o755),
                      'Contents/Info.plist': (b'info', 0o644),
                      'Contents/Resources/runtime': (bytes(range(256)) * 100, 0o755),
                      'Contents/Resources/deleted': (b'deleted', 0o644)}
        archive(self.old, self.files)

    def test_changed_added_deleted_and_mode_only(self):
        target = dict(self.files)
        target['Contents/MacOS/vega'] = (b'new', 0o755)
        target['Contents/Resources/added'] = (b'added', 0o644)
        target['Contents/Resources/runtime'] = (target['Contents/Resources/runtime'][0], 0o644)
        del target['Contents/Resources/deleted']
        archive(self.full, target)
        manifest = delta_update.generate(self.root, '1.0.3', [('1.0.1', self.old)])
        self.assertEqual(len(manifest['patches']), 1)
        candidate = manifest['patches'][0]
        self.assertEqual(candidate['from_version'], '1.0.1')
        self.assertEqual(candidate['files'], ['Contents/MacOS/vega', 'Contents/Resources/added'])
        self.assertEqual(manifest['full_sha256'], delta_update.digest(self.full))
        self.assertNotIn('Contents/Resources/deleted', [item['path'] for item in manifest['files']])
        self.assertEqual(next(item for item in manifest['files'] if item['path'].endswith('runtime'))['mode'], 0o644)
        delta_update.reconstruct(delta_update.inventory(self.full), self.old,
                                 self.root / candidate['archive'], candidate['files'])

    def test_empty_and_five_base_limit(self):
        archive(self.full, self.files)
        self.assertEqual(delta_update.generate(self.root, '1.0.9', [])['patches'], [])
        bases = [(f'1.0.{index}', self.old) for index in range(1, 8)]
        self.assertEqual(len(delta_update.generate(self.root, '1.0.9', bases)['patches']), 5)

    def test_oversized_patch_is_discarded(self):
        archive(self.full, self.files)
        with patch.object(delta_update, 'MAX_ARCHIVE', 1):
            manifest = delta_update.generate(self.root, '1.0.2', [('1.0.1', self.old)])
        self.assertEqual(manifest['patches'], [])
        self.assertFalse((self.root / 'Vega-delta-1.0.1-to-1.0.2.zip').exists())

    def test_invalid_paths(self):
        for paths in [['Contents/../bad'], ['Contents/a', 'Contents/A'],
                      ['Contents/a', 'Contents/a/b'], ['Contents/foo/a', 'Contents/Foo/b'], ['Contents/é'], ['Contents/a:b'],
                      ['Contents/a\n'], ['other/file'], ['Contents//file'], ['Contents/\\bad']]:
            with self.subTest(paths=paths), self.assertRaises(ValueError):
                delta_update.validate_paths(paths)

    def test_unsupported_target_falls_back(self):
        self.files['Contents/Resources/é'] = (b'x', 0o644)
        archive(self.full, self.files)
        manifest = delta_update.generate(self.root, '1.0.2', [('1.0.1', self.old)])
        self.assertEqual(manifest['files'], [])
        self.assertEqual(manifest['patches'], [])
        self.assertEqual(manifest['full_sha256'], delta_update.digest(self.full))

    def test_malformed_current_bundle_fails_publication(self):
        archive(self.full, {'Contents/Info.plist': (b'info', 0o644)})
        with self.assertRaises(ValueError):
            delta_update.generate(self.root, '1.0.2', [])

    def test_special_files_and_limits_rejected(self):
        archive(self.full, self.files)
        with zipfile.ZipFile(self.full, 'a') as output:
            entry = zipfile.ZipInfo('Vega.app/Contents/link')
            entry.external_attr = (stat.S_IFLNK | 0o755) << 16
            output.writestr(entry, '/tmp/outside')
        with self.assertRaises(ValueError):
            delta_update.inventory(self.full)
        with patch.object(delta_update, 'MAX_EXPANDED', 1), self.assertRaises(ValueError):
            delta_update.inventory(self.old)

    def test_reconstruction_detects_tampering(self):
        archive(self.full, self.files)
        manifest = delta_update.generate(self.root, '1.0.2', [('1.0.1', self.old)])
        candidate = manifest['patches'][0]
        target = delta_update.inventory(self.full)
        self.files['Contents/MacOS/vega'] = (b'tampered', 0o755)
        archive(self.old, self.files)
        with self.assertRaises(ValueError):
            delta_update.reconstruct(target, self.old, self.root / candidate['archive'], [])


class PublicationTests(unittest.TestCase):
    def release(self, names):
        return dict(tag_name='v1.0.2', prerelease=False, draft=True,
                    assets=[dict(name=name, state='uploaded', size=1) for name in names])

    def test_draft_requires_delta_and_referenced_patch(self):
        manifest = dict(version='1.0.2', patches=[dict(from_version='1.0.1', archive='Vega-delta-1.0.1-to-1.0.2.zip', size=1)])
        self.assertFalse(release.complete(self.release(release.SIGNED_ASSETS), manifest=manifest))
        self.assertFalse(release.complete(self.release(release.ASSETS), manifest=manifest))
        self.assertTrue(release.complete(self.release((*release.ASSETS, manifest['patches'][0]['archive'])), manifest=manifest))
        self.assertTrue(release.complete(self.release(release.ASSETS), manifest=dict(version='1.0.2', patches=[])))

    def test_completion_binds_version_and_patch_size(self):
        manifest = dict(version='1.0.2', patches=[dict(from_version='1.0.1',
                        archive='Vega-delta-1.0.1-to-1.0.2.zip', size=2)])
        record = self.release((*release.ASSETS, manifest['patches'][0]['archive']))
        self.assertFalse(release.complete(record, manifest=manifest))
        manifest['patches'][0]['size'] = 1
        manifest['version'] = '1.0.3'
        self.assertFalse(release.complete(record, manifest=manifest))

    def test_missing_delta_metadata_never_downloads(self):
        with patch.object(release.subprocess, 'run') as run:
            self.assertFalse(release.complete(self.release(release.SIGNED_ASSETS)))
        run.assert_not_called()

    def test_historical_completion(self):
        with patch.object(release, 'run', return_value='assets/update-public-key.hex'):
            self.assertTrue(release.complete(self.release(release.SIGNED_ASSETS), allow_legacy=True))
        with patch.object(release, 'run', return_value=''):
            self.assertTrue(release.complete(self.release(release.LEGACY_ASSETS), allow_legacy=True))

    def test_generator_selects_five_older_official_bases_and_skips_missing(self):
        items = [dict(tag_name=f'v1.0.{index}', draft=False, prerelease=False, assets=[])
                 for index in range(1, 9)]
        items += [dict(tag_name='v1.0.9', draft=True, prerelease=False, assets=[]),
                  dict(tag_name='v1.0.10', draft=False, prerelease=True, assets=[])]
        with patch.dict(os.environ, RELEASE_TAG='v1.0.9'), \
                patch.object(release, 'releases', return_value=items), \
                patch.object(release.subprocess, 'run') as run, \
                patch.object(release.delta_update, 'generate') as generate, \
                patch('builtins.print') as report:
            release.generate_delta()
        run.assert_called_once_with(['cargo', 'xtask', 'verify-update', 'dist', '1.0.9'], check=True)
        generate.assert_called_once_with(Path('dist'), '1.0.9', [])
        self.assertEqual(report.call_count, 5)
        self.assertIn('v1.0.8', report.call_args_list[0].args[0])
        self.assertIn('v1.0.4', report.call_args_list[-1].args[0])

    def test_asset_name_cannot_escape_dist(self):
        with self.assertRaises(ValueError):
            release.delta_assets(dict(version='1.0.2', patches=[dict(from_version='1.0.1', archive='../escape')]))


if __name__ == '__main__':
    unittest.main()
