# Single-line transactional paste correction

## Acceptance fixed before implementation

This is implementation of the P2 finding in `review-native-security.md`, not independent review. Parent supplies HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, fetched master `2a29adf9b2ea78eaf59d591838531b45adc7373e`, and frozen review manifest `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`. Dirty file hashes, not HEAD alone, identify the implementation. No Git, network, Cargo, vendor or dependency changes in this lane.

- Use the current explicit `NativeEditorPolicy`, never infer policy from `multiline` or image-paste admission. Default Composer stays unchanged.
- Match the actual legacy ordinary single-line paste at `state.rs:660–664`: replace LF with space, preserving CR. Consequently CRLF becomes CR followed by space; lone CR stays CR. Composer only replaces CRLF with LF, retaining LF and lone CR. This repair does not broaden the legacy normalization policy.
- Admission insertion, expected resulting text/directed UTF-16 selection, mutation validation and authoritative replacement must use the same policy. Original clipboard strings, opaque metadata, image entries and carrier identity remain immutable; only the derived inserted string changes.
- Capture current policy/session and focus epoch for paste; reject a changed policy/session or epoch before preparation, completion or authoritative commit. Existing owner/mount/register/composition/serial/unique carrier/budget fences remain.
- Preserve native submit acknowledgment, rename trim/persistence and all existing assertions, scheduler limits and deadlines. Add LF/CRLF/lone-CR, directed non-BMP selection, default Composer and stale-policy/focus regression coverage, including real mounted rename paste then Return persistence. No production rename changes without a concrete cause.
- No OS HTML/capability expansion, AX claim, backend change or self-review. Parent owns central build/run, independent delta review, final gates and development-test cleanup.

## Minimal plan

Centralize derived text normalization on `NativeEditorPolicy`; carry a bounded paste context through the existing bridge queue and use the policy under the authoritative commit lock. Wire the adapter and shared replacement helper to that contract. Extend existing development tests and mounted rename case without removing behavior checks. Record exact changed hashes and unrun checks, then stop READY for central execution.

## Implemented delta

- `editing_core.rs`: `NativeEditorPolicy::paste_text` is the single derived-string normalization contract. Each begun paste captures policy (including SingleLine session entity), policy-change epoch and focus epoch. Completed paste contexts are associated with their explicit serial in a queue bounded by the existing 64 pending edits. Preparation/completion reject changed context; authoritative `commit` rechecks it under the same state lock before application and removes it on acknowledgment. Policy epoch changes only when the policy changes; ordinary route refresh is not invalidation. Switching away and back cannot revive an old paste.
- `native_adapter.rs`: prepare uses the pending context's policy for both expected text/selection and the actual backend insertion. Existing native Weak allocation seal, unique completion ownership, reservation, metadata/image/URI checks and delivery remain.
- `nsh.rs`: expected resulting text, explicit mutation inserted bytes and authoritative replacement share that policy. Policy is retrieved for the actual queued serial before commit and context is revalidated under the commit lock. Historical helper tests now explicitly request Composer rather than implicitly relying on it. The nontransactional Composer fallback is unchanged.
- `rename_tests.rs`: extend the existing mounted case, without changing runner case count, with real native select-all over `中😀文`, selection acknowledgment, native paste carrying opaque metadata, accepted controller readiness, native Return and real file-backed title persistence for LF, CRLF and lone CR. Restore the earlier accepted title before the original blank/cancel/outside/rebind/navigation checks. No production rename changes.

Legacy CR preservation is verified from both `state.rs:660–664` and `input_handler.rs:51–68` → `bindings.rs:120–126`; no later CR sanitizer exists in that replacement path. SingleLine therefore intentionally expects `alpha\r beta` for CRLF, not a newly invented flatten-all-breaks rule.

## Added development regressions and next central verification

Automatically discovered UI tests:

- `text_input::native_adapter::tests::paste_policy_preserves_original_carrier_and_directed_utf16_replacement`: six policy/line-ending combinations, reversed non-BMP selection `(3,1)`, explicit normalized mutation, original item/metadata equality, pending/acknowledged readiness and carrier release.
- `text_input::native_adapter::tests::changed_paste_policy_session_or_focus_rejects_each_transaction_stage`: policy kind, SingleLine session, focus epoch and away/back policy changes, before prepare, before completion and before authority; no stale application or extra delivery.
- `text_input::nsh::recovery_tests::single_line_paste_authority_replaces_reversed_non_bmp_selection`: actual controller authority replacement and final byte/UTF-16 caret for each line ending, independent literal expected text.
- `text_input::nsh::recovery_tests::native_paste_hooks_adapter_authority_preserve_each_explicit_line_policy`: actual backend field/session hooks → adapter → authority, six combinations, original item/opaque metadata, explicit mutation, pending acknowledgment and original carrier release.

Parent should compile the UI tests and existing `nsh_reading_integration` feature runner together, then execute these four tests, existing editing/adapter/recovery tests (especially original image/metadata/full-budget/queue-discard coverage), and the unchanged 15-case reading runner including `mounted_rename_native_ack_ime_cancel_rebind_and_persistence`. Run final scoped Clippy/required gates centrally. No helper invocation is described as AX or OS clipboard acquisition.

## Actual validation and limitations

Executed direct `rustfmt --edition 2024 --config skip_children=true` on exactly the four authorized Rust files and final `--check`: exit 0. Formatter: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`. The initial formatting check reported formatting differences; those were formatted before the successful final check. Read-only Python final newline/trailing whitespace, frozen/current hash extraction and original mounted behavior/budget presence checks: exit 0. These are static checks, not runtime or independent-review evidence.

No Cargo/rustc, compilation, Clippy, test runner, application/OS/AX execution, network or Git command ran. All authored behavior tests and mounted persistence remain **unrun** pending parent central execution; no new green result or acceptance is claimed. No backend, dependencies, production sidebar, shared wiring, comments or suppressions changed. Rust development tests and exclusive wiring remain subject to the parent-owned pre-master cleanup rule. This lane is READY and stops writing; independent delta review belongs to the original non-implementing reviewer.

## Exact content identities

Frozen review manifest: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`.

| File | Frozen SHA-256 | READY SHA-256 |
| --- | --- | --- |
| `text_input/native_adapter.rs` | `d9144ef7b2e6626b3062e7a28945c06924cde48b38342cf7ea4badb149593155` | `7b4d9d0a7f87b04ff203d6091ac320754f1915e142a55c22e8b566d013add7d9` |
| `text_input/editing_core.rs` | `5ea00d864cb939b7f7868c4c85443a9dd603cf7804c892e8155a520c8969c1ac` | `cb037e051edcaea4eee4634b2e7b74435d31a3263c46b59ecc779a5ceff3c0f0` |
| `text_input/nsh.rs` | `8f71c554bc9643c50e03f8ad7ad4f4a78dc84ff789c3ffcd9ad88625d621dc59` | `fd29c6714f18c272a54c5ecc09f2526e294929a2e053e39b6d1b6b5ba23fad47` |
| `sidebar/threads_block/rename_tests.rs` | `42ada6277b0519fa6d3a54de5a7fca1ac4f79572c6973580897cab1ceb283139` | `5ef9e8df876dfddd8c54b7cb74c8308ed3b37374f24456b3227172bb100d34d1` |

All table paths are relative to `crates/vega_ui/src/`. The new document's self-excluding hash is reported in the final handoff.
