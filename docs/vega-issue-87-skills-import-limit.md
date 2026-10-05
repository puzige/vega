# Issue #87 / A8-05 — Skills import candidate limit

Status: frozen narrow S22 contract; implementation and focused service/GPUI tests passed on 2026-10-06. PR/cloud integration and post-fix native acceptance remain pending; whole #87 stays OPEN/PARTIAL.
Issue: https://github.com/puzige/vega/issues/87
Frozen contract: [Skills S2/S5](vega-issue-74-skills.md) and [S22](vega-issue-74-skills-delivery.md).

## Observed failure

On the installed official v0.1.41 build, choosing an owned root with 129 valid Skill candidates displayed the generic invalid/unsafe-source message. Choosing the same root after reducing it to one candidate produced a normal preview. Cancelling that preview left source associations, approvals, switches and authorization generations unchanged. This is pre-fix native evidence, not native verification of this change. Public bounded observations: [Skills](https://github.com/puzige/vega/issues/87#issuecomment-5999396930) and [v0.1.41 QA](https://github.com/puzige/vega/issues/249#issuecomment-5999397426).

The runtime already rejects roots above its 128-candidate ceiling with `SkillError::TooManyCandidates`. Settings root preview currently flattens that error into `SkillSettingsError::Invalid`, losing the recovery reason.

## Contract

1. Root preview preserves `SkillError::TooManyCandidates` as a dedicated content-free `SkillSettingsError::TooManyCandidates`, with stable code `too_many_candidates`.
2. The existing Settings-owned error type remains in its current module and export path. No second error type, new cross-crate data model, dependency, migration or permission is added.
3. Settings presents exactly: `每个目录最多支持 128 个 Skill 候选，请减少数量后重试，未更改授权`.
4. The label is static and bounded. It includes the cap, a reduce-and-retry instruction and the unchanged-authorization result. It contains no selected path, Skill name/body, count supplied by the source or underlying error text.
5. A root with exactly 128 valid candidates previews all 128. A root with 129 is rejected entirely and creates no partial preview or association; discovery is never silently truncated.
6. Preview rejection, preview without approval and preview dismissal do not change stored sources, approvals, switches, consent generation or revocation generation. Reduced input can be selected again in the same Settings session.
7. Other runtime discovery errors still map to `Invalid`; existing `NotFound`, `Stale`, `PreviewRequired`, `SelectionLimit` and storage error mappings/labels keep their semantics.

The ceiling, candidate scan, filesystem validation, import-by-reference and explicit approval flow are unchanged. This slice excludes S21 introductory copy/layout, resource tooling and all unrelated Skills acceptance.

## Test-first matrix

All automated fixtures are owned `TempDir` roots and SQLite Stores. Production discovery and Settings service are used; GPUI tests use the existing in-process mounted Settings harness and simulated native folder-picker response. No shell, Git process, network, provider, user configuration/database or installed app is exercised.

| ID | Precondition and operation | Required observation | Layer | Initial state |
| --- | --- | --- | --- | --- |
| L01 | Select 129 valid candidates | Dedicated error and `too_many_candidates`; no preview/partial import; source/approval/switch/epoch state exactly unchanged | production service + Store | PASS — first business-red retained, now exact typed variant/code; empty and existing approved authority states |
| L02 | Select exactly 128 valid candidates | All 128 distinct candidates visible; unapproved/disabled; no Store mutation | production service + Store | PASS — all 128 distinct names and disabled/unapproved state |
| L03 | After L01 reduce owned input to one, retry then dismiss preview | One-candidate preview succeeds in same session; dismissal prevents approval; no Store mutation, including after reopening Store | production service + Store | PASS — same service/Store session, cancelled token rejected, then original Store dropped and reopened |
| L04 | Select a regular file as root and an owned candidate symlink escaping the root | Invalid root remains rejected; unsafe candidate stays unavailable and diagnosed; no Store mutation | production service + Store | PASS — `Invalid` root, content-free `unsafe_path` candidate, original owned outside bytes unchanged |
| L05 | Format the new error and existing errors | Exact bounded limit label/code; existing label mappings unchanged; no private source content in messages | unit | PASS — exact static label within 256 UTF-8 bytes and all six prior code/label pairs unchanged |
| L06 | Mounted picker returns 129-candidate root at 960px and a normal wide size, Light/Dark | Production operation yields limit label in mounted error presentation; no root/body preview or link action; import stays retryable | mounted GPUI + service/Store | PASS — 960×600 and 1403×860, Light/Dark; exact service message plus mounted node bounds within actual content viewport, no glyph/native readability claim |
| L07 | Mounted picker retries reduced root then cancels preview | Normal preview and cancel action; no source/approval/switch/epoch mutation | mounted GPUI + service/Store | PASS — picker cancellation, same-session reduced-input retry, scroll-to-visible preview cancel, expired token denied and genuine Store reopen |
| L08 | Run the changed build's real native import/error/retry/cancel path | 128 limit guidance readable and unchanged authorization observable | native | NOT RUN — integrator/user acceptance |

The first service regression initially asserts the stable limit code on current production behavior so it can produce a business failure without relying on a missing enum variant. Typed-variant assertions are added with the production mapping and retain the original stable-code assertion. First failure and final pass outputs are retained.

The service tests also cover the shared project/global root-preview mapping and preserving an existing linked, approved source with nondefault global/project/source switches. The exact 128-success/129-failure production boundary and exact UI label constrain the documented ceiling without expanding the runtime's private constant into a new public API.

## Focused evidence

Local toolchain: Rust 1.98.0, Cargo Nextest 0.9.146, aarch64 macOS. Raw logs and source/diff freezes are private; public rows contain bounded outcomes only.

| Stage | Evidence | Outcome | Raw log SHA-256 |
| --- | --- | --- | --- |
| Initial test compile | L01 Nextest command below | exit 101, no test run: `unwrap_err` required `SkillRootPreview: Debug`; corrected only test error extraction to `.err().unwrap()` | `e3e530098d087b9f06431bc2dcc03ca3e70adc2c9819a4526cfb312fbdb44884` |
| Business first-red before production changes | Nextest `08695935-6985-4b12-bb77-d24b6a09022e` | exit 100; 1 failed, 544 skipped; stable code was `invalid_source`, expected `too_many_candidates`; unchanged Store/ticket assertions preceded the failure | `4825ff851d8888d246592bc343bb5453d5c3ee7b56448aac6955bea1355a1fb0` |
| Service final | Nextest `1cac1766-257f-43b3-a71d-1f2be2d17dd2` | exit 0; 12 passed, 538 skipped; 6 new cases and 6 existing related consent/preview safety cases; summary 0.219s | `c6e7dc771533e3e5d2959b25b70c9a51a9d075bc256384f0f4d7ae606a370ae4` |
| Mounted first run | Nextest `b7a49f82-e397-407a-ade3-789009bbd20f` | exit 100; 5 passed, 4 failed, 537 skipped; new tests mistakenly compared message bounds with `settings-page-skills`, which production marks on the title only. Exact service-message and unchanged-state assertions passed first. Tests now compare containment against the actual `settings-section-content` viewport and scroll preview Cancel into view; product layout did not change | `10f86c092c3d762b1db89e741230986af8721c5035eb6a2ab65a501686edc08c` |
| Mounted final | Nextest `a5245a81-d797-4bb8-96b5-f6c64f4ac072` | exit 0; 9 passed, 537 skipped; 5 new cases and 4 existing related folder-picker/late-result cases; summary 0.234s | `c0b3d30719a142d143b98bfb90e3f9136f7bcb0238999e18b48ddd762f3bff3c` |

Initial compile and business first-red command:

```sh
cargo nextest run -p vega_conversation -E 'test(/issue87_s22_import_rejects_129_candidates_without_mutating_authority/)'
```

Service final command:

```sh
cargo nextest run -p vega_conversation -E 'test(/skill_settings::tests::issue87_s22_/) | test(/skill_settings::tests::project_review_is_hash_bound_and_stale_approval_cannot_reenable/) | test(/skill_settings::tests::imported_root_requires_preview_and_unlink_never_deletes_source/) | test(/skill_settings::tests::stale_generation_and_cross_project_scope_cannot_mutate/) | test(/skill_settings::tests::leaving_page_or_closing_settings_revokes_all_preview_receipts/) | test(/skill_settings::tests::slow_preview_cannot_reinstall_receipt_after_page_leave/) | test(/skill_settings::tests::vega_global_preview_uses_only_supplied_config_root/)'
```

Mounted first/final command:

```sh
cargo nextest run -p vega_ui -E 'test(/issue87_s22_import_limit/) | test(/issue74_native_folder_picker_previews_exact_root_before_link/) | test(/issue87_skills_settings_discards_late_result/) | test(/issue74_s06_reload_hides_stale_projection_while_pending_and_after_failure/)'
```

Production changes are limited to the existing Settings error variant/code, the root-preview `TooManyCandidates` mapping, the static label and a debug identifier on the unchanged warning-message renderer. Source scanning, safety fences, candidate ceiling, authorization and schema are unchanged. No dependencies or code comments were added; existing assertions were retained. `cargo fmt --all` only formatted this slice.

Final `cargo fmt --all -- --check` and `git diff --check` both exited 0 with empty output. Each empty raw log has SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

No local workspace-wide tests are run. Cloud gate and native post-fix acceptance are pending integration.

## Implementation sequence

1. Integrator confirms this contract and matrix.
2. Add L01–L04 service regressions and preserve the first failing run before production changes.
3. Add the dedicated Settings error variant and narrow `TooManyCandidates` mapping in root preview; add the static UI label, retaining the existing renderer and theme tokens.
4. Add L05–L07 bounded-label and mounted picker/error/retry/dismissal regressions. A stable debug identifier on the existing message element may be added if needed for mounted assertions; its style and placement stay unchanged.
5. Run only these new task-specific Nextest cases plus related pre-existing Settings safety/late-result cases. Run formatting and diff checks; retain raw logs, run IDs, toolchain and source/diff hashes privately.
6. Commit at most three task commits and hand off for PR, cloud check, merge and native acceptance. Whole #87 remains OPEN/PARTIAL.

## Compatibility, rollback and residuals

There is no schema or stored-data change. Rolling back this patch restores the generic Settings error mapping and label; the runtime limit remains enforced either way. The new variant may require updating exhaustive matches; the change is scoped to the existing Settings error ownership.

Native post-fix readability, actual platform folder-picker behavior and installed-build identity remain NOT RUN by this implementation agent. GPUI layout bounds establish mounted presentation, not final native glyph rendering or semantic macOS focus ownership.

## Change log

- 2026-10-06: frozen integrator-reviewed S22 import-limit recovery slice from an installed-build native observation, implemented with business-first-red and focused service/mounted recovery evidence; no expansion of the frozen Skills authority or format contract.
