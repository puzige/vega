# Sidebar NSH migration

## Current repair: second mounted run command admission failure

The migration repair compiled centrally (parent: exit 0, 37.82s), then the real 15-case runner retained the original 14 PASS results but rename timed out at `acknowledged rename persisted` (`target/nsh-closure-repair-reading-rename.stderr`, then-current `rename_tests.rs:67`). This is a behavior failure, not the resolved missing-schema fixture failure. No post-repair green rename result exists yet.

Confirmed production cause in source: the adapter command-admission flag comes from `TextInput.image_paste` (`text_input/nsh.rs:979`), but task rename deliberately has no image paste. The prior `NativeAdapter::route_key` rejected Return/Escape before reaching explicit form policy. The repair permits the **current explicit SingleLine policy** through that gate (`native_adapter.rs:453–460`, `editing_core.rs:565–568`), without enabling image paste or changing native command token/readiness/IME/route guards. A new actual adapter regression uses flag=false and covers latest accepted submit, cancel, pending/composing suppression and policy replacement. Default composer behavior remains unchanged; original assertions are retained.

The original mounted assertions and budgets are unchanged. Development-only bounded timeout state now captures native/controller text (96 characters each), serial/selection/composition/policy, readiness/gate/focus/registration, binding/current route/generations, persisted title and pending/error state (`nsh.rs:443–468`, `rename.rs:17–31`, `rename_tests.rs:60–110`). Diagnostics do not confirm/cancel, retry or weaken acceptance. See [exact lifecycle, identities and next central commands](nsh-single-line-native-policy.md#second-run-repair-implementation-and-handoff). Direct scoped rustfmt check passed; no Cargo/rustc/build/test/network/Git mutation was run here. Parent must rebuild and execute; no independent review or fullnative acceptance is claimed.

## Earlier author implementation: explicit native rename policy and NSH mount

The parent has now delegated the application editing gap and task rename to this single author lane. The [explicit native single-line policy acceptance and minimum plan](nsh-single-line-native-policy.md) was written before the first Rust edit in this continuation. It supersedes the sidebar-only write restriction/API handoff below, not dormant group-editor scope. This is implementation, not independent review or fullnative acceptance.

Implemented source delta (not compiled or executed here):

- `text_input/editing_core.rs:198–216,739–755`: default `NativeEditorPolicy::Composer`, explicit `SingleLine(EntityId)` policy; only explicit single-line routes claim ordinary unshifted Return Submit / Escape Cancel. Policy/session identity is part of `NativeEditorRoute` and therefore existing token equality validation. Existing composition-first, owner/focus, serial/settled/paste/repeat and one-claim guards are retained.
- `text_input/nsh.rs:130–151,775–782,1184–1195`: `ComposerRoute::single_line(session)` and crate-visible `render_nsh_single_line` use the same transactional bridge and carry actual policy into the native route. Default `render_nsh_input` remains Composer; no multiline inference or popup flag workaround.
- `sidebar/threads_block.rs:492–517,1472–1473` → `threads_block/rename.rs:97–123`: current production recent/project/pinned row rename mounts `RenameMount` and the real NSH editor for the original `RenameSession.input`, no old GPUI TextInput child. The local RenderOnce carries the actual Window without changing row/group signatures. The Fast wrapper is layout/focus/outside-pointer only, with no visual paint styles.
- `threads_block/rename.rs:16–30,34–85`: each binding retains an entity identity and captures input, row, thread, project/navigation generations and current route. The session-owned `subscribe_in` requires current binding, current thread existence, actual focus/readiness and `ComposerShortcut::matches` against the identity-bearing single-line route. Same-input rebinds cannot inherit old commands. Old `ConfirmRename` / `CloseSettings` handlers and `ThreadRename` context are removed from this mount; native Submit/Cancel uses adapter → token → dispatch → subscription only. Outside callbacks validate binding before intentional cancellation, including pending edits.
- `threads_block.rs:519–555` keeps the existing latest-authoritative input read, `resolve_rename`, trim/blank, `apply_update`, pending and async persistence paths. No duplicate business controller or editor state was introduced.
- `text_input/nsh.rs:306–412`: original input chrome is NSH, conditional on explicit single-line route and `bare_chrome == false`: original rem-based horizontal/vertical insets, 1px border, radius and elevated fill. BODY font 13px × 1.55 = 20.15px remains in the native editor. At a 16px rem, horizontal inset 8px + border 1px = 9px, matching `SIDEBAR_RENAME_CHROME_INSET`; vertical input height is 20.15 + 2×4 + 2×1 = 30.15px inside the existing 32px row. These are source calculations, not pixel verification.

Development regressions authored: two actual adapter cases, one bridge native-before-engine/focus case, and `mounted_rename_native_ack_ime_cancel_rebind_and_persistence` in new `rename_tests.rs`. The mounted case uses a real temporary SQLite project/thread and actual NSH native focus, AX replace, type, mark/unmark, paste, undo/redo, key delivery and outside-click operations. It covers pending Enter without replay, accepted-text persistence/trim, blank cancellation, IME candidate Return/Escape, native Escape, outside cancellation, reopen, same-input rebind, replaced session and navigation fence. Bounds remain 512 scheduler ticks/2 seconds per drain and 10 seconds per stage. Existing default composer regression assertions and all production budgets are unchanged. Tests are unrun and must be archived/removed before master under repository policy.

### Parent integration and validation still required

1. In parent-owned `conversation_stream/nsh_composer.rs:21`, add `policy: Default::default(),` to its exhaustive `ComposerRoute` initializer. This lane deliberately did not write conversation code. This is a known compile prerequisite, not an API blocker left for another author.
2. Under existing `nsh-reading-integration`, re-export `threads_block::mounted_rename_native_ack_ime_cancel_rebind_and_persistence` from `sidebar/mod.rs` and register `vega_ui::sidebar::mounted_rename_native_ack_ime_cancel_rebind_and_persistence` in `tests/nsh_reading_integration.rs`, increasing its cases array length. Inner module/feature wiring is already present in owned `threads_block.rs`. Parent owns exports/Cargo/harness registration.
3. Run the exact commands in [the policy verification handoff](nsh-single-line-native-policy.md#verification-handoff), then final-version strict checks and native/user acceptance. Actual compile/type/borrow correctness, mounted timing, pixels, OS IME and three-desktop behavior remain unverified here. No Rust/Cargo/compiler, network or Git command was run in this continuation; direct owned-file rustfmt/check completed successfully.

## Historical sidebar-only rename acceptance and minimum plan

At that stopped stage, this section superseded the historical task-rename IME blocker below, not the dormant group-editor scope. The current user authorizes the existing native editing bridge for task rename. This is implementation work, not independent review or fullnative acceptance. Parent reports fetch/update against `origin/master` `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`, task remote equal to HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, with dirty work preserved. Read-only HEAD inspection agrees; remote freshness is parent-reported, not re-fetched here.

Current verified input identities before Rust changes:

- `crates/vega_ui/src/sidebar/threads_block.rs`: SHA-256 `3d73bf3986074a74171e1e4a75296b0c377c5cbb73368b0309b2a6ec1fc19569`.
- `crates/vega_ui/src/text_input/nsh.rs`: SHA-256 `6c330e1ab0d8695a1ed24b032954353fd955f858fc1e5ff0e16936907428205c` (read-only dependency).

Acceptance fixed before implementation:

1. Every currently mounted task-title rename uses `TextInput::render_nsh_input`, retaining the existing `RenameSession.input` authority, not a mirror, hidden GPUI editor or fallback. GPUI may supply layout, focus, events and controller subscriptions only.
2. Subscribe to that input's `ComposerShortcut`; bind the current input entity, thread, row and route. Reject replaced sessions, moved rows/routes, removed threads, mismatched shortcut stamps and non-ready/composing editors. Native `Submit` / `Cancel` continue through the shared command token, `NativeAdapter` and `dispatch_native_command`; no sidebar raw Return/Escape interception may bypass them. IME candidate confirmation/dismissal must not commit/cancel rename.
3. Enter reads the latest acknowledged authoritative input and reuses `commit_rename` / `resolve_rename` / `apply_update`: trim, blank cancellation, pending guards, persistence and async routing fences remain. Escape and outside click cancel without persistence; retained callbacks cannot cancel a replacement session. An outside click may discard an unacknowledged local edit intentionally, but never submits it.
4. Preserve existing row insets, width, input BODY typography, native focus/selection, IME, paste and undo through the existing bridge. No extra product copy, dependency, queue/budget change or shared API change.
5. Group/dormant routing, separator/mounted ownership, conversation and unrelated business bugs remain outside this lane. Writes are limited to `threads_block.rs`, new `threads_block/rename.rs` and meaningful development-only `rename_tests.rs` if feasible, plus this document and `nsh-shell-workspace-production-closure.md`.

Minimum plan: use a sidebar-local `RenderOnce` mount to carry the real `Window` into `render_nsh_input` without changing organization row signatures; bind a session-owned native shortcut subscription with captured identity; remove ungated old action handlers; preserve the controller mutation path. Add scoped host regressions if existing APIs can exercise actual native acknowledgment/dispatch without fake algorithms, source assertions, dependencies or expanded production APIs. Run only allowed direct formatting/static checks; parent owns all Cargo/rustc/build/test execution and final gates. Record exact resulting hashes and unrun commands at handoff.

## Historical stopped implementation: blocked on native single-line command policy

The acceptance/plan above was written before any Rust edit. **No Rust edit has been made.** The reusable native edit/acknowledgment bridge exists, so the old blanket “NSH cannot support IME-safe editing” claim is obsolete. A narrower current API gap prevents a correct sidebar-only switch: the default route does not generate unmodified Enter/ Escape rename commands.

Current callchain evidence (repository-relative):

- `crates/vega_ui/src/text_input/nsh.rs:692–706`: `render_nsh_input` uses `ComposerRoute::default()` and transactional native rendering.
- `text_input/nsh.rs:130–141,1093–1103`: neither the composer route nor its `NativeEditorRoute` projection carries a single-line submit/cancel policy; popup is false by default.
- `text_input/native_adapter.rs:453–495`: actual native Return/Escape translation delegates to `bridge.key_with_context`, and only a returned token enters `NativeDelivery::Command`.
- `text_input/editing_core.rs:715–723,731–779`: composition suppression exists. Submit is generated for platform-primary Return; plain Return produces Accept only for a popup or prediction. Plain Escape produces Cancel only for a popup, or DismissPrediction for a prediction. With the default rename route and no prediction, both fall through to Pass, not Submit/Cancel.
- `text_input/nsh.rs:630–689`: `dispatch_native_command` validates tokens and converts Submit/Cancel into `ComposerShortcut`, but cannot manufacture a token for keys the bridge passes through.
- `text_input/native_adapter.rs:627–649`: an existing unrun regression explicitly expects plain Return to pass and primary Return to submit. This is supporting source evidence, not an executed test result.
- `sidebar/threads_block.rs:1477–1478,1898–1915`: current rename still mounts the old entity and raw `ConfirmRename` / `CloseSettings` handlers. Simply replacing its child while retaining these handlers bypasses native command identity and IME safety; removing them without shared policy loses ordinary Enter/Escape.

### Exact minimal request to parent/shared editing owner

Provide an explicit native **single-line form/rename** command policy, propagated through the existing renderer, `ComposerRoute`, `NativeEditorRoute` and token validation. `render_nsh_input` must select that policy for this single-line input (or expose a dedicated crate-visible rendering entry plus its matching route for sidebar subscription). For a current focused, settled, non-composing input, plain Return must emit native Submit and plain Escape native Cancel through the existing `NativeAdapter` → `dispatch_native_command` → `ComposerShortcut` path. Pending edits, pending paste, repeat, stale owner/focus/route and IME candidate events must retain existing rejection/suppression. Keep current multiline composer behavior and queue/byte budgets unchanged. Parent/shared owner determines the smallest compatible API shape; this lane does not authorize a broader bridge redesign.

After that API exists, this lane can implement the planned session-owned subscription and NSH mount using `event.matches(input, matching_route)`, `composer_editor_ready` and actual focus plus captured input/thread/row/route identity. Do not set fake popup/files/actions flags, synthesize command tokens, mutate bridge routes behind the renderer, or retain ungated key handlers as a substitute.

No `rename.rs` / `rename_tests.rs` was created: a sidebar host test cannot honestly prove the required native plain-key route before the shared bridge supplies it, and a fake algorithm/source assertion is not a substitute. No build, Cargo, rustc, test, network or Git mutation was run. No native interaction, independent review or fullnative acceptance is claimed.

Additional current read-only SHA-256 identities:

- `crates/vega_ui/src/text_input/editing_core.rs`: `88f8bdf4d21dd84f856785f346166146bbe91834fc90d2911c28ef2f454b4146`.
- `crates/vega_ui/src/text_input/native_adapter.rs`: `d724f6bb29b56cabe7390d359a293f690aa5a3efaa0cfa3ed0a4c585f9f81ab1`.

Parent next verification after the shared policy and sidebar wiring are implemented: `cargo test -p vega_ui text_input::native_adapter::tests::actual_key_translation_keeps_newline_and_single_claim_submit_separate -- --exact` to retain existing multiline behavior, then focused new single-line native-host/rename development regressions and `cargo clippy -p vega_ui --all-targets -- -D warnings`. These commands are **not run** here; no new rename test name/result exists yet. Required development coverage: acknowledged latest-text persistence and trim/blank; in-flight edit then Enter; candidate Enter/Escape; ordinary Escape/outside cancellation; replaced session/row/thread/route rejection; actual native paste/undo/focus. Keep existing budgets and remove development Rust tests under repository delivery policy before master.

## Historical authorization and baseline

Owned writes: `crates/vega_ui/src/sidebar/**` and this document only. No nested delegation, Git/index/network/dependency/configuration changes, builds, tests, application execution or installation. Parent reports remote fetch/update and integration of `origin/master` `34555cec9c0c4c4c1f7a528d739752f414e53edf` this cycle. Assigned branch `feat/907-nsh-full`, starting HEAD `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`; no Git command is used to reverify it.

Starting SHA256 verified from disk:

- `mod.rs`: `d6f49ca1dd44e07371a93519cd309406329b4d746ed94869e4477d31068f1401`
- `projects_block.rs`: `b96bc8d3849920c5ec9d6d0d371725b3580c8443c7e4be5f3af57b20e546d823`
- `threads_block.rs`: `8b5553b921a37f96a968b46d8bc9f64e14b9640b84d4c28482c02d9cf21b6667`

## Acceptance before implementation

The goal is native NSH sidebar presentation, not embedding GPUI controls in an NSH view. Native hosts may provide layout, focus, accessibility and services. NSH engine code must contain presentation and command transport only; original controllers remain authoritative.

| Slice | Acceptance / preserved contracts |
| --- | --- |
| Shell | Existing width, toolbar/traffic-light reserve, title dragging, narrow/show rules, settings entry, new task and inline errors. Reuse `navigation::controls` unchanged. |
| Organization | One existing project/task projection; pinned/projects/recents/group ordering, persistence, collapse, progressive growth and scroll retained. Native host owns reorder gestures. |
| Rows | Actual project/task labels, selected/unread/running state, fixed action slots, pointer/keyboard/AX activation and row lease invalidation. Recycled or removed rows must reject retained actions. |
| Menus | Actual NSH menu contents in existing window-level deferred placement, not clipped by row Surface bounds. Stale menu entries cannot act on changed controller/menu state. |
| Rename | Existing blank cancellation, trimmed title, pending guards, input composition, draft/session identity and route fences cannot be weakened. No engine-only composing flag or `When::Always` claim of native IME safety. |
| Mutations | Existing delete confirmation, running workers, draft navigation guard, store authority, async operation generations and route/database fences remain authoritative. |
| Recovery | Errors stay visible; retries remain existing controller operations. Surface recovery reconstructs presentation only and never replays a business command. Ordered channel sends must not silently drop requests. |
| Isolation | `preferences.rs` remains unchanged. No navigation/window/public Surface modifications; missing shared APIs are reported to parent. |

## Review correction: task-menu focus restoration

Static review confirmed that Escape removed the temporary focused menu leaf without restoring focus to the surviving task-actions trigger. The correction must give that trigger's actual NSH leaf the controller-owned row FocusHandle, then restore that handle on explicit menu dismissal. An outer ancestor handle alone is not sufficient. Business actions that intentionally transfer focus remain unchanged. Required native regression: Tab to More → Enter → Escape → focus remains on More → Enter reopens; Shift-Tab continues to the preceding live target. The correction is statically closed by a non-implementer and subsequently typechecked under strict all-target Clippy. All four existing sidebar development regressions passed in integrated Nextest run `3cf61ac2-a25a-4720-96c4-2e3630be7a15` (166run/166passed/0skipped). The native focus/reopen regression remains unrun, not passed.

## Actual contracts inspected

`Surface::new` accepts `fn(Signal<S>, mpsc::Sender<C>) -> AnyElement`, publishes watch snapshots using `SignalEquals::Never`, and exposes error watch, environment synchronization, entity/focus and presentation lifecycle. `ProjectionId::matches` is identity comparison, not `PartialEq`. `OrderedCommands` serializes awaited bounded sends and exposes a failure signal. It is not an acknowledgment of persistence.

The existing navigation control host uses genuine GPUI key events, tracked focus, an accessibility action proxy, and mount leases; ordinary NSH `View.focusable` is not sufficient proof of native Tab/AX activation. Dependency versions remain NSH 0.4 / Kit 0.7.3 / Fast 0.1.9, with no fork or upgrade. Existing organization controller services, projection, reorder and menu positioning are retained rather than copied into the engine.

## Verification plan and status

Allowed: focused source inspection, direct rustfmt and static checks limited to owned files. Authored Rust development regressions, if any, are not executed here and must be removed under the repository delivery policy before master. Parent coordinates compilation, regressions, independent review and final head CI. Earlier baseline 124-test results do not cover this delta. Native keyboard/AX/IME, scroll, themes, narrow windows and all desktop-platform behavior require parent/user verification; no performance benchmark or new product gate is introduced.

## Implemented slices (not compiled or native-validated)

- Added `sidebar::nsh_leaf`: actual NSH text/SVG/action rendering, the repository `OrderedCommands` transport, `ProjectionId::matches`, native GPUI focus/key routing and an AX activation proxy. Mount leases and unique mounted element IDs invalidate retained actions after recycling. Requests also check the live controller guard and captured route, selected project, settings/collapse state, task mutation generation/pending count, draft input identity/text/composition and database path. Controller guard transitions and changed presentation invalidate the projection without a persistent-false-guard remount loop.
- Migrated New task and Settings (including existing shortcut hints), preference-save error/retry, inline error content and delete confirmation content. Public APIs and original handlers remain; confirmation still calls `Sidebar::confirm_pending_delete`. `preferences.rs` was not written.
- Migrated production task titles, unread emphasis, pin icon, resting status text, quick pin/archive actions and keyboard/context More. Native row hover, context events, contextual focus scope, reorder gestures and geometry remain. Ordinary GPUI row interaction paint was removed. Existing title editor remains explicitly unmigrated.
- Migrated production project labels/folder icons and project/section actions, pinned/project/recent section labels, archived view actions, progressive show-more/collapse, and drag-preview content. Original projection, progressive state, persistence and service/controller methods remain.
- Migrated thread and organization menu contents, with captured thread/menu/projection guards and retained disabled-fork reasons. Existing anchored/deferred window-level placement and scrolling remain. NSH menu hosts implement native Up/Down/Home/End/Tab selection, original next/previous/activate action bindings and scroll reveal, with focus/AX selection matched to the active item.
- Migrated standalone `ProjectsBlock` sort/add/select/remove and standalone `ThreadsBlock` section/archive/empty states. These are not claims that unused fallback paths are newly selected in production.
- Added four **unrun** development regressions in `nsh_leaf/tests.rs`, using explicit imports: remount/recycle rejection; independent authority/controller/drag fences; quiet/disabled keyboard targets; recovery invalidation. Production activation uses the same acceptance predicate. No native AX, root-layout, IME or integration coverage is claimed.

## Incomplete and blocked acceptance

This is a partial production presentation migration, **not full sidebar acceptance**.

1. **Historical blocker, superseded for task rename by the current explicit policy implementation above; dormant group editor is still out of scope. Task rename and group editor input remained GPUI in this earlier snapshot.** Original `RenameSession`, composition checks, blank/trim behavior, pending guards and persistence/route fences remain unchanged. With the pinned NSH host, `events.rs::on_key_down` routes unhandled text-input keys through the engine dispatcher after `InputBridge::key_down` returns false; `text_input.rs::key_down` has no submit/composition guard at the Return/Escape boundary. The NSH engine cannot truthfully infer native marked text from its own composing flag or `When::Always`. Required capability is a native-host-owned composition-aware Return/Escape interception and query bound to the active input, before dispatch, with composition commit not also submitting/cancelling. No permitted shared sidebar hook can supply this inside the pinned dependency. Do not fork/upgrade or weaken the existing editor to hide this gap.
2. **Dormant group/project/timeline routing remains dormant.** The production `render_organization` path uses pinned/projects/recents rather than calling `render_group_projection` / `render_timeline_projection`; the existing group editor renderer is not mounted there either. Reusable group label/color/action leaves and menu contents were migrated, but group editor/color-choice controls and some dormant projection chrome remain GPUI. Enabling a view/editor changes existing behavior and is not silently included. Parent must decide whether to authorize wiring existing `render_group_editor(window, cx)` and group/timeline projection routes; no new controller or duplicate projection should be added.
3. **Native visual chrome remains:** popup scroll-edge host, sidebar host background/divider/title-drag surface, reorder insertion-line canvas, and some dormant projection containers. The next Agent3 batch migrates popup background/border/shadow and delete dimming background to actual NSH decoration Surfaces; exact acceptance, implementation status and verification gaps are in [sidebar residual migration](nsh-sidebar-residual.md). This delta is scoped-rustfmt/static inspected only, not compiled or native-validated. Window title controls continue to call `navigation::controls::title_controls` unchanged. Complete removal of visible GPUI chrome is not delivered.
4. **Running-state animation fidelity is not complete:** resting NSH status currently uses static running/waiting glyphs rather than the original spinner animation. There is no claim that this matches native animation or AX status announcements.
5. Native Tab/Shift-Tab/menu Escape, AX retained actions after recycle, focus restoration, double-click, reorder/scroll, wrapped error/disabled-reason sizing, themes and narrow windows are unverified. Actual GPUI key/focus/AX APIs are wired, but API wiring and pure development tests do not prove platform behavior. Especially verify popup focus transitions against the existing controller focus-out closing policy.

## Preserved controller contracts

No store worker/business replay was moved into the engine. Existing selection/open/create/archive/pin/delete/fork methods, task mutation fence, draft navigation guard, running-worker handling, operation generations, route/database fences and preference worker remain authoritative. Surface reload stops/reconstructs presentation only; old recovery IDs cannot activate a rebuilt business control. Public sidebar globals/actions/events and preference notice/delete overlay signatures remain. No navigation/window/public Surface, tool activity, commit panel, dependency, Git or configuration file was written.

## Parent actions and validation gaps

- Compile this exact source and run the sidebar/native-host regressions under parent CPU coordination. Baseline 124 tests predate and do not validate this delta. No compiler result is available from this worker.
- Run/remove authored development-only tests according to repository master-delivery policy; do not count these four as executed.
- Independent review and native hand testing are still required; final-head CI remains the parent's integration responsibility. No benchmark or new product gate is proposed.
- Historical shared-input blocker: see the current acceptance and precise single-line native command-policy request above. The native edit bridge now exists, but this lane has not implemented task rename or modified shared hooks.
- Review dormant group/editor routing separately before authorizing a behavior change. Window title host APIs need no change for the completed slices.

## Static commands actually run

- Direct `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --version`: `rustfmt 1.9.0-stable (c397dae80b 2026-09-30)`.
- Direct rustfmt with `--edition 2024 --config skip_children=true` on the eleven owned Rust files listed below; final same-file `--check` completed successfully. No Cargo command was run. The toolchain directory name is not a recorded `rustc --version`; rustc/Cargo versions were not queried.
- Python/read-only source scans: trailing whitespace empty; new leaf/test source has no comments, `try_send`, `.unwrap()` or `.expect(`. Focused `rg`/`sed`/file reads inspected renderer, channel, projection, input, keyboard/AX, controller and deferred-menu contracts. `find crates docs -name AGENTS.md` returned no nested rule files.
- SHA256 inventory below. No Git/index/network, builds, tests, application launch, install or dependency/configuration mutation was performed.

## Owned source hashes at handoff

Paths below are relative to `crates/vega_ui/src/sidebar/`.

| File | SHA256 |
| --- | --- |
| `mod.rs` | `d594d935056a42d65ab56d114297b6e0aa50ef2a251188e286ad5fe397936589` |
| `projects_block.rs` | `88099bb037a5eed28a948cd178a0928415349ec17efc12bb3b2eaa4f8981f760` |
| `threads_block.rs` | `22bfa371dd634a3debe69ac99f05b1d321e1d987df929e9a769b246040637468` |
| `row_helpers.rs` | `5cf47e1a0f7256111eaa2e06754df5e4ec70d888fbac66f18a451f1537398343` |
| `nsh_leaf.rs` | `ae2a7dc832f2af4cb73aa28c412900238cde0567b0fc6bd4e63061e6a317c449` |
| `nsh_leaf/tests.rs` | `25c5b98d3477eea049839814cfaddd10cc46489a3e3d89a9a013d736c13c73c3` |
| `threads_block/organization.rs` | `a0f2ff39d61ca8dac9ff7cc31997f264b9137f3eb4faac795750fbe895d72502` |
| `threads_block/organization/render.rs` | `4a4ac44bc1b5efd4709524ca7d97a5206249af5e6e182718322485552819b9e6` |
| `threads_block/organization/menu.rs` | `4c8f81a34851411187f14a785c5bbca98b8ce1df71404ac3b143efa054c3701b` |
| `threads_block/organization/projections.rs` | `d380671a636be943b5e519931e890460c3595ac7759ec35742ee1f9fdd5311c3` |
| `threads_block/organization/reorder.rs` | `303972a9182967c9347210adbb506f715c017b537ebc238dd314a82af036043c` |

Read-only reference `preferences.rs`: `cd56db75bbea57a4eb1849cf567ee17dae200c2492db10f3ced07254875bfd16`. This document's own final hash is emitted separately to avoid self-referential hashing.
