# Native mutation provenance

## Scope and semantic blocker

This assignment changes only `vendor/nsh-platform-gpui/src/editing.rs`, `vendor/nsh-platform-gpui/src/text_input.rs`, `vendor/nsh-platform-gpui/src/text_input/host_tests.rs`, and this new document. Parent owns exports, application integration, validation, and independent review. Supplied base HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`; not rechecked with Git.

The inspected `nsh-core 0.4.0` model exposes `undo`/`redo` as opaque whole-snapshot restores, with no replaced range or operation ledger. Exact mutation provenance for those native fallback operations cannot be recovered without changing core or maintaining a second history. Neither is authorized. When hooks remain attached, an unhandled native undo/redo therefore retires hooks and consumes that gesture without changing text. The parent-owned application key policy must handle authoritative undo/redo first. Unobserved legacy model history remains unchanged. This is a concrete integration requirement, not a claim of native-history acceptance. An observed text change without valid operation provenance also retires hooks; it never fabricates a diff or a full replacement.

## Contract

`InputEdit` adds `mutation: Option<InputMutation>`. `InputMutation` has exactly public `range_utf16: (u32, u32)` and `inserted: String`. Ranges are sorted scalar-boundary UTF-16 offsets in the previous emitted serial's full display text, including marked text. The inserted string is the actual accepted insertion after native clipping/filtering. A same-text replacement is `Some`, advances serial, and notifies even when the resulting text/selection/composition tuple is identical. Registration, re-registration, snapshots, and selection-only changes are `None`.

## Native operation semantics

- Typing and explicit replacement report the selected or supplied native range and actual accepted text, never an LCP/suffix diff. Prefix-preserving replacements retain the complete operation range.
- Marked start deletes selected committed text and installs marked text before one native report. Engine ChangeText accounting still reflects only the committed deletion. Marked updates and cancellation use the previous full display, including the old composition. Updating the same marked text is an explicit replacement and still emits.
- Supplied replaced ranges take precedence over the old marked range. The internal model can represent only one marked insertion, so partial/overlapping updates reconstruct the affected marked storage while metadata retains only the actual supplied replacement. Preserved prefix/suffix text is not included in the reported mutation. Partial commit places the caret after the actual insertion, before a preserved suffix.
- Implicit commit and unmark that only move identical display text into committed storage emit `None` with composing=false. Explicit same-text replacement during composition remains `Some`. Cancellation emits deletion of the marked display range, without backspacing the preceding committed character. Native maxLength clipping on commit reports the actual accepted string.
- Backspace, forward delete, and cut report their actual removed ranges, including reversed selection and surrogate pairs. Deletion endpoint calculation uses the known delete operation's resulting caret and removed byte count, not a text diff.
- AX set-value is a known full-display replacement, including same-text replacement and removal of an active composition. Secure inputs still cannot register or produce native snapshots.
- Paste completion supplies the real sorted pre-selection range and accepted insertion. Empty attachment-only insertion is `None`, preserves text/selection, and still explicitly advances the paste serial. Count/ChangeText accounting remains before paste completion, including callback blur and stale engine-value rejection. Existing menu capability, held-key, focus epoch, registration, and retirement fences remain in place.
- Unknown text changes, including an unfenced engine value change while hooks are attached, are not inferred into mutations. The next observation retires hooks unless the text still matches the preceding observation. Parent must continue to suppress stale legacy projection while attached.

## Parent exports and development regressions

Export `InputMutation` alongside `InputEdit` from parent-owned public integration. Existing host regression functions remain in place; one legacy regression now uses the actual IME path rather than directly mutating model text without provenance. Existing paste regression additionally asserts mutation metadata and attachment-only `None`.

Add these nine functions to the parent-owned host exports/test wrappers:

1. `native_replacements_preserve_operation_ranges_and_same_text_serials`
2. `native_selection_and_reregistration_have_no_mutation`
3. `native_marked_update_commit_cancel_and_reversed_surrogates_are_exact`
4. `native_partial_and_overlapping_composition_ranges_preserve_untouched_text`
5. `native_deletes_cut_and_accessibility_report_real_ranges`
6. `unknown_native_mutations_retire_without_diff_or_fallback`
7. `native_clipping_and_explicit_same_text_composition_replacement_are_exact`
8. `native_same_text_paste_is_explicit_and_secret_snapshots_are_absent`
9. `native_opaque_history_is_not_reported_as_a_guessed_mutation`

There are 19 host regression functions in the owned file after this assignment. This is a source inventory, not a discovered or executed test count. Parent must run all existing host regressions plus these nine against the integrated final source, handle independent review, and archive/remove development Rust tests before master as required by repository policy.

## Native context-menu Undo follow-up acceptance

The independently confirmed menu action bypass calls the native field key handler without the application key policy. Repair only menu dispatch and scoped host regressions. Each actual `LocalItem::Item.action` must validate its captured mount/focus identity and hook registration, exclude native composition before policy, invoke the current guarded key policy exactly once without holding or replaying the gesture, and invoke native field fallback only if policy declines and the same target remains current. A handled Undo preserves hooks/focus epoch and never reaches opaque native history. A truly unhandled Undo retains hook retirement and consumes without a guessed mutation. Stale, inactive, hidden, replaced, or re-registered menu targets must not affect a new session. Existing secure clipboard checks, paste capability/accounting, and keyboard dispatch stay unchanged.

Required direct-action regressions cover accepted and rejected Undo, native marked text, stale focus/mount/registration, and a declining policy that changes its target before fallback. Parent exports/wraps/runs these tests and coordinates builds and independent review. No opaque-history redesign or application/export/shared-file changes belong to this follow-up.

## Native context-menu Undo follow-up delivery

Changed only `vendor/nsh-platform-gpui/src/text_input.rs`, `vendor/nsh-platform-gpui/src/text_input/host_tests.rs`, and this document. All three starting SHA256 values exactly matched the previous delivery. Parent supplied refreshed remote/branch evidence; no Git/network operation was performed here.

Menu construction captures the actual field's identity and hook registration before asynchronous clipboard capability inspection and validates them before opening. Each action captures those same target fences, rejects disabled/stale/inactive/hidden targets, excludes native composition using the existing composition-key fence, then invokes `guarded_key` exactly once. Only a decline with the still-current target reaches the original native field handler. No held-key queue, global shortcut dispatch, clipboard reread, native-history redesign, or keyboard-path change was added. A policy that changes focus or hook registration cannot accidentally send its declined gesture into the replacement session. Existing menu enablement and secure/paste implementations remain unchanged except the existing image-capability test now supplies the surface to the menu builder.

Parent-owned export/wrapper additions:

1. `native_menu_undo_calls_policy_once_and_unhandled_history_fails_closed`
2. `native_menu_undo_excludes_composition_without_queueing_policy`
3. `native_menu_undo_rejects_stale_focus_mount_registration_and_window`
4. `native_menu_undo_rechecks_target_after_declining_policy`

These call the actual enabled `LocalItem::Item.action`, not a duplicate menu-dispatch helper. Assertions cover exactly one policy invocation, original modifiers/identity, no native undo/text mutation/serial/count change, preserved hooks/focus epoch on acceptance, and retirement without guessed mutation on rejection. Other assertions cover marked CJK/surrogate display preservation, candidate-key queue fencing with no deferred menu gesture, post-unmark policy delivery, stale focus/remount/registration/window/Surface generation/cross-Surface rejection, and reentrant policy blur/rebinding before fallback. The owned source now contains 23 public host regression functions: source inventory only, not an executed/discovered count. Parent's supplied 25 earlier passing host regressions are historical integrated evidence, not validation of this follow-up.

Executed `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. Executed `rustfmt --edition 2024 --config skip_children=true` and the same with `--check`, targeting only the two authorized Rust files: exit 0. Read final formatted changed ranges and inspected the installed `nsh-core 0.4.0` model's actual insert/history API. Focused read-only Python checks passed for whitespace, exact hashes, single policy/fallback call sites, composition/identity/registration fences, no queued redispatch, no new code comments, and host function inventory. These are source checks, not type checking or behavioral test results. The first diagnostic Python command failed because its inline regex flag was misplaced; correcting only that diagnostic expression produced exit 0 without changing assertions or product source.

| Follow-up Rust file | Final SHA256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/text_input.rs` | `c06e7f65bf156a00c687d94b3fb7f4744ee6d49f11606bba1943c197167dac15` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `979e74b1318562935a3fa45c95fee5665f71a957a9a92f6c906aef012c1c0180` |

NOT RUN for this follow-up: Cargo/rustc version queries, compile/build/check/Clippy, any host/application tests, CI, native launch/manual acceptance, Git/index/commit operations, network, or independent review. Parent coordinates current-source validation and another Agent's independent review. Actual OS popup activation, application-authoritative Undo projection/mention preservation, real IME candidate handling, native focus transitions, secure clipboard and payload-paste behavior remain native integration checks; no runtime acceptance is claimed. Opaque native history remains intentionally fail-closed when the application declines it. No changes were made to core, Kit/Fast, dependencies, exports/manifests, reading, application integration, Surface, or shared files.

## Initial mutation delivery verification and exact source bytes

The following hashes and checks describe the preceding mutation delivery, not the menu follow-up above.

Executed scoped formatting and `--check` using `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`, edition 2024, `skip_children=true`, for only the three owned Rust files: exit 0. A read-only Python whitespace/hash/function-inventory check also exited 0. Formatting parses source; it does not type-check or execute assertions.

| Owned Rust file | SHA256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/editing.rs` | `edb6aa253041c64954f6416b4521d7dc768f869b6880a27c63c50326507532b5` |
| `vendor/nsh-platform-gpui/src/text_input.rs` | `30a22458340ce0cfc39c60c25a11a64254b502d765ee2966068c3d2727caa57a` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `ce45d574f8ff2f5f97b6cb75335d67114678471d708cf00ef84f0a2980b6683a` |

NOT RUN: Cargo/rustc version queries, builds, check, Clippy, host tests, application tests, CI, native launch/manual IME/AX/clipboard acceptance, network, Git, index operations, or independent review. The supplied earlier independent hash and parent test results are historical evidence only and do not validate these changed bytes. Parent owns unified validation and all exports/application integration. No core, Kit/Fast, manifests, shared Surface/application/reading/library files, or Git index was edited.
