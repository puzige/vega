# Bundled computer-use executors

This directory implements issue #75, requirements R29–R31 and acceptance case CU15. `executors.json` fixes the official Cua Driver 0.34.0 and Node 24.21.0 archive hashes and the Cua license hashes. The npm lock fixes Playwright MCP 0.0.83 and its two transitive packages. No browser binary, Python runtime, global npm package, or additional agent loop is installed.

`cargo xtask package` prepares these resources before building Vega and copies them into the app:

```text
Contents/
  Helpers/cua-driver
  Helpers/node
  Resources/computer-use/
    manifest.json
    sources.json
    licenses/
    playwright/cli.js
    playwright/package.json
    playwright/package-lock.json
    playwright/node_modules/
```

Preparation can also run independently on a macOS arm64 build machine:

```sh
python3 scripts/prepare-computer-use.py
```

The default output is `.vega-build/computer-use/Contents`; verified download archives are cached in `.vega-build/computer-use/downloads`. A cached file with a wrong hash is rejected. The script never replaces an existing output directory unless its preparation marker and directory layout identify it as a previous generated output. For development, set `VEGA_COMPUTER_USE_ROOT` to the absolute path of this generated `Contents` directory. Runtime does not download or upgrade components or search PATH.

`manifest.json` has `schema_version: 1`, `platform: darwin-arm64`, `minimum_macos: 14.0`, three versioned `executors` with Contents-relative `entrypoint` paths, and `sources` containing the archive and npm lock SHA-256 digests. Source hashes describe the original downloads: re-signing Mach-O files changes their bytes. Final bundle integrity is covered by the app signature. Vega's existing app deployment floor is unchanged; runtime gates computer use to macOS 14 and later.

Preparation runs the downloaded Node's bundled npm using `ci --omit=dev --ignore-scripts --no-bin-links` and disables browser download. The generated resource tree retains npm package licenses and notices. Cua's pinned MIT license and third-party notices and Node's complete bundled license are copied into `licenses/`. The universal Cua archive supplies only the `cua-driver` executable to Vega; its separate SDK dylib, Node addon and cursor-theme executable are not included.

`scripts/sign-computer-use.py` verifies the app contains only the three expected Mach-O executables, signs Cua and Node before signing Vega.app, then verifies the complete signature. Node receives only the JIT entitlement in `node-entitlements.plist`. Local packaging uses ad-hoc signing. The existing release workflow passes its Developer ID identity and temporary keychain to the same script before notarization. Ad-hoc signing is not notarization.

The offline test command covers path traversal, selected links, duplicate members, archive size limits, hash failure, lock drift, unowned output preservation, install flags, runtime manifest and nested signing order:

```sh
python3 -B -m unittest discover -s scripts -p 'prepare_computer_use_tests.py' -v
```

These tests do not control Chrome or macOS windows. Real TCC attribution, the Chrome extension connection and system interaction remain user acceptance checks.
