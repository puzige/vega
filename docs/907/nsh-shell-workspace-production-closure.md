# #907 — Shell/workspace production implementation-gap inventory

## Current author repair: native command admission, final runtime result pending

Central migration-repair compilation succeeded (parent reports exit 0, 37.82s), and the actual reading/rename runner recorded 14 PASS / rename FAIL at `acknowledged rename persisted` after the existing ten-second stage budget. The original missing-projects-table fixture blocker is resolved. Parent's policy initializer/export/fifteenth-case registration is already integrated; the historical prerequisites below are no longer outstanding. Exact raw artifacts and binary identity are recorded in [the single-line policy repair record](nsh-single-line-native-policy.md#second-mounted-execution-command-admission-repair-acceptance).

Source-established production defect: `NativeAdapter`'s legacy command flag is `TextInput.image_paste`, false for task rename (`text_input/nsh.rs:979`, `sidebar/threads_block.rs:507–510`), so its old gate rejected plain Return/Escape before explicit SingleLine policy. The current repair admits the live explicit SingleLine policy at that gate (`text_input/native_adapter.rs:453–460`, `editing_core.rs:565–568`), retaining all native composition/readiness/serial/identity/token and session subscription gates. No image-paste capability, fallback editor or replay was added. A flag=false actual adapter regression and bounded actual mounted timeout diagnostics were added; no mounted assertion or budget was changed. This source repair is not a green behavior result.

[Current handoff](nsh-single-line-native-policy.md#second-run-repair-implementation-and-handoff) lists final dirty-source hashes, lifecycle evidence and central rerun commands. Direct scoped rustfmt check passed. This lane ran no Cargo/rustc/build/test/network/Git mutation and did not reopen any unrelated surface, separator or conversation work. Parent must rebuild and execute. This remains author implementation, not independent review, and fullnative acceptance is not claimed.

## Earlier author implementation: rename migrated in source, central validation pending

The user subsequently delegated the concrete application-editing policy gap and rename repair to this same single writer. The [single-line policy spec/acceptance/minimum plan](nsh-single-line-native-policy.md) was added before Rust edits. This supersedes the stopped sidebar-only handoff below. Parent-reported HEAD/master/fetch identities remain unchanged; no network or Git mutation occurred here. This is author implementation, **not consolidated independent review**.

Current production entry: sidebar task action `start_rename` (`crates/vega_ui/src/sidebar/threads_block.rs:492–517`) → existing row selector (`:1402–1406`) → NSH rename mount (`:1472–1473`) → `threads_block/rename.rs:97–123` → `TextInput::render_nsh_single_line` (`text_input/nsh.rs:775–782`). The old `.child(session.input.clone())` GPUI visual editor and raw `ConfirmRename`/`CloseSettings` handlers are removed from that live mount. The GPUI wrapper supplies flex/insets, focus and outside-pointer cancellation only; editor pixels/chrome are NSH (`text_input/nsh.rs:306–412`). There is no hidden GPUI widget, mirror or editor fallback in this replacement.

`NativeEditorPolicy::Composer` remains default; opt-in `SingleLine(EntityId)` travels through composer/native routes and token equality (`text_input/editing_core.rs:198–216,739–755`; `text_input/nsh.rs:130–151,1184–1195`). Plain Return/Escape now use existing NativeAdapter/token/dispatch/ComposerShortcut delivery for the explicit policy only. Existing native-before-engine composition guard, acknowledgment, owner/focus/serial/settled/paste/repeat/single-claim fences remain. The session-owned subscription checks current input/session/row/thread/route/generations and native focus/readiness (`threads_block/rename.rs:16–85`) before original `commit_rename`/`cancel_rename` (`threads_block.rs:519–555`). Same-input rebinds use a fresh route session identity; outside cancellation checks the retained binding. Existing trim/blank, apply_update/persistence/pending and asynchronous controller fences remain authoritative.

Development coverage was authored, not run: actual adapter/bridge receipt/composition/policy/focus cases and one bounded actual mounted rename editor/controller/SQLite scenario covering accepted text, pending Enter, native IME keys, paste/undo/redo, trim/blank, native Escape/outside click, reopen, rebind/replaced session and navigation fence. No build/test/native visual or OS-candidate result is available. Direct scoped rustfmt/check passed; that does not establish type or runtime correctness. Original row/input geometry is preserved by source calculation, not pixel acceptance.

**Concrete parent integration prerequisites:** add `policy: Default::default(),` to the exhaustive `conversation_stream/nsh_composer.rs:21` initializer; feature-gated export of the new mounted case from `sidebar/mod.rs`; register it in existing `tests/nsh_reading_integration.rs`. These parent-owned files were not written. Exact source hashes, current implementation details, test names/commands and all remaining factual uncertainty are in [sidebar implementation](nsh-sidebar.md#current-implementation-explicit-native-rename-policy-and-nsh-mount) and [policy verification handoff](nsh-single-line-native-policy.md#verification-handoff).

This closes the inventory's confirmed production rename visual gap **in authored source only**, pending those minimal parent integration edits and central compile/tests/native acceptance. It does not resurvey or update unrelated shell/workspace/conversation routes, separator/mounted work, shared icons/tooltips or the already-documented codeheader benchmark/permission path. The historical inventory and hashes below remain explicitly stopped-version evidence. No fullnative acceptance is claimed. This writer stops at handoff.

## Historical author implementation attempt: blocked, rename not yet migrated

The later user request authorizes fixing only task-title rename, with writes confined to `sidebar/threads_block.rs`, new `sidebar/threads_block/rename.rs` / development `rename_tests.rs` if appropriate, and these two sidebar/inventory documents. Parent reports completed remote fetch/update (`origin/master` `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`, task remote equal to HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`) with dirty work preserved. No network or Git mutation was performed in this lane. This remains **author implementation work, not independent review**. The historical inventory below is retained as its stopped-version evidence, not silently restated as a new complete inventory.

The current acceptance and minimum implementation plan were added to [nsh-sidebar.md](nsh-sidebar.md) before any Rust edit. The plan reuses `RenameSession.input`, existing `commit_rename` / `apply_update`, actual native edit acknowledgments and `ComposerShortcut` identity/ready/focus validation, with a sidebar-local `RenderOnce` mount supplying `Window` without changing organization/dormant routes.

**Result: no Rust changed; the confirmed production rename gap remains open.** The current native editing bridge exists, superseding the historical blanket IME blocker, but source inspection disproves the assumption that its default route emits plain Enter Submit / Escape Cancel:

- `crates/vega_ui/src/text_input/nsh.rs:692–706` uses `ComposerRoute::default()`; `:130–141,1093–1103` exposes/projects no single-line submit/cancel policy.
- `crates/vega_ui/src/text_input/native_adapter.rs:453–495` translates native Return/Escape through `key_with_context`; a token is delivered only when that policy returns a command.
- `crates/vega_ui/src/text_input/editing_core.rs:715–723,731–779` already suppresses composition, but Submit requires primary-modified Return. Plain Return/Escape need popup/prediction routes; default no-popup/no-prediction inputs return Pass. The unrun existing regression `native_adapter.rs:627–649` explicitly expects plain Return to pass.
- `crates/vega_ui/src/text_input/nsh.rs:630–689` validates/dispatches an existing token to `ComposerShortcut`; it does not fill the missing plain-key command policy. Retaining sidebar raw action handlers would bypass that boundary, and fake popup flags or token synthesis are not acceptable repairs.

**Minimal shared-owner request:** add an explicit native single-line form/rename policy carried through the renderer and validated route so current, focused, settled, non-composing plain Return/Escape emit Submit/Cancel through the existing adapter/token/dispatch path. `render_nsh_input` must select that policy for single-line rename, or a dedicated crate-visible entry must expose its matching subscription route. Preserve multiline composer behavior, composition/candidate suppression, pending-edit/paste/repeat rejection, owner/focus/route fences and existing budgets. No shared editing/native-core/export/Cargo/vendor writes are authorized to this lane, so this request goes to parent rather than a sidebar workaround. Full details and next verification commands are in [nsh-sidebar.md](nsh-sidebar.md).

Current SHA-256 identities rechecked at handoff (HEAD alone does not identify these dirty bytes):

| Source | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/sidebar/threads_block.rs` (unchanged) | `3d73bf3986074a74171e1e4a75296b0c377c5cbb73368b0309b2a6ec1fc19569` |
| `crates/vega_ui/src/text_input/nsh.rs` (read-only) | `6c330e1ab0d8695a1ed24b032954353fd955f858fc1e5ff0e16936907428205c` |
| `crates/vega_ui/src/text_input/editing_core.rs` (read-only) | `88f8bdf4d21dd84f856785f346166146bbe91834fc90d2911c28ef2f454b4146` |
| `crates/vega_ui/src/text_input/native_adapter.rs` (read-only) | `d724f6bb29b56cabe7390d359a293f690aa5a3efaa0cfa3ed0a4c585f9f81ab1` |

No new rename test/source file was created; no fake algorithm/source assertion or production API expansion was introduced. No Cargo/rustc/build/test/native UI/network/CI run occurred. Static evidence is not a runtime result. Separator/mounted/workspace-doc owners, conversation, shared icons/tooltips/codeheader exclusions and unrelated business behavior were not reopened. This attempt does not claim implementation closure or fullnative acceptance. This lane stops writing on handoff pending the minimal shared policy.

## Historical inventory status, authority and source boundary

This is an **author-side implementation inventory**, not the consolidated non-implementer independent review. It does not approve fullnative acceptance, merge, release, or any native interaction. Its purpose is to identify currently reachable non-NSH product pixels and the smallest implementation follow-up, without changing Rust.

- Read-only Git inspection confirmed branch `feat/907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`; the worktree is dirty, including concurrent implementation work. HEAD alone does not identify the inspected source. Key dirty-file SHA-256 identities are recorded below.
- Source observation: 2026-11-09 UTC. No fetch, network, checkout, index operation, commit, or other Git mutation was performed. The current task explicitly forbids those operations; no remote freshness is claimed.
- Read root `AGENTS.md`, `docs/vega-exec-guide.md`, `docs/vega-design-guidelines.md`, and the applicable scope/architecture/acceptance sections of `docs/907/nsh-full.md`. No nested `AGENTS.md` was found under `docs`, `crates/vega`, or `crates/vega_ui`.
- Applicable acceptance: [nsh-full.md:7–9](nsh-full.md) requires real NSH product UI, not old GPUI visuals concealed behind adapters. Fast may retain window/event/layout/controller duties. Existing data, permission, persistence, keyboard, and recovery contracts remain intact.
- This lane writes **only this new document**. No Rust, Cargo, lockfile, vendor, build, runtime, CI, installation, or machine-configuration changes. Parent owns Cargo and conversation implementation; unrelated business defects were not investigated.
- Workspace separator/controller work is already present and belongs to another active lane. The user's “current six green” report is contextual evidence, not a check run by this lane. No `separator.rs`, `mounted*`, or workspace-document implementation/validation work was undertaken here. Current callsites are acknowledged below, not reopened as missing work.

**Static outcome:** one confirmed reachable GPUI visual implementation gap was found: the sidebar task-title rename editor. The sampled shell chrome, sidebar display leaves, terminal, and file/image preview routes use NSH. This is a bounded callchain inventory, not proof that every production surface or native state has passed acceptance.

## Current root and leaf callchains

All source references below are repository-relative, with line numbers from the inspected dirty bytes, not necessarily from HEAD.

| Production route | Current entry and visible leaf evidence | Classification |
| --- | --- | --- |
| Startup → workspace root | `crates/vega/src/main.rs:125–165` initializes workspace UI and replaces the root with `VegaWindow::new`; `crates/vega/src/window/render.rs:228–229` implements the production root Render. Startup itself dispatches to `render_presentation` at `crates/vega/src/startup_controller.rs:185–188`; `startup_controller/presentation.rs:40` mounts `view::render` through `Surface::new`. | Real product route, not the NSH preview window. |
| Main title, navigation error, trailing shell slots | `window/render.rs:734–747` constructs the main region; `:932–959` dispatches title chrome to `Kind::Header`; `:892–914` dispatches the slots to `Kind::Slots`; `:742–743` and `:797–807` dispatch navigation errors. `window/shell_chrome/mod.rs:487` mounts `view::render`; `:549–561` synchronizes/publishes and obtains the Surface entity. `window/shell_chrome/view.rs:3,33` uses `nsh_core::prelude` and returns an NSH element; its borders/background/buttons are NSH, not GPUI style calls. | NSH visuals; outer GPUI divs position/occlude/route input. |
| Blocked permission-reset notice | `window/render.rs:740` → `window/permission_reset/projection.rs:164–172` → `render_shell_chrome(Kind::PermissionReset)` → the same NSH chrome renderer. | Not a remaining GPUI notice. This does not assess reset business correctness. |
| Root/sidebar background and workspace separators | `window/render.rs:771–776,787–792` use `nsh_workspace::background`; `:843–850` dispatches sidebar background/separator. `window/workspace.rs:1224,1256` dispatches panel drag regions to `nsh_workspace::drag_region`. | Current NSH callsites, owned by the separator/workspace lane. No duplicate separator gap or local validation claim. |
| Sidebar normal display | Root `window/render.rs:778–780` mounts Sidebar. `crates/vega_ui/src/sidebar/mod.rs:663–676` renders the sidebar with an NSH background; `:740` mounts the sessions controller. `sidebar/nsh_leaf.rs:251` mounts `Surface::new(..., render, ...)`, `:698–707` hosts its entity; `:849` and `:884,955,1046` construct NSH labels/icons/actions. `sidebar/threads_block/organization/render.rs:331–340` likewise places NSH control content in a geometry-only div. | NSH display, **except the rename branch identified below**. Inherited GPUI text styles on structural divs are not evidence of GPUI glyph paint. |
| Navigation controls, sidebar shown or hidden | `sidebar/mod.rs:594` and `window/render.rs:949` → `crates/vega_ui/src/navigation.rs:48–49` → `navigation/nsh_controls.rs:184` Surface mount. | NSH navigation controls, not legacy buttons. |
| Workspace environment/header/menu/error | `window/workspace.rs:1008–1027` → `Kind::Environment`; `:1041–1050` → `Kind::Header`; `:1292–1301` → `Kind::Error`; `:1304–1339` → `Kind::Menu`. `window/nsh_workspace/mod.rs:892–909` mounts `view::render`; `window/nsh_workspace/view.rs:264` is the NSH page renderer. | NSH leaves inside Fast layout/input hosts. The deferred menu wrapper is not itself an old menu control. |
| Terminal tab | `window/workspace.rs:1067–1071` mounts the selected terminal entity. `crates/vega_ui/src/terminal.rs:205–221` mounts `surface::render`, and `:232–256` returns only the Surface inside its focus/layout host. `terminal/surface.rs:3` imports NSH; `:177–178` creates an NSH `Painter`; `:550–552` supplies that draw result to NSH canvas; `:596` uses NSH input. | Actual NSH terminal cell/cursor/selection drawing, not a GPUI terminal hidden by a Surface adapter. PTY behavior, IME, resize and input delivery have not been executed here. |
| File preview tab | `window/workspace.rs:1062–1066` → FilePreview. `crates/vega_ui/src/file_preview.rs:62–63` mounts `render_page`; `:103–121` returns the Surface entity. `:125–130` explicitly returns NSH `AnyElement` and imports NSH APIs; `:137–162` builds markdown, `:171–191` builds the NSH virtual text list, `:193–237` builds the NSH header/body/actions. | Real NSH file content, line numbers, header and reveal action. GPUI types at the controller boundary do not imply GPUI text paint. |
| Image preview tab/thumbnail | `window/workspace.rs:1057–1061` → ImagePreview. `crates/vega_ui/src/image_preview.rs:287–290` calls `render_nsh(false, ...)`; `image_preview/nsh_preview.rs:135–145` mounts `render`; `:244` mounts NSH recovery; `:654,745` define the recovery/page renderers. | NSH preview and recovery, with native save/open services kept outside the visual tree. |
| Artifact preview rows | `window/workspace.rs:1086–1108` uses a Fast `uniform_list` for row placement; `crates/vega_ui/src/artifact_card.rs:295–303` delegates each row to `host::render(..., Mode::Row(row), ...)`; `artifact_card/host.rs:601,707` mounts NSH row/menu surfaces. | Host virtualization remains; row and menu pixels are NSH. `uniform_list` is not sufficient evidence of an old visible widget. |
| Diff / commit / subagent panel boundaries | Diff tab dispatch at `window/workspace.rs:1072–1084` reaches `crates/vega_ui/src/diff_view/nsh_view.rs:192,246,296–297`; Commit Render is at `commit_panel/panel.rs:311`, with `commit_panel/host.rs:161,553` mounting NSH panel/recovery. `window/subagent_panel.rs:1114–1145` places NSH workspace chrome beside the detail stream. | Surface boundaries checked only. Conversation/detail/editor behavior and these panels' broader implementation are not independently reviewed here. |

The shared Surface is not an arbitrary old-widget adapter: `crates/vega_ui/src/nsh.rs:81` types its renderer as `fn(Signal<S>, Sender<C>) -> nsh_core::AnyElement`; `:98–110` invokes that renderer; `:114–115,164` owns/creates an NSH platform `AppView`. The table also inspects the actual renderer imports/children rather than relying on the name “Surface”.

## Historical confirmed implementation gap: sidebar task-title rename

### Entry route and visible GPUI work

This is reachable on the normal production sidebar, including the current organization/pinned route; it is not a benchmark, fixture, demo, or unmounted legacy title renderer.

```text
VegaWindow::render
  window/render.rs:778–780 → Sidebar
  sidebar/mod.rs:740 → ThreadsBlock
  sidebar/threads_block.rs:1936–1937 → render_organization
  sidebar/threads_block/organization/render.rs:297 → render_pinned_pi
  same file:410–418 → render_pinned_row
  sidebar/threads_block.rs:1364–1384 → render_row_at_height

User activates Rename in the task actions:
  sidebar/threads_block.rs:1128–1130 → start_rename
  same file:506–515 → TextInput::new + RenameSession + focus
  same file:1407–1410 → editing_session for this row_key
  same file:1477–1478 → render_rename_editor
  same file:1898–1915 → .child(session.input.clone())

TextInput's ordinary GPUI Render:
  text_input/element.rs:624–625
  :695–703 → GPUI background, border and rounded single-line chrome
  :709–713 → TextElement
  :561–580 → window.paint_quad(selection), ShapedLine::paint, paint_quad(cursor)
```

The last node is the decisive evidence: it draws GPUI glyphs and quads, not merely text measurement, focus/controller state or event forwarding. The ordinary non-editing task row instead uses `nsh_leaf::Item`/host (`sidebar/threads_block.rs:1488–1498`); that does not migrate its editing branch.

`text_input/state.rs:10,41` initializes `edit_menu: false` and `bare_chrome: false`. Consequently the default rename input has visible legacy chrome, but this inventory does **not** count the conditional gpui_kit context menu at `text_input/element.rs:729–761` as a demonstrated open production menu on this route. The actual glyph/selection/caret and chrome evidence is sufficient without inventing that additional gap.

### Smallest repair direction, not implemented here

Replace the sidebar rename **visual child**, retaining `RenameSession.input` as the trusted text/draft controller and retaining the current row identity, persistence, confirm/cancel and focus policies. The current reusable NSH entry is `TextInput::render_nsh_input(input, window, cx)` at `crates/vega_ui/src/text_input/nsh.rs:692–706`, which delegates to the native editor path rather than ordinary `TextInput::render`.

Integration will require carrying `window` to the render/mount boundary (or a retained per-input host) instead of mechanically placing the old Entity under a new adapter. It must preserve single-line rename geometry/typography, initial title and caret, double-click/menu entry, Enter confirmation, Escape/outside cancel and return focus. Confirm must respect the NSH editor's latest accepted draft/synchronization state; do not persist a stale controller snapshot while an edit is in flight. Check Chinese IME, paste/selection, long titles, scrolling/repeated rows, cancellation and reopening. Existing native draft/recovery mechanisms should be reused, not a new rename business implementation.

This is a proposed leaf-level implementation follow-up for the parent to allocate, **not** a claim that the public helper is a verified drop-in replacement or that this document fixed the gap. Do not globally replace every TextInput, alter rename validation/storage, or reopen unrelated group-editor behavior as part of this finding.

## Non-gaps and exclusions that prevent false positives

| Candidate | Current evidence and decision |
| --- | --- |
| Root `gpui_kit::base::TextSelectionLayer` | Mounted at `window/render.rs:777`. Inspected the locally cached **gpui-base 0.7.3** implementation, not an older Kit version: `src/text_selection.rs:2748–2769` sets up the selection frame and calls `paint_text_selection`; `:2805–3061` schedules selection bookkeeping and installs mouse/touch/scroll handlers. That function does not itself issue quad/glyph paint. It is not a demonstrated old visual overlay. This does not declare all conversation selection participants migrated; parent owns that inventory. |
| Sidebar old `threads_block/title.rs` | `:8–37` really contains GPUI `ShapedLine::paint` (`:26`), so it is not measurement-only. However focused Rust reference search found no current `mod title`, `title::...`, or `fit_title` reference outside that file. It is not on the mounted production sidebar route and is excluded, not mechanically migrated or described as harmless measurement. |
| Sidebar reorder canvas | `sidebar/threads_block/organization/reorder.rs:366–376` records full/visible row bounds; its paint callback is empty. Geometry for reordering, not GPUI visible pixels. The drag preview display route uses the existing NSH leaf host. |
| Image visibility canvas | `image_preview.rs:123–139` intersects bounds with the content mask and updates visibility; `:133` is an empty paint callback. Tracking only, not image paint. |
| Structural GPUI styles/events | Root/layout divs, text-style inheritance, masks, scroll handles, focus and AX event hosts do not establish visible GPUI glyphs. For example `sidebar/threads_block/organization/render.rs:400–403,451–455,513–515` places `section_label` under inherited styles; current labels/actions are NSH. `workspace.rs:1311–1339` positions and defers the NSH menu, without a legacy `PopupMenu` body. |
| Native image services | `window/image_preview.rs:252` requests a native save path; `:335,401` queries/launches external applications. Native OS/service UI is not an embedded legacy GPUI preview. No save/open service behavior was exercised or re-reviewed. |
| Shared icon and three tooltip paths | Reuse [nsh-production-icon-residue.md](nsh-production-icon-residue.md), rather than reopening its implementation. Current `icons.rs:250` delegates tooltip content to `tooltip::element`; current `icons.rs` and `icons/tooltip.rs` hashes match that record's latest listed identities. The sidebar's still-existing `.tooltip(...)` trigger at `sidebar/threads_block.rs:1437–1439` therefore does not establish old GPUI tooltip-body paint. No new icon/tooltip finding or native tooltip acceptance is asserted. |
| Old codeheader/legacy message body | Reuse [nsh-production-code-residue.md](nsh-production-code-residue.md): benchmark/permission-only routing and required measurement are already documented. Do not reclassify those paths as live shell gaps, delete measurement, or reopen the conversation investigation here. |
| Preview/dev/benchmark/test files | `crates/vega/src/nsh_preview*`, hidden benchmark routes and dev/mounted fixtures are not normal production-entry evidence. No residual is raised merely because such a file contains a GPUI control. Existing mounted tests are neither run nor treated as new proof by this lane. |

## Source identities for this observation

SHA-256, obtained directly from current bytes. These are content identities, not commit IDs or build results. The workspace is not frozen; later edits require rechecking relevant identities and line numbers before implementation or consolidated review.

| Repository-relative path | SHA-256 |
| --- | --- |
| `crates/vega/src/main.rs` | `8ec4dd06778516928cbb5ccb4b69e05711d834e7c9f90e6c08f6cd24ee83a620` |
| `crates/vega/src/window/render.rs` | `7197aa5dfedc0cd00543f7a4c464aa1ceb4a3deaaaa3aa83257227ef06393367` |
| `crates/vega/src/window/workspace.rs` | `264cc442038ae3f0089811bd83cac3f76a2b5a0befe355083c9da4780dec3457` |
| `crates/vega/src/window/nsh_workspace/mod.rs` | `c06a3ed1a68adcce8b808a878331ec172a534af920e54d741c31dc88145d6f1d` |
| `crates/vega/src/window/shell_chrome/view.rs` | `04485a7da0d1a5af2a50e0b74865f1e36b17b1b6e05ec2185b474f92f54e1a8f` |
| `crates/vega_ui/src/sidebar/mod.rs` | `1fbccc77f44fbabba1dc918415a9d81f4d097d4d001163654ef5f5f0825532ae` |
| `crates/vega_ui/src/sidebar/threads_block.rs` | `3d73bf3986074a74171e1e4a75296b0c377c5cbb73368b0309b2a6ec1fc19569` |
| `crates/vega_ui/src/sidebar/nsh_leaf.rs` | `af7abd01113022e32ec1a1cb89b21e18ab21cc2e668fa5bf498b9bed8fe8e47d` |
| `crates/vega_ui/src/sidebar/threads_block/organization/render.rs` | `566f68c795cf51ad03c82229153a0f64898a5eb368f757b7d6d45a4ae2d4dc9e` |
| `crates/vega_ui/src/sidebar/threads_block/title.rs` | `984bdaabd5aa289f6106bdef8cb8694c1f62d5730b53a9d43e8664cae012381a` |
| `crates/vega_ui/src/text_input/element.rs` | `59467997101b00d8cf49ca405b0295101983dc9ec2f1ae2430eb2135c2057a24` |
| `crates/vega_ui/src/text_input/nsh.rs` | `6c330e1ab0d8695a1ed24b032954353fd955f858fc1e5ff0e16936907428205c` |
| `crates/vega_ui/src/text_input/state.rs` | `a228a83b55bb6e014f72a984cae2ea56ff964eb9d81e29b8b744c6b3c14c2226` |
| `crates/vega_ui/src/terminal.rs` | `3cd11c1f41b9f9250af56e309f22607a780dc30f1de5a71ca33cbf330d36ffc6` |
| `crates/vega_ui/src/terminal/surface.rs` | `d5bc622d8b9d60ffd6a188e756ebaf82b302322cb576431384f7fa461a36db13` |
| `crates/vega_ui/src/file_preview.rs` | `c9b7abc71d05129acb50440216503ed8c52a84754a6d771a9c7338f2b14c200b` |
| `crates/vega_ui/src/image_preview.rs` | `0913507d2b5400355ae0abe58c5e4ac19fed1d89a6ebe88427597b6a3002e91b` |
| `crates/vega_ui/src/image_preview/nsh_preview.rs` | `2f3790d67292a17f2cf71717eccc780995c365164f60512f166924626a1a5661` |
| `crates/vega_ui/src/nsh.rs` | `e35474eab2519497b89216193213967c785faf44b8d653f99d8835b61441270c` |
| `crates/vega_ui/src/icons.rs` | `2a87d60acb500e0d8005bd6fe2975064a0a243250ca1f3437b460aa4247f236c` |
| `crates/vega_ui/src/icons/tooltip.rs` | `8742b44cd31f1435500be0ef0291712c031b2a76e2cb5db2cb39f46e98f96d93` |
| `docs/907/nsh-production-code-residue.md` | `46f71238daf8cdae7dd6b870e02b08ea9dc5eb1011026d4db5797b75eaf25c68` |
| `docs/907/nsh-production-icon-residue.md` | `49ea5bd5a368cb5b10af6867b9111b59267b51474621620d24444cf4869d780d` |

The supporting cached `gpui-base-0.7.3/src/text_selection.rs` had SHA-256 `7d3c7619b32efd627f8e3de34e2283e3152352d738269622e9eab4d2927b4891`. This is local dependency-source evidence, not a new vendored patch. Current lockfile entries identify Kit/base `0.7.3`, Fast `0.1.9`, NSH core/platform `0.4.0`; no dependency resolution or Cargo invocation was performed.

## Validation, uncertainty and handoff

Executed only read-only source/reference searches, focused numbered reads, read-only Git identity/status inspection, SHA-256 inspection, and document static checks. No build, rustc/Cargo query, test, application launch, network or CI query was run. There is no new behavioral test result, pixel capture, compositor evidence, AX/keyboard/IME result, Windows/Linux evidence, or independent-review result to report.

At this historical stopped version, the one confirmed rename gap was sufficient to prevent the inventory from claiming fullnative closure. The later authored repair above supersedes that implementation status, not the need for current validation. The absence of another proven live GPUI painter in the bounded routes above is not whole-product completeness. Surface source proves who constructs the visible tree; it does not prove runtime parity, native events, all failure states, clipping, resizing, scrolling or lifecycle behavior. Historical green counts and another lane's current six-green report do not substitute for final-current-source validation.

Parent next action: allocate the rename leaf repair, preserve the trusted controller and existing acceptance, then validate the exact resulting source and include it in the later consolidated non-implementer review and required gates. Conversation paths and all Cargo remain parent-owned. Separator/controller/mounted work remains with its current owner. This inventory requests no changes to those files or their documentation.

**Handoff boundary:** this lane stops writing after delivery of this document. Subsequent source changes invalidate corresponding line/hash claims until re-observed; no fullnative acceptance is claimed.
