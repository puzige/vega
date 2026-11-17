# Native UI parity repairs after user screenshots

## Scope and version

The user rejected the visual quality of the production NSH migration and supplied the new-task, General, Providers, MCP and Agents screens. This supersedes any inference that historical code/CI completion established product parity. This repair addresses the pictured composition defects; it does not certify the nine-row native acceptance matrix.

- Base: `080a755caec8c5ccc9e48fe40b7aa4728d159ade`.
- Pre-migration layout comparison: `34555cec9c0c4c4c1f7a528d739752f414e53edf`.
- Nine-file source identity: `aa570546e051b0751109140e2904c9f72f1acaa0d58592e4dca71977e1fba86e`.
- Manifest: `target/nsh-ui-parity-source-manifest.json`; identity is SHA-256 of Python-default `json.dumps(sorted((repository-relative path, file SHA-256) pairs))`.
- Frozen binary patch: `target/nsh-ui-parity-frozen.patch`, SHA-256 `52af810a569b70fbf9d9e605737d74303925ed4e6eaf9f1d55ae2e4f2febb875`.
- No dependency, vendor, GPUI Kit/Fast, controller, permission configuration, daily installation or user-data changes. The pre-existing modified AX blocker record and Python caches were preserved.

## Findings and changes

| Visible symptom | Source evidence and repair | Native result |
|---|---|---|
| White sidebar rectangles, including empty space and navigation | Sidebar leaf and navigation hosts used opaque `Surface::new`; its default backdrop paints the window background over the sidebar. Change only these hosts to `new_transparent`; retain explicit selected/hover backgrounds and semantic/event wiring. | Not observed after repair |
| Short project/backend labels ellipsized in a wide window | Utility-trigger width counted text, padding and icon but omitted the NSH control's two border pixels. Add actual border contribution, ceil measured text, use the same icon-gap token as painting. Existing maximum text width remains. | Not observed after repair; possible additional causes remain unproven |
| Plain adjacent Copy/Restore draft text | Recovery controls used unstyled base buttons. Give retry/copy/restore compact themed button chrome, focus/hover/disabled appearance and inherited body typography; publish theme colors with editor projection. Keep recovery visibility, immutable draft, actions and authority/pending/composition fences unchanged. | Not observed after repair |
| General settings vertically stacked labels and oversized choice/switch chrome | Restore wrapping 208px label / flexible control rows, 16px column gap, 12px vertical row padding and compact minimum-28px choices using existing theme tokens. Remove extra switch wrapper padding. | Not observed after repair |
| Providers flattened form, stray `?`, oversized object menus | Restore grouped fields, label/control rows, compact primary Save and actions, bounded 2–4-row model editor, section/metadata hierarchy, valid icon-based help popover and compact object menus. Remove misplaced form help trigger while retaining protocol guidance. Restore 24px list/detail gap. | Not observed after repair |
| MCP full-width action stack and oversized ordinary-feedback card | Render toolbar/feedback/waiting unframed, keep server/confirmation framing, wrap actions horizontally, emphasize unhealthy/action-required states and place transport/auth/secret/save controls with their fields. Do not hide waiting/error/authorization information. Success-message lifetime is unchanged from the existing controller. | Not observed after repair |
| Agents flattened status/configuration rows | Restore aligned field/status rows, headings and secondary metadata, compact actions and primary Codex save. Preserve permission and agent actions, enabled/pending guards, editing and composition. | Not observed after repair |

The project bar/input container offset was also compared with the original source: the prior utility bar intentionally used a 12px inset and separate surface. It was not changed merely to force equal outer edges. Empty history/provider configuration in an isolated home does not explain or excuse the pictured painting/layout defects.

## Actual local checks

Official Cargo `1.98.0 (797e8a9bc 2026-08-05)`, rustc `1.98.0 (88d9e12ae 2026-08-18)`; local Nextest `0.9.151`, distinct from CI's pinned `0.9.146`. Cargo uses `CARGO_FORK_AUTO_UPDATE=0`, offline/locked, at most two build jobs and `target/official-nsh-validation`.

| Check | Actual result |
|---|---|
| `cargo check -p vega_ui --locked --offline -j 2 --target-dir target/official-nsh-validation` | exit 0, 7.60s, log `target/nsh-ui-parity-check-4.log` |
| `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked --offline -j 2 --target-dir target/official-nsh-validation -- -D warnings` | exit 0, 6.36s, `target/nsh-ui-parity-clippy.log` |
| `python3 scripts/release_tests.py` | 5/5, `target/nsh-ui-parity-python-release.log` |
| `python3 scripts/prepare_computer_use_tests.py` | 15/15, `target/nsh-ui-parity-python-cua.log` |
| `python3 -m unittest discover -s scripts/tests` | 40/40, `target/nsh-ui-parity-python-scripts.log` |
| `python3 scripts/check_i18n.py --json` | structured `errors: []`; no allowlist changes, `target/nsh-ui-parity-i18n.json` |
| `cargo nextest list --workspace --locked --offline --target-dir target/official-nsh-validation --message-format json`, `CARGO_BUILD_JOBS=2` | test-count 0, 16 binaries |
| `cargo nextest run --workspace --locked --offline --target-dir target/official-nsh-validation --no-tests=pass`, `CARGO_BUILD_JOBS=2` | no Rust tests: 0 discovered / 0 executed, 0 passed / 0 skipped; run `5baef051-461d-4105-8662-50dc1c340762` |
| `cargo build -p vega --bin vega --locked --offline -j 2 --target-dir target/official-nsh-validation --message-format=json-render-diagnostics` | exit 0, 53.92s; `target/nsh-ui-parity-build.jsonl` and `.stderr` |

The development binary is `target/official-nsh-validation/debug/vega`, SHA-256 `f8598b2fef316d581c1afc109c15976db24abbd75305b98467cc768fa8110ab4`. The existing linker warning about `__eh_frame` exceeding 16MB remains unsuppressed. Source-manifest readback after building matched the frozen identity. Existing running applications were not restarted or replaced.

### Retained failures and corrections

Early compile logs `target/nsh-ui-parity-check.log`, `-2.log`, `-3.log`, `-final.log` retain errors from unsupported NSH style methods, incorrect font-type qualification and GPUI-style percentage helpers. Actual fixed-version APIs were inspected; final code uses NSH `.focus` / `.hover`, prelude `FontWeight` and `relative(1.)`. No assertion, compiler warning policy, dependency or feature was relaxed.

One i18n invocation referenced a nonexistent `check_i18n_literals.py`; it did not run a check. Located and ran the real `scripts/check_i18n.py --json`. One Nextest invocation supplied unsupported `-j`; corrected build concurrency to `CARGO_BUILD_JOBS=2`, without changing discovered tests or runtime policy. These invocation failures are not product failures or passing checks.

## Native verification boundary and manual continuation

This run's first desktop preflight returned nonretryable `screen_recording_permission_missing` with `execution_state: not_dispatched`. No window was observed or input sent. Computer operations stopped; no shell, alternate capture, AX automation or permission bypass was used. The known prior native AX duplicate-child crash and its explicit unapproved adapter fix remain independently recorded in `nsh-native-acceptance-blockers.md`; this UI patch does not repair or disable accessibility.

Therefore **none of these repaired screens has a post-repair pixel, click, keyboard, focus, IME or AX pass**, and none of the nine matrix rows is newly complete. Windows/Linux remain not run. Historical green CI covers the old source only; new-head required CI is separate. No merge, release, distribution packaging or installation is authorized or performed.

To continue manually, quit only the user's owned test instance and relaunch the same isolated home; do not delete or overwrite it:

```sh
cd /Users/puzige/Workspace/worktrees/vega-907-nsh-full
VEGA_HOME="$PWD/target/nsh-native-manual-home" \
  ./target/official-nsh-validation/debug/vega
```

Priority manual observations: sidebar/nav idle and hover without white slabs; short project/backend labels without unnecessary ellipsis; draft-recovery controls legible and functional; General theme/language/toggles; Providers add/edit/model editor/help/menu; MCP toolbar/local/remote/auth/confirmation/waiting/errors; Agents statuses/settings/Codex form. Repeat at wide and narrow widths, in both themes and languages; preserve credentials and runtime state. A successful sub-operation does not pass an entire native acceptance row.

## Independent review and cloud gates

Independent nonauthor review completed with **no actionable P0–P3 findings**. It independently verified all nine source hashes, recomputed the content identity, and compared the current diff byte-for-byte with the frozen patch, initially and before delivery. Review execution: `01M71P1GHCR97V8NEX20PSK9SF` / `01M71P1GHCHEVARQG4F69DV4GX`. Manifest file SHA-256: `d0646a4658b9eecee3fa4d8139d632f1311f20042de708c4e949f84971d8b64f`.

Review checked transparent constructor semantics, trigger border/gap accounting, recovery theme/reactivity and action fences, settings draft/composition/save guards, preservation of all generated MCP actions and required permission/OAuth/credential warnings. It wrote no files and ran no Cargo, network or Git mutations. It did not independently certify the parent's build commands or native behavior.

Review binds only to the frozen nine-file delta and affected contracts, not the entire historical migration. New-head cloud gates are pending publication. Native limitations above remain regardless of static-review or CI outcome.
