# Sidebar NSH migration

## Authorization and baseline

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

1. **Task rename and group editor input remain GPUI.** Original `RenameSession`, composition checks, blank/trim behavior, pending guards and persistence/route fences remain unchanged. With the pinned NSH host, `events.rs::on_key_down` routes unhandled text-input keys through the engine dispatcher after `InputBridge::key_down` returns false; `text_input.rs::key_down` has no submit/composition guard at the Return/Escape boundary. The NSH engine cannot truthfully infer native marked text from its own composing flag or `When::Always`. Required capability is a native-host-owned composition-aware Return/Escape interception and query bound to the active input, before dispatch, with composition commit not also submitting/cancelling. No permitted shared sidebar hook can supply this inside the pinned dependency. Do not fork/upgrade or weaken the existing editor to hide this gap.
2. **Dormant group/project/timeline routing remains dormant.** The production `render_organization` path uses pinned/projects/recents rather than calling `render_group_projection` / `render_timeline_projection`; the existing group editor renderer is not mounted there either. Reusable group label/color/action leaves and menu contents were migrated, but group editor/color-choice controls and some dormant projection chrome remain GPUI. Enabling a view/editor changes existing behavior and is not silently included. Parent must decide whether to authorize wiring existing `render_group_editor(window, cx)` and group/timeline projection routes; no new controller or duplicate projection should be added.
3. **Native visual chrome remains:** popup background/border/shadow and scroll-edge host, sidebar host background/divider/title-drag surface, delete dimming background, reorder insertion-line canvas, and some dormant projection containers. These are not claimed as migrated NSH controls. Window title controls continue to call `navigation::controls::title_controls` unchanged. Complete removal of visible GPUI chrome is not delivered.
4. **Running-state animation fidelity is not complete:** resting NSH status currently uses static running/waiting glyphs rather than the original spinner animation. There is no claim that this matches native animation or AX status announcements.
5. Native Tab/Shift-Tab/menu Escape, AX retained actions after recycle, focus restoration, double-click, reorder/scroll, wrapped error/disabled-reason sizing, themes and narrow windows are unverified. Actual GPUI key/focus/AX APIs are wired, but API wiring and pure development tests do not prove platform behavior. Especially verify popup focus transitions against the existing controller focus-out closing policy.

## Preserved controller contracts

No store worker/business replay was moved into the engine. Existing selection/open/create/archive/pin/delete/fork methods, task mutation fence, draft navigation guard, running-worker handling, operation generations, route/database fences and preference worker remain authoritative. Surface reload stops/reconstructs presentation only; old recovery IDs cannot activate a rebuilt business control. Public sidebar globals/actions/events and preference notice/delete overlay signatures remain. No navigation/window/public Surface, tool activity, commit panel, dependency, Git or configuration file was written.

## Parent actions and validation gaps

- Compile this exact source and run the sidebar/native-host regressions under parent CPU coordination. Baseline 124 tests predate and do not validate this delta. No compiler result is available from this worker.
- Run/remove authored development-only tests according to repository master-delivery policy; do not count these four as executed.
- Independent review and native hand testing are still required; final-head CI remains the parent's integration responsibility. No benchmark or new product gate is proposed.
- Shared native input capability required for safe NSH rename is described above; with the explicit no-fork/no-upgrade decision it remains blocked rather than represented by an unsafe engine workaround. No shared file hook was implemented.
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
