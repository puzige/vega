# #307 selector declaration integration repair

## Scope and baseline

READY for independent delta review, not a claim of a passing final CI gate or native/manual acceptance.

- Isolated worktree: `/Users/puzige/Workspace/worktrees/vega-307-cua-selector-fix`.
- Branch: `feat/307-cua-selector-fix`; verified HEAD `3e32543432d15952d986bda504ff703f63602073`.
- Parent supplied the prior fetch/update and created this worktree. This writer made no Git/index/commit changes and did not query CI. The original onboarding worktree was not used or written; its independent broad review remains separate.
- Only writes: `crates/vega_ui/src/cua_onboarding/native/macos.rs` and this new report. No nested `AGENTS.md` exists. `native.rs`, `interaction/platform.rs`, existing reports, acceptance, tests, dependencies and workflows are unchanged.
- Reviewed native source baseline SHA-256: `e5c65124751445bb5edcec0db7bc7fd27133aef8cb19fcc7f994598b0faf03f0`.
- Repaired native source SHA-256: `5c0d06a6f081960610860f1e176978031cb98218a78e7daa91c4309e23f3f251`.

## Original failure evidence and classification

Parent-inspected raw evidence, inherited rather than queried or rerun by this writer:

- Fully wired Actions run [39832532323](https://github.com/puzige/vega/actions/runs/39832532323) at the baseline above: Nextest job `120012146283` executed the unchanged 28 tests, 28 passed, 0 skipped; workflow discovery was 28. Release-script checks reported 5 + 15 + 40 success.
- Clippy job `120012145906` failed on `sel_registerName` being redeclared with a different signature under `-D clashing-extern-declarations` via `-D warnings`.
- `interaction/platform.rs:41` declares `fn sel_registerName(name: *const c_char) -> *const c_void`; baseline `cua_onboarding/native/macos.rs:65` declared the return as `Object`, an alias for `*mut c_void`.

This is a product integration declaration/lint defect, not an acceptance change, behavior-test failure or observation/harness defect. The prior test success does not prove the repaired final gate passes. No lint suppression, assertion change, additional test, or unchanged-CI retry is used to obtain a green result.

## Minimal repair and ABI equivalence

The source delta is one new alias, one registration return annotation, and 13 selector parameter annotations (15 added / 14 removed lines):

1. Add `type Selector = *const c_void` alongside the unchanged `Object = *mut c_void`.
2. Return `Selector` from `sel_registerName`, matching the existing crate declaration exactly after alias expansion.
3. Use `Selector` as the second argument of every typed ordinary `objc_msgSend` signature, and the third argument of the x86_64 `objc_msgSend_stret` signature.

Read-only SDK inspection confirmed `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/objc/objc.h:57` contains `typedef struct objc_selector *SEL;`. The Rust alias does not assert that the SDK typedef is const-qualified: it models the opaque selector as a pointer that this code does not mutate and agrees with the crate's existing FFI declaration. Changing raw pointer mutability does not change pointer representation or its native calling convention. No selector cast is introduced.

Receivers and object arguments/results remain `Object`; integer, BOOL and geometry types remain unchanged. Both architecture gates remain unchanged: arm64 ordinary aggregate return still returns the same `repr(C)` `NSRect`; x86_64 stret still receives the result pointer first, receiver second and selector third. Existing function bodies, selector strings, call expressions, symbols, transmute operations, safety comments, class lookup, autorelease/CF ownership, discovery, panel configuration and movement are byte-identical outside these annotations. No production behavior change is introduced.

### All 13 typed dispatch signatures audited

`Object` is `*mut c_void`; `Selector` is `*const c_void`. Each call still obtains its selector directly from `sel_registerName`.

| Dispatch helper | Repaired typed signature | Unchanged result/argument contract |
|---|---|---|
| `send_object` | `(Object, Selector) -> Object` | Object result |
| `send_object_arg` | `(Object, Selector, Object) -> Object` | One object argument, object result |
| `send_index` | `(Object, Selector, usize) -> Object` | NSUInteger array index, object result |
| `send_bool` | `(Object, Selector) -> ObjcBool` | BOOL is `bool` on arm64, `i8` on x86_64 |
| `send_bool_arg` | `(Object, Selector, Object) -> ObjcBool` | Object argument and same architecture-specific BOOL |
| `send_unsigned` | `(Object, Selector) -> usize` | NSUInteger result |
| `send_signed` | `(Object, Selector) -> isize` | NSInteger result |
| `send_pid` | `(Object, Selector) -> i32` | pid_t result |
| `set_unsigned` | `(Object, Selector, usize)` | NSUInteger argument, void result |
| `set_signed` | `(Object, Selector, isize)` | NSInteger argument, void result |
| `set_origin` | `(Object, Selector, NSPoint)` | Same by-value two-double point, void result |
| `send_rect`, arm64 | `(Object, Selector) -> NSRect` | Same ordinary `objc_msgSend` aggregate return |
| `send_rect`, x86_64 | `(*mut NSRect, Object, Selector)` | Same `objc_msgSend_stret` output pointer and void result |

There are 11 common signatures plus the two mutually exclusive architecture-specific rect signatures, 13 in the source and 12 per supported target. Static counts also confirmed 13 registration call sites plus the one extern declaration.

## Actual local validation

Only fast static/format/diff/hash inspection was performed. No Vega/native execution, compilation, build, package, cargo check, Clippy, Rust/Python tests or CI queries were run.

Commands were run from the isolated worktree. Final read-only checks repeat formatting, whitespace, scope, content-equivalence and hashes after this report is written.

| Check | Actual result / evidence boundary |
|---|---|
| `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/cua_onboarding/native/macos.rs` | Exit 0; no formatting changes required. Parsing/format check, not type checking. |
| `git diff --check` | Exit 0; no tracked whitespace diagnostics. |
| `git diff --no-index --check /dev/null docs/307/vega-issue-307-selector-declaration-fix.md` | New-file content-difference exit 1 is expected; require no whitespace diagnostics. |
| Baseline native source transformed only by the alias, registration return and selector-slot substitutions, piped to `cmp` against the repaired file | Exit 0, byte-identical. All other production text, including call expressions and SAFETY comments, is unchanged. |
| Complete diff and source readback; `rg` signature/registration counts | 13 typed dispatches and 13 registration call sites audited, including both architecture paths; only the two in-crate `sel_registerName` declarations exist, now alias-equivalent. |
| Added-line hygiene scan | No new comments, unwrap/expect/panic, tests or lint suppression. |
| Strict `git diff --name-only` / untracked-file scope audit | Only the permitted native source and this new report differ; no other changes. |
| Protected-file diff and SHA-256 checks | Wrapper, existing interaction declaration, historical reports and frozen tests unchanged. |

Preserved hashes:

| File | SHA-256 |
|---|---|
| `crates/vega_ui/src/cua_onboarding/native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `crates/vega_ui/src/interaction/platform.rs` | `2362532258c4d06bdfe0cfbb847a5ead9e132a899f740dd4551a312066a6ef77` |
| `crates/vega_ui/src/cua_onboarding/guidance_tests.rs` | `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2` |
| `crates/vega_runtime/src/computer_use/preparation_session_tests.rs` | `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a` |

Declaration counts remain 15 UI + 13 runtime; these counts are not local test execution. This document's exact hash is supplied separately in the READY handoff to avoid a self-referential hash.

## Remaining gates and handoff

- A non-writer must review this exact native delta, including selector constness and both aggregate-return paths. The previous native static PASS applies to the baseline hash, not automatically to this repair.
- Parent owns commit/integration after the original immutable broad review, then a full existing current-head cloud gate, including Clippy and the same frozen 28 tests. No repaired-version CI result is claimed here.
- Existing manual gaps are unchanged: real panel matching, multi-display placement/following, drag/drop, nonactivation, keyboard accessibility, Spaces/fullscreen, administrator-dialog visibility and real TCC/restart behavior. Pointer-annotation repair does not validate any of them.
- No acceptance, test, dependency, workflow, panel/permission/capture behavior, or other production-source change. No open implementation blocker remains for this narrow repair; renewed review and full CI remain pending.

Writes stop with this report; subsequent checks are read-only. The original onboarding worktree and its historical reports remain untouched by this followup.
