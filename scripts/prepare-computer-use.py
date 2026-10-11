#!/usr/bin/env python3
import argparse
import base64
import copy
import hashlib
import json
import os
import pathlib
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse


ROOT = pathlib.Path(__file__).resolve().parents[1]
PACKAGE_ROOT = ROOT / "packaging/computer-use"
EXPECTED_EXECUTORS = {
    "cua": {"version": "0.34.0", "entrypoint": "Helpers/cua-driver"},
    "node": {"version": "24.21.0", "entrypoint": "Helpers/node"},
    "playwright_mcp": {
        "version": "0.0.83",
        "entrypoint": "Resources/computer-use/playwright/cli.js",
    },
}
PLAYWRIGHT_VERSION = "1.64.0-alpha-1790635538000"
NPM_PACKAGES = {
    "@playwright/mcp": {
        "version": "0.0.83",
        "integrity": "sha512-oNcl+Ae2/IAjhfPeP46BfIkSakfmprY+aOtkv5MjrQ4lPav4/yNtPhL0iq8SlIM90oApWgBDUxaNKvktazUKOg==",
    },
    "playwright": {
        "version": PLAYWRIGHT_VERSION,
        "integrity": "sha512-/5XDUMxpOd/9AojJtWDFIRV3EJZNQ0AwvK69wgt3CPzy8wTgiBG9pX45VfxtqtvIRGwfWzMhrvawNKreG69Snw==",
    },
    "playwright-core": {
        "version": PLAYWRIGHT_VERSION,
        "integrity": "sha512-pNwaXirhXMRLaRQs4NQ18EpTdtDoFwyPH9FOaaDC7YXG4hpwE2xmwCic/1srbuExVQC7zT9LJSJpDBAru+Vo9A==",
    },
}


def sha256_file(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def validate_asset(asset):
    parsed = urllib.parse.urlparse(asset["url"])
    if parsed.scheme != "https" or parsed.hostname not in {
        "github.com", "nodejs.org", "raw.githubusercontent.com"
    } or parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError("executor download must use an official HTTPS source")
    if not re.fullmatch(r"[a-f0-9]{64}", asset["sha256"]):
        raise ValueError("executor download requires a fixed SHA-256")
    if not isinstance(asset["max_bytes"], int) or not 0 < asset["max_bytes"] <= 67108864:
        raise ValueError("invalid executor download limit")


def validate_contract(contract):
    if contract["schema_version"] != 1 or contract["platform"] != "darwin-arm64":
        raise ValueError("unsupported executor manifest schema or platform")
    if contract["minimum_macos"] != "14.0" or contract["executors"] != EXPECTED_EXECUTORS:
        raise ValueError("executor version or entrypoint drift")
    if set(contract["downloads"]) != {"cua", "node"}:
        raise ValueError("unexpected executor download set")
    for asset in contract["downloads"].values():
        validate_asset(asset)
    names = set()
    for asset in contract["licenses"]:
        validate_asset(asset)
        if not re.fullmatch(r"[a-zA-Z0-9_.-]+", asset["name"]) or asset["name"] in names:
            raise ValueError("invalid executor license path")
        names.add(asset["name"])
    if names != {"cua-LICENSE.md", "cua-THIRD_PARTY_NOTICES.md"}:
        raise ValueError("executor license set is incomplete")


def load_contract(directory):
    contract = json.loads((directory / "executors.json").read_text())
    validate_contract(contract)
    return contract


def npm_tarball_url(name, version):
    leaf = name.split("/")[-1]
    return f"https://registry.npmjs.org/{name}/-/{leaf}-{version}.tgz"


def validate_npm_lock(package, lock):
    dependencies = {"@playwright/mcp": "0.0.83"}
    if package.get("dependencies") != dependencies or package.get("scripts"):
        raise ValueError("unexpected Playwright root package dependencies or scripts")
    if lock.get("lockfileVersion") != 3:
        raise ValueError("Playwright requires npm lockfile version 3")
    packages = lock.get("packages", {})
    expected_paths = {""} | {"node_modules/" + name for name in NPM_PACKAGES}
    if set(packages) != expected_paths or packages[""].get("dependencies") != dependencies:
        raise ValueError("Playwright lock package set drift")
    for name, expected in NPM_PACKAGES.items():
        entry = packages["node_modules/" + name]
        if entry.get("version") != expected["version"]:
            raise ValueError("Playwright lock version drift")
        if entry.get("resolved") != npm_tarball_url(name, expected["version"]):
            raise ValueError("Playwright lock source drift")
        if entry.get("integrity") != expected["integrity"] or entry.get("hasInstallScript"):
            raise ValueError("Playwright lock integrity or install-script drift")
        try:
            digest = base64.b64decode(entry["integrity"].removeprefix("sha512-"), validate=True)
        except ValueError as error:
            raise ValueError("invalid Playwright integrity") from error
        if len(digest) != 64:
            raise ValueError("invalid Playwright integrity length")
        expected_dependencies = {
            "@playwright/mcp": {"playwright": PLAYWRIGHT_VERSION, "playwright-core": PLAYWRIGHT_VERSION},
            "playwright": {"playwright-core": PLAYWRIGHT_VERSION},
            "playwright-core": {},
        }[name]
        if entry.get("dependencies", {}) != expected_dependencies:
            raise ValueError("Playwright transitive dependency drift")


def verify_download(path, asset):
    if path.is_symlink() or not path.is_file():
        raise ValueError("executor download must be a regular file")
    if path.stat().st_size > asset["max_bytes"]:
        raise ValueError("executor download limit exceeded")
    if sha256_file(path) != asset["sha256"]:
        raise ValueError("executor download SHA-256 mismatch")


def fetch_asset(asset, cache):
    validate_asset(asset)
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / asset["sha256"]
    if path.exists() or path.is_symlink():
        verify_download(path, asset)
        return path
    with tempfile.TemporaryDirectory(prefix="download-", dir=cache) as temporary:
        partial = pathlib.Path(temporary) / "asset"
        subprocess.run([
            "curl", "--fail", "--location", "--silent", "--show-error",
            "--proto", "=https", "--proto-redir", "=https", "--max-time", "300",
            "--max-filesize", str(asset["max_bytes"]), "--output", str(partial), asset["url"],
        ], check=True, timeout=310)
        verify_download(partial, asset)
        partial.replace(path)
    return path


def archive_path(name):
    path = pathlib.PurePosixPath(name)
    if not name or "\\" in name or "\x00" in name or path.is_absolute() or ".." in path.parts:
        raise ValueError("unsafe archive path")
    if not path.parts or path.parts[0] == ".":
        raise ValueError("empty archive path")
    return path


def extract_selected(archive_pathname, destination, selections, max_file_bytes=167772160,
                     max_total_bytes=536870912, max_members=30000):
    selected = []
    seen = set()
    total = 0
    with tarfile.open(archive_pathname, "r:gz") as archive:
        for index, member in enumerate(archive):
            path = archive_path(member.name)
            if index >= max_members or member.size < 0 or member.size > max_file_bytes:
                raise ValueError("archive limit exceeded")
            total += member.size
            if total > max_total_bytes:
                raise ValueError("archive limit exceeded")
            normalized = str(path)
            if normalized in seen:
                raise ValueError("duplicate archive path")
            seen.add(normalized)
            if not any(normalized == name.rstrip("/") or (
                name.endswith("/") and normalized.startswith(name)
            ) for name in selections):
                continue
            if not member.isfile() and not member.isdir():
                raise ValueError("unsupported archive member type")
            selected.append((member, path))
        destination.mkdir(parents=True, exist_ok=True)
        for member, path in selected:
            target = destination.joinpath(*path.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            if source is None:
                raise ValueError("archive file content missing")
            with source, target.open("xb") as output:
                shutil.copyfileobj(source, output, 1024 * 1024)
            target.chmod(0o755 if member.mode & 0o111 else 0o644)


def npm_install_command(directory, node_root):
    node = node_root / "bin/node"
    npm = node_root / "lib/node_modules/npm/bin/npm-cli.js"
    command = [str(node), str(npm), "ci", "--omit=dev", "--ignore-scripts",
               "--no-bin-links", "--no-audit", "--no-fund", "--registry=https://registry.npmjs.org/"]
    allowed = {"HOME", "TMPDIR", "LANG", "LC_ALL", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update({
        "PATH": str(node_root / "bin") + os.pathsep + "/usr/bin:/bin:/usr/sbin:/sbin",
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD": "1",
        "npm_config_ignore_scripts": "true",
        "npm_config_userconfig": str(node_root.parent / "user.npmrc"),
        "npm_config_globalconfig": str(node_root.parent / "global.npmrc"),
        "npm_config_cache": str(node_root.parent / "npm-cache"),
        "npm_config_update_notifier": "false",
    })
    return command, env


def runtime_manifest(contract, lock_path):
    result = {key: copy.deepcopy(contract[key]) for key in (
        "schema_version", "platform", "minimum_macos", "executors"
    )}
    result["sources"] = {
        "cua_sha256": contract["downloads"]["cua"]["sha256"],
        "node_sha256": contract["downloads"]["node"]["sha256"],
        "npm_lock_sha256": sha256_file(lock_path),
    }
    return result


def verify_installed_packages(directory):
    for name, expected in NPM_PACKAGES.items():
        package_dir = directory / "node_modules" / name
        package = json.loads((package_dir / "package.json").read_text())
        if package.get("name") != name or package.get("version") != expected["version"]:
            raise ValueError("installed Playwright package identity mismatch")
        if not (package_dir / "LICENSE").is_file():
            raise ValueError("installed Playwright package license missing")
    for path in directory.rglob("*"):
        if path.is_symlink():
            raise ValueError("unexpected symbolic link in Playwright runtime")


def validate_output_directory(output):
    if output.is_symlink() or output.name != "Contents":
        raise ValueError("executor output must be a Contents directory, not a symbolic link")
    if output.exists():
        marker = output / ".vega-computer-use-prepared"
        if not output.is_dir() or marker.is_symlink() or not marker.is_file():
            raise ValueError("refusing to replace an unowned executor output directory")
        if marker.read_text() != "vega-computer-use:1\n":
            raise ValueError("executor output ownership marker mismatch")
        if {path.name for path in output.iterdir()} != {
            ".vega-computer-use-prepared", "Helpers", "Resources"
        }:
            raise ValueError("refusing to replace executor output with unexpected contents")


def prepare(output, cache):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("computer-use packaging requires macOS arm64")
    contract = load_contract(PACKAGE_ROOT)
    package_path = PACKAGE_ROOT / "playwright/package.json"
    lock_path = PACKAGE_ROOT / "playwright/package-lock.json"
    validate_npm_lock(json.loads(package_path.read_text()), json.loads(lock_path.read_text()))
    validate_output_directory(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    archives = {name: fetch_asset(asset, cache) for name, asset in contract["downloads"].items()}
    licenses = [(asset["name"], fetch_asset(asset, cache)) for asset in contract["licenses"]]
    with tempfile.TemporaryDirectory(prefix="prepare-", dir=output.parent) as temporary:
        staging = pathlib.Path(temporary)
        extract_selected(archives["cua"], staging / "cua", ["cua-driver"])
        node_prefix = "node-v24.21.0-darwin-arm64"
        extract_selected(archives["node"], staging / "node", [
            node_prefix + "/bin/node", node_prefix + "/LICENSE",
            node_prefix + "/lib/node_modules/npm/",
        ])
        node_root = staging / "node" / node_prefix
        contents = staging / "Contents"
        helpers = contents / "Helpers"
        resources = contents / "Resources/computer-use"
        helpers.mkdir(parents=True)
        license_dir = resources / "licenses"
        license_dir.mkdir(parents=True)
        shutil.copy2(staging / "cua/cua-driver", helpers / "cua-driver")
        shutil.copy2(node_root / "bin/node", helpers / "node")
        for path in helpers.iterdir():
            path.chmod(0o755)
        shutil.copy2(node_root / "LICENSE", license_dir / "node-LICENSE")
        for name, path in licenses:
            shutil.copy2(path, license_dir / name)
        playwright = resources / "playwright"
        playwright.mkdir()
        for name in ["cli.js", "package.json", "package-lock.json"]:
            shutil.copy2(PACKAGE_ROOT / "playwright" / name, playwright / name)
        command, env = npm_install_command(playwright, node_root)
        pathlib.Path(env["npm_config_userconfig"]).write_text("")
        pathlib.Path(env["npm_config_globalconfig"]).write_text("")
        subprocess.run(command, cwd=playwright, env=env, check=True, timeout=300)
        if sha256_file(playwright / "package-lock.json") != sha256_file(lock_path):
            raise ValueError("npm changed the fixed lockfile")
        verify_installed_packages(playwright)
        (resources / "manifest.json").write_text(
            json.dumps(runtime_manifest(contract, lock_path), indent=2) + "\n"
        )
        shutil.copy2(PACKAGE_ROOT / "executors.json", resources / "sources.json")
        (contents / ".vega-computer-use-prepared").write_text("vega-computer-use:1\n")
        validate_output_directory(output)
        if output.exists():
            shutil.rmtree(output)
        contents.replace(output)
    return output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path, default=ROOT / "target/computer-use/Contents")
    parser.add_argument("--cache", type=pathlib.Path, default=ROOT / "target/computer-use/downloads")
    args = parser.parse_args()
    output = prepare(args.output.absolute(), args.cache.absolute())
    print(f"Prepared pinned computer-use executors: {output}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, tarfile.TarError, subprocess.SubprocessError) as error:
        sys.exit(f"Computer-use preparation failed: {error}")
