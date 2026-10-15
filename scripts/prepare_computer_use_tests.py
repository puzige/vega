import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import pathlib
import tarfile
import tempfile
import unittest
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "prepare_computer_use", ROOT / "scripts/prepare-computer-use.py"
)
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)
SIGN_SPEC = importlib.util.spec_from_file_location(
    "sign_computer_use", ROOT / "scripts/sign-computer-use.py"
)
sign = importlib.util.module_from_spec(SIGN_SPEC)
SIGN_SPEC.loader.exec_module(sign)


class ComputerUsePackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)

    def archive(self, entries):
        path = self.root / "source.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            for name, payload, kind in entries:
                member = tarfile.TarInfo(name)
                member.type = kind
                if kind == tarfile.REGTYPE:
                    member.size = len(payload)
                    archive.addfile(member, io.BytesIO(payload))
                else:
                    member.linkname = payload.decode()
                    archive.addfile(member)
        return path

    def run_preparation(self, *arguments):
        def fetch(asset, cache):
            cache.mkdir(parents=True, exist_ok=True)
            path = cache / asset["sha256"]
            path.write_text("fixture asset")
            return path

        def extract(archive, destination, selections):
            for name in selections:
                path = destination / name
                if name.endswith("/"):
                    path.mkdir(parents=True)
                else:
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("fixture executable or license")

        def install(command, *, cwd, env, check, timeout):
            self.assertIn("--ignore-scripts", command)
            self.assertTrue(check)
            self.assertEqual(env["PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD"], "1")
            for name, expected in prepare.NPM_PACKAGES.items():
                directory = cwd / "node_modules" / name
                directory.mkdir(parents=True)
                (directory / "package.json").write_text(json.dumps({
                    "name": name, "version": expected["version"],
                }))
                (directory / "LICENSE").write_text("fixture license")

        with contextlib.ExitStack() as stack:
            for target, attribute, options in [
                (prepare, "ROOT", {"new": self.root}),
                (prepare.platform, "system", {"return_value": "Darwin"}),
                (prepare.platform, "machine", {"return_value": "arm64"}),
                (prepare, "fetch_asset", {"side_effect": fetch}),
                (prepare, "extract_selected", {"side_effect": extract}),
                (prepare.subprocess, "run", {"side_effect": install}),
                (prepare.sys, "argv", {"new": ["prepare-computer-use.py", *map(str, arguments)]}),
            ]:
                stack.enter_context(mock.patch.object(target, attribute, **options))
            stack.enter_context(contextlib.redirect_stdout(io.StringIO()))
            prepare.main()

    def test_default_preparation_ignores_restored_cargo_directories_and_repeats(self):
        legacy = self.root / "target/computer-use/Contents"
        (legacy / "Helpers").mkdir(parents=True)
        (legacy / "Resources/computer-use").mkdir(parents=True)
        sentinel = legacy / "keep"
        sentinel.write_bytes(b"unowned content")
        before = sorted(str(path.relative_to(legacy)) for path in legacy.rglob("*"))
        output = self.root / ".vega-build/computer-use/Contents"
        cache = self.root / ".vega-build/computer-use/downloads"
        for _ in range(2):
            self.run_preparation()
            prepare.validate_output_directory(output)
            self.assertEqual((output / ".vega-computer-use-prepared").read_text(),
                             "vega-computer-use:1\n")
            self.assertEqual((output / "Helpers/node").stat().st_mode & 0o777, 0o755)
            manifest = json.loads((output / "Resources/computer-use/manifest.json").read_text())
            self.assertEqual(manifest["executors"], prepare.EXPECTED_EXECUTORS)
            self.assertEqual(len(list(cache.iterdir())), 4)
            self.assertEqual(sorted(str(path.relative_to(legacy)) for path in legacy.rglob("*")), before)
            self.assertEqual(sentinel.read_bytes(), b"unowned content")
            self.assertFalse((legacy / ".vega-computer-use-prepared").exists())
        self.assertFalse((self.root / "target/computer-use/downloads").exists())

    def test_preparation_honors_custom_output_and_cache(self):
        output = self.root / "custom/Contents"
        cache = self.root / "custom-downloads"
        self.run_preparation("--output", output, "--cache", cache)
        prepare.validate_output_directory(output)
        self.assertTrue((output / "Helpers/cua-driver").is_file())
        self.assertEqual(len(list(cache.iterdir())), 4)
        self.assertFalse((self.root / ".vega-build").exists())

    def test_default_preparation_preserves_untrusted_output(self):
        output = self.root / ".vega-build/computer-use/Contents"
        output.mkdir(parents=True)
        sentinel = output / "personal-file"
        sentinel.write_bytes(b"keep")
        marker = output / ".vega-computer-use-prepared"
        for value, message in [(None, "unowned"), ("other", "marker mismatch"),
                               ("vega-computer-use:1\n", "unexpected contents")]:
            with self.subTest(marker=value):
                if value is not None:
                    marker.write_text(value)
                with self.assertRaisesRegex(ValueError, message):
                    self.run_preparation()
                self.assertEqual(sentinel.read_bytes(), b"keep")
                self.assertFalse((output.parent / "downloads").exists())
        marker.unlink()
        marker.symlink_to(sentinel)
        with self.assertRaisesRegex(ValueError, "unowned"):
            self.run_preparation()
        output.rename(output.with_name("original"))
        output.symlink_to(output.with_name("original"), target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symbolic link"):
            self.run_preparation()
        self.assertEqual(sentinel.read_bytes(), b"keep")

    def test_archive_extracts_only_requested_files(self):
        path = self.archive([
            ("bundle/bin/node", b"node", tarfile.REGTYPE),
            ("bundle/bin/npm", b"../lib/npm.js", tarfile.SYMTYPE),
        ])
        output = self.root / "extract"
        prepare.extract_selected(path, output, ["bundle/bin/node"])
        self.assertEqual((output / "bundle/bin/node").read_bytes(), b"node")
        self.assertFalse((output / "bundle/bin/npm").exists())

    def test_archive_rejects_traversal_absolute_and_backslash_names(self):
        for name in ["../escape", "/escape", "bundle/../../escape", "bundle\\escape"]:
            with self.subTest(name=name):
                path = self.archive([(name, b"bad", tarfile.REGTYPE)])
                with self.assertRaisesRegex(ValueError, "archive path"):
                    prepare.extract_selected(path, self.root / "extract", ["bundle/"])
                self.assertFalse((self.root / "escape").exists())

    def test_archive_rejects_selected_links_and_duplicate_members(self):
        for kind in [tarfile.SYMTYPE, tarfile.LNKTYPE]:
            with self.subTest(kind=kind):
                path = self.archive([("bundle/node", b"outside", kind)])
                with self.assertRaisesRegex(ValueError, "archive member type"):
                    prepare.extract_selected(path, self.root / "extract", ["bundle/"])
        path = self.archive([
            ("bundle/node", b"one", tarfile.REGTYPE),
            ("bundle/node", b"two", tarfile.REGTYPE),
        ])
        with self.assertRaisesRegex(ValueError, "duplicate archive path"):
            prepare.extract_selected(path, self.root / "duplicate", ["bundle/"])

    def test_archive_rejects_size_and_count_limits(self):
        path = self.archive([
            ("bundle/one", b"1234", tarfile.REGTYPE),
            ("bundle/two", b"5678", tarfile.REGTYPE),
        ])
        for limits in [dict(max_file_bytes=3), dict(max_total_bytes=7), dict(max_members=1)]:
            with self.subTest(limits=limits), self.assertRaisesRegex(ValueError, "archive limit"):
                prepare.extract_selected(path, self.root / "extract", ["bundle/"], **limits)

    def test_hash_mismatch_and_oversize_never_accept_cache(self):
        path = self.root / "archive"
        path.write_bytes(b"wrong")
        asset = {"sha256": hashlib.sha256(b"right").hexdigest(), "max_bytes": 100}
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            prepare.verify_download(path, asset)
        asset["sha256"] = hashlib.sha256(b"wrong").hexdigest()
        asset["max_bytes"] = 4
        with self.assertRaisesRegex(ValueError, "download limit"):
            prepare.verify_download(path, asset)

    def test_contract_has_fixed_versions_paths_and_official_sources(self):
        contract = prepare.load_contract(ROOT / "packaging/computer-use")
        self.assertEqual(contract["executors"]["cua"]["entrypoint"], "Helpers/cua-driver")
        self.assertEqual(contract["executors"]["node"]["version"], "24.21.0")
        for mutate in [
            lambda c: c.update(schema_version=2),
            lambda c: c["executors"]["cua"].update(version="latest"),
            lambda c: c["executors"]["node"].update(entrypoint="../node"),
            lambda c: c["downloads"]["node"].update(url="http://nodejs.org/node.tar.gz"),
        ]:
            candidate = copy.deepcopy(contract)
            mutate(candidate)
            with self.assertRaises(ValueError):
                prepare.validate_contract(candidate)

    def test_lock_rejects_version_integrity_url_and_package_drift(self):
        directory = ROOT / "packaging/computer-use/playwright"
        package = json.loads((directory / "package.json").read_text())
        lock = json.loads((directory / "package-lock.json").read_text())
        prepare.validate_npm_lock(package, lock)
        for field, value in [
            ("version", "0.0.84"),
            ("integrity", "sha512-invalid"),
            ("resolved", "https://example.org/mcp.tgz"),
            ("hasInstallScript", True),
        ]:
            with self.subTest(field=field):
                bad = copy.deepcopy(lock)
                bad["packages"]["node_modules/@playwright/mcp"][field] = value
                with self.assertRaises(ValueError):
                    prepare.validate_npm_lock(package, bad)
        extra = copy.deepcopy(lock)
        extra["packages"]["node_modules/unexpected"] = {}
        with self.assertRaises(ValueError):
            prepare.validate_npm_lock(package, extra)
        package["scripts"] = {"postinstall": "do-something"}
        with self.assertRaises(ValueError):
            prepare.validate_npm_lock(package, lock)

    def test_npm_command_disables_scripts_and_browser_downloads(self):
        command, env = prepare.npm_install_command(self.root, self.root / "node")
        self.assertEqual(command[0], str(self.root / "node/bin/node"))
        self.assertIn("ci", command)
        self.assertIn("--ignore-scripts", command)
        self.assertIn("--omit=dev", command)
        self.assertIn("--no-bin-links", command)
        self.assertEqual(env["PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD"], "1")
        self.assertNotIn("NODE_OPTIONS", env)
        self.assertNotEqual(env["npm_config_userconfig"], env["npm_config_globalconfig"])
        self.assertNotIn("Resources", env["npm_config_cache"])

    def test_output_replacement_preserves_unowned_directories(self):
        output = self.root / "Contents"
        prepare.validate_output_directory(output)
        output.mkdir()
        (output / "personal-file").write_text("keep")
        with self.assertRaisesRegex(ValueError, "unowned"):
            prepare.validate_output_directory(output)
        self.assertEqual((output / "personal-file").read_text(), "keep")
        (output / ".vega-computer-use-prepared").write_text("vega-computer-use:1\n")
        with self.assertRaisesRegex(ValueError, "unexpected contents"):
            prepare.validate_output_directory(output)

    def test_runtime_manifest_uses_contents_relative_paths_and_source_hashes(self):
        directory = ROOT / "packaging/computer-use"
        contract = prepare.load_contract(directory)
        manifest = prepare.runtime_manifest(contract, directory / "playwright/package-lock.json")
        self.assertEqual(manifest["schema_version"], 1)
        self.assertEqual(manifest["minimum_macos"], "14.0")
        self.assertEqual(manifest["executors"]["playwright_mcp"]["entrypoint"],
                         "Resources/computer-use/playwright/cli.js")
        self.assertEqual(manifest["sources"]["npm_lock_sha256"],
                         prepare.sha256_file(directory / "playwright/package-lock.json"))

    def test_signs_nested_helpers_before_app_and_jit_entitlement_only_on_node(self):
        commands = sign.signing_commands(self.root / "Vega.app", "IDENTITY", self.root / "jit.plist", "KEYCHAIN")
        signing = [command for command in commands if "--sign" in command]
        self.assertEqual([pathlib.Path(c[-1]).name for c in signing], ["cua-driver", "node", "Vega.app"])
        self.assertNotIn("--deep", [arg for command in signing for arg in command])
        self.assertNotIn("--entitlements", signing[0])
        self.assertIn("--entitlements", signing[1])
        self.assertNotIn("--entitlements", signing[2])
        self.assertTrue(all("--timestamp" in command for command in signing))
        self.assertTrue(all("--keychain" in command for command in signing))
        self.assertEqual(commands[-1][:4], ["codesign", "--verify", "--deep", "--strict"])
        adhoc = sign.signing_commands(self.root / "Vega.app", "-", self.root / "jit.plist")
        self.assertTrue(all("--timestamp" not in c for c in adhoc))

    def test_signing_inventory_rejects_hidden_native_code_and_symlinks(self):
        contents = self.root / "Vega.app/Contents"
        for name in ["Helpers/cua-driver", "Helpers/node", "MacOS/vega"]:
            path = contents / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(bytes.fromhex("cffaedfe") + b"fixture")
        sign.validate_inventory(contents.parent)
        extra = contents / "Resources/hidden.node"
        extra.parent.mkdir()
        extra.write_bytes(bytes.fromhex("cffaedfe"))
        with self.assertRaisesRegex(ValueError, "unexpected native"):
            sign.validate_inventory(contents.parent)
        extra.unlink()
        extra.symlink_to(contents / "Helpers/node")
        with self.assertRaisesRegex(ValueError, "symbolic link"):
            sign.validate_inventory(contents.parent)


if __name__ == "__main__":
    unittest.main()
