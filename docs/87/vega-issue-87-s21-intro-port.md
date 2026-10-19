# Issue #87 S21 — Current-mainline Skills introduction port

Status: **narrow implementation ported; final focused GPUI regression passed on S22-integrated mainline; cloud gate and native acceptance remain open** (2026-10-06).

## Contract

The approved 2026-10-03 S21 preview shortened the Skills Settings introduction so its final punctuation did not occupy an isolated line at a 960px native window width. Port only that product change to fresh `origin/master`:

- Add the existing-style `skills-intro` element ID and debug selector to the introduction container.
- Use the exact introduction: `Skills 是低信任工作流说明，不会扩大脚本、MCP 或文件工具权限。全局与自动触发默认关闭。`
- Keep the current secondary-text theme token, typography, column geometry, controls and focus order.
- Preserve the existing low-trust and independent-tool-authority meaning. This text grants no script, MCP or file-tool authority; global Skills and automatic triggering retain their existing defaults.

The product patch is limited to the introduction container and literal. No dependency, migration, public API, consent, settings mutation, Provider/MCP execution or source-discovery change belongs to this slice. The old preview branch contains unrelated changes and must not be merged as a whole. The shared S01–S22 delivery matrix remains owned by the integrator.

## Existing native evidence

- The exact user-approved 2026-10-03 S21 preview was previously observed at 960px in Light and Dark without the isolated final punctuation. This is evidence for that old preview only.
- On 2026-10-06 the installed official v0.1.41 build still displayed isolated final punctuation in Skills Settings at 960×860 in Light. This is the repair's pre-port native evidence, observed during the S22 import-limit check. The private observation is retained in the main task; no standalone screenshot file or hash is claimed here.
- Neither observation accepts the current-mainline port. Its installed native Light/Dark, narrow/wide and focus/error matrix is **NOT RUN**. Parent #87 remains **OPEN/PARTIAL**.

## Test-first acceptance matrix

| ID | Requirement / risk | Setup / operation | Expected observation | Evidence class | Status |
|---|---|---|---|---|---|
| P01 | Exact text and limited authority | Review the four-line product port against the frozen literal and old preview patch | Only ID, debug selector and literal change; authority code and tokens unchanged | source review | PASS — exact narrow diff reviewed |
| P02 | Minimum-size Light mounting | Owned empty Store/config, 960×600 window, maximum 365px Settings navigation; mount production Settings → Skills | Content width 547px; intro has positive bounded bounds inside the column and actual `settings-section-content` viewport, test-platform height below 30px; Refresh follows inside that viewport without overlap | GPUI production render / test-platform metrics | PASS — final focused run, GPUI only |
| P03 | Minimum-size Dark mounting | Repeat P02 in Dark | Same bounded geometry and unchanged empty/default consent projection | GPUI production render / test-platform metrics | PASS — final focused run, GPUI only |
| P04 | Normal-size Light mounting | Owned empty Store/config, 1403×860 window, default 304px navigation | Content capped at 744px; same intro and Refresh bounds | GPUI production render / test-platform metrics | PASS — final focused run, GPUI only |
| P05 | Normal-size Dark mounting | Repeat P04 in Dark | Same bounded geometry and unchanged empty/default consent projection | GPUI production render / test-platform metrics | PASS — final focused run, GPUI only |
| P06 | Existing Settings geometry | Run the existing Settings shell/sidebar-width regression | The rail follows Sidebar width and the content cap remains intact | existing GPUI regression | PASS — final focused run |
| P07 | Existing consent/picker and keyboard flow | Run existing exact-root preview-before-link and keyboard-reachable Import Folder regressions | Existing preview/link/SHA-review separation and keyboard path remain protected | existing GPUI / owned Store regression | PASS — final focused run |
| P08 | Native post-port typography | Install a separately identified approved current-mainline artifact, visit Skills at 960px and normal width in Light/Dark | Final punctuation remains with readable text; no clipped glyphs or control overlap | macOS native UI | NOT RUN |
| P09 | Full S21 matrix | Independently verify active-Skill indicator, native focus/error states and other appearance rows | Evidence remains scoped to the actual artifact and operation | native UI / remaining matrix | NOT RUN |

The GPUI platform uses test text metrics. Positive mounted bounds and a below-30px test-platform height do **not** establish macOS glyph layout, readability or a production red under the old literal. No automated pre-port wrap failure is claimed. Persistence/recovery beyond unchanged read-only projections, long/error candidate contents, responsive Environment modes and active-Skill behavior are outside this literal-only patch.

### Geometry calculation

The existing Settings implementation supplies 24px horizontal insets on both sides and a 744px content cap. No shared values change.

| Window / navigation | Remaining width | Content width |
|---|---|---|
| 960 / 365 | `960 - 365 - 2×24 = 547` | `min(547, 744) = 547` |
| 1403 / 304 | `1403 - 304 - 2×24 = 1051` | `min(1051, 744) = 744` |

The test also checks the intro and following Refresh bounds lie inside the content column, the actual `settings-section-content` viewport and the requested window height. The old preview's 30px test-platform ceiling is retained solely as a deterministic GPUI layout regression, not an inference about native line count.

## Implementation and verification plan

1. Fetch origin; create an isolated `codex/87-s21-current-port` worktree from current `origin/master`. Preserve all existing preview worktrees.
2. Freeze this narrow contract, inspect current Settings rendering and adapt the old layout test using existing GPUI APIs and an owned empty Store/config.
3. Apply only the two container markers and exact shortened literal. Add four Light/Dark × minimum/normal-size mounted tests; preserve all existing assertions.
4. Format touched Rust files and freeze source/content/diff identity before testing. Run only these S21 tests and the directly related existing Settings/picker regressions, with zero retries.
5. Record first and final commands, run IDs, raw-log hashes, exit/result/duration, content identity and residuals. After source freeze, only this evidence document may change unless a failure requires an explicitly recorded repair and new freeze.
6. Commit at most three times, leave a clean worktree and hand off. The main agent owns PR, cloud full gate, merge and board state. This subtask does not push, install, restart or operate the user's Vega app/database.

Planned task command:

```sh
cargo nextest run -p vega_ui -E 'test(/issue87_s21_intro_/) | test(/r21_settings_shell_opens_general_and_tracks_sidebar_width/) | test(/issue74_native_folder_picker_previews_exact_root_before_link/) | test(/issue74_skills_settings_import_is_keyboard_reachable/)'
```

Cloud workspace fmt/Clippy/Nextest remains the merge gate. No local full-workspace tests are requested or run by this slice. Rollback restores the former introduction literal and removes the added markers/tests; no user data or permission rows need rollback.

## Freeze and results

### First post-port run

- Frozen at 2026-10-05 17:48:45 UTC / 2026-10-06 01:48:45 Asia/Shanghai; branch `codex/87-s21-current-port` on fresh fetched `origin/master`.
- Environment: macOS 15.8 arm64, Rust/Cargo 1.98.0, Nextest 0.9.146, Git 2.55.0.
- Source-set SHA-256: `24b682dbe8b3ce708126bec2942c503b5d54a232c746491cb7a4e71535c4d2d5`.
- Source diff SHA-256: `8c6a040c61e06765c0741819d90d596f5fc41091e3943e3db092158195ea2c97`; staged source/spec diff SHA-256: `c21080a776f5ed76d640769503384b98331dfe6983c32ce8a1be063d3c2d0397`.
- Exact command: the planned task command above. First execution was post-port; no automated pre-port production red is claimed.
- Run ID: `9af97f66-8ee3-4f2e-9483-56684d0af5c9`; exit 0, 99.137s wall time including the fresh isolated build. Test summary: 7 passed, 538 skipped in 0.172s.
- Raw-log SHA-256: `b4eef6144a07add53a8cf70a801b3c1aabe36ef6497883feb1ffff3ba9522c9a`. Private raw log and freeze manifest are retained outside the worktree. Both source files matched their pre-run hashes after execution.
- Touched-file `rustfmt --edition 2024 --check crates/vega_ui/src/settings/skills.rs` and `git diff --check`: exit 0.

Bounded raw footer:

```text
 Nextest run ID 9af97f66-8ee3-4f2e-9483-56684d0af5c9 with nextest profile: default
    Starting 7 tests across 1 binary (538 tests skipped)
     Summary [   0.172s] 7 tests run: 7 passed, 538 skipped
```

The integrator reviewed the product diff and four mounted tests. After the first run completed, explicit `settings-section-content` viewport assertions were added for all four edges of both the introduction and Refresh, preserving the content-column, window-height and spacing checks. The clean branch was fetched and rebased onto the S22-integrated current mainline without conflicts. The final focused run below verifies this updated source; the first run is preserved separately. Spec deviations: **none**.

### Final current-mainline run

- Frozen at 2026-10-05 17:53:36 UTC / 2026-10-06 01:53:36 Asia/Shanghai; branch `codex/87-s21-current-port` rebased onto current `origin/master` containing the merged S22 import-limit fix.
- Environment unchanged from the first run. Source-set SHA-256: `729f86cb8ec3207ff353dfa854ffe057774cc5398a570ff64c3a13580858305f`.
- Source diff SHA-256: `d2965ef54fffcdfd4953b6d56e2b3aac4cbe1e6a86d7d48cb932beed7f364906`; staged source/spec diff SHA-256 at test freeze: `479a3a64af42cd3f9e76d9507f33275f87e677ce831ba0edb85e6fdcc8b4bd0f`.
- Exact command: the same planned task command above. Started 2026-10-05 17:54:12 UTC; completed 17:54:29 UTC.
- Run ID: `d3426721-11b6-4913-a0f0-96dd8e42978e`; exit 0, 16.537s wall time including incremental build. Test summary: 7 passed, 543 skipped in 0.177s. The changed skipped count reflects the integrated S22 tests; it is not broader executed coverage.
- Raw-log SHA-256: `5e06033229ca72ed660cb6cfc983c49fd73c6dddbb3b6f2f09d0951499cae869`. Raw first/final logs, source patches and complete Git/tree freeze identities are retained privately outside the worktree.
- Source hashes after the final run exactly matched the freeze. Only this evidence document changed afterward.
- `rustfmt --edition 2024 --check crates/vega_ui/src/settings/skills.rs` and `git diff --check origin/master`: exit 0. Both checks emitted empty raw logs, SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

Bounded raw footer:

```text
 Nextest run ID d3426721-11b6-4913-a0f0-96dd8e42978e with nextest profile: default
    Starting 7 tests across 1 binary (543 tests skipped)
     Summary [   0.177s] 7 tests run: 7 passed, 543 skipped
```

## Residuals

- **LIMIT:** four new tests prove production GPUI mounting and test-platform bounds only. They do not prove native glyph wrapping, rendered text readability, native focus or visual contrast. The former literal was not tested for a failing native-wrap assertion; there is no fabricated automated production red.
- **NOT RUN:** installed current-mainline native post-port checks and remaining S21 appearance/accessibility scenarios, including active-Skill indicator and native error/focus paths.
- **NOT RUN:** S21 PR/cloud full gate and master integration at this implementation handoff. Those are owned by the main agent.
- Parent #87 remains **OPEN/PARTIAL**. No user-owned Skill file, user configuration/database, installation, running app or Provider/MCP operation was changed by this subtask. The owned test fixtures are independent of the user's app state. No worktree is cleaned before the main task's acceptance decision.
