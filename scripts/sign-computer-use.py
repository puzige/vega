#!/usr/bin/env python3
import argparse
import pathlib
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
MACHO_MAGICS = {
    bytes.fromhex(value)
    for value in ["feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca", "cafebabf", "bfbafeca"]
}


def validate_inventory(app):
    contents = app / "Contents"
    expected = {"Helpers/cua-driver", "Helpers/node", "MacOS/vega"}
    found = set()
    for path in contents.rglob("*"):
        if path.is_symlink():
            raise ValueError("unexpected symbolic link in app signing inventory")
        if not path.is_file():
            continue
        with path.open("rb") as source:
            if source.read(4) not in MACHO_MAGICS:
                continue
        relative = path.relative_to(contents).as_posix()
        if relative not in expected:
            raise ValueError("unexpected native executable in app signing inventory")
        found.add(relative)
    if found != expected:
        raise ValueError("app signing inventory is missing a required executable")


def signing_commands(app, identity, entitlements, keychain=None):
    options = ["codesign", "--force", "--options", "runtime", "--sign", identity]
    if identity != "-":
        options += ["--timestamp"]
    if keychain:
        options += ["--keychain", keychain]
    return [
        options + [str(app / "Contents/Helpers/cua-driver")],
        options + ["--entitlements", str(entitlements), str(app / "Contents/Helpers/node")],
        options + [str(app)],
        ["codesign", "--verify", "--deep", "--strict", str(app)],
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("app", type=pathlib.Path)
    parser.add_argument("--identity", default="-")
    parser.add_argument("--keychain")
    args = parser.parse_args()
    app = args.app.absolute()
    validate_inventory(app)
    entitlements = ROOT / "packaging/computer-use/node-entitlements.plist"
    for command in signing_commands(app, args.identity, entitlements, args.keychain):
        subprocess.run(command, check=True, timeout=120)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        sys.exit(f"Computer-use signing failed: {error}")
