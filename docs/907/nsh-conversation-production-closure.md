# Conversation production closure — remaining author implementation inventory

## Current superseding author implementation — C1 and C2

The sections below this addition preserve the **historical pre-repair inventory**, exact old file:line evidence and old hashes. Their documentation-only write restriction and statement that all three gaps remained describe that completed earlier stage, not the current authorization or source state.

The user subsequently authorized implementation of C1/timestamps and C2/ordinary-send controls in the same dirty worktree, still **not independent review**. Both visible leaves now use retained transparent NSH Surfaces. C3/message anchors remains exclusively parent-owned; this lane did not modify or reassess it. Full conversation/native acceptance and #907 completion are not claimed.

- C1: `crates/vega_ui/src/conversation_stream/timestamps.rs:319` now returns local RenderOnce `presentation::Separator`; `timestamps/presentation.rs:105` retains the Surface, `:187` handles passive width/height observation, `:237` creates real NSH day/suffix text with original weight, center/padding, accessible content and natural-height reporting. Collection, formatting and initial list-height estimate are preserved.
- C2: `crates/vega_ui/src/conversation_stream/ordinary_submissions.rs:611` preserves the original matching/suppression/content/diagnostics decisions; `:652` returns a local RenderOnce row without changing the call signature. `ordinary_submissions/presentation.rs:159` projects original strings, `:362` checks immutable snapshot/owner/live mount/geometry/Surface health, `:387` retains original event and displayed-attempt diagnostics authority. The actual visible NSH tree and measured presentation receipts are in `ordinary_submissions/presentation/pixels.rs:10`. Fast remains only transparent host, original focus/keyboard/AX wrappers and geometry/paint forwarding.
- Existing `render.rs` calls need no changes. No shared export, Cargo/dependency or registration write is requested. Two timestamp and four ordinary production projection/NSH behavior development tests are locally wired; **none was executed in this lane**.
- Executed final scoped rustfmt checks and Python preservation/UTF-8/whitespace/source-boundary checks passed. No Cargo, rustc, build, application, network, Git mutation, vendor or shared-source write occurred. Focused vendor definitions were read to use the actual current API. Parent owns compilation and runtime/integrated checks; pre-delta GREEN is not post-delta evidence.

Full acceptance, exact seven-file hashes, verification limits, remaining mounted keyboard/clipboard/stale-action checks and development-test cleanup requirements are recorded in [timestamp/ordinary implementation handoff](nsh-timestamps-ordinary-controls.md). Seven-file source identity: `ed79f0098cd59c787ac47cd861ec2d2e497fa25b8d4ec8d838b938f770b9445d`; unchanged dirty HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`. Timestamp leaf SHA-256 is now `b4dd35a9b5a8df479a9c4df4cd8b42ec607a3b886d3722f71542b29fe748007d`; ordinary leaf is now `292eba6cb83f87ca310a100cc29bb5f6f08175cfc5d387c31c46a07ff38703ed`. This stage stops writes at handoff; no review gate is started.

## Historical inventory — authority, identity and result

This is **#907 author-side implementation inventory, not independent review**. It does not start the consolidated independent-review gate. The current instruction is to finish all implementation before that gate, and this lane may write **only this new document**. No Rust, Cargo, vendor, shared-document, build, network or Git mutation is authorized here.

Read-only Git inspection confirms worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, branch `feat/907-nsh-full`, HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, with substantial pre-existing dirty implementation. HEAD alone therefore does not identify the inspected source. The initial shell identity command ran in the tool's default Loom directory; that unrelated HEAD was discarded. All subsequent investigation explicitly selected the assigned Vega worktree. No fetch/update was performed: the current no-network/no-Git-mutation instruction overrides the general pre-change fetch rule.

Read the applicable root AGENTS, [execution guide](../vega-exec-guide.md), [design guidelines](../vega-design-guidelines.md), and [full migration contract](nsh-full.md). No nested AGENTS was found under `docs` or `crates/vega_ui`, nor at the `crates` ancestor. The migration criterion remains NSH-built production-visible UI, with Fast retained for hosting, layout, events, controllers and measurement; a GPUI `div` is not automatically a violation, and a no-draw canvas is not a visible renderer.

**Result: production conversation rendering is not yet fully closed. Three remaining implementation areas are confirmed reachable and visibly GPUI-rendered:**

1. Timestamp separator text.
2. Ordinary-send failure/status, retry/check-status and copy-diagnostics controls.
3. Message-position rail marks and its hover/keyboard preview chrome/text.

The requested tool, thinking, run, composer, permission, question and artifact card entry points reach genuine NSH surfaces in the inspected snapshot. No additional visible GPUI card-body gap was established there. This is a rendering-ownership conclusion, not native behavior, business/security correctness, independent approval or full #907 acceptance. None of the three gaps was repaired by this documentation-only lane.

## Actual production root and exhaustive row dispatch

Paths below are repository-relative. Line numbers identify the inspected dirty bytes; symbols are provided because parent implementation may move them.

- `crates/vega/src/window/render.rs:622` creates the real `ConversationStream` for a thread. `crates/vega/src/window/nsh_workspace/detail_layout.rs:78–91` mounts the stream in the workspace detail body. These are application routes, not a benchmark window.
- `crates/vega_ui/src/conversation_stream/render.rs:538` implements `Render for ConversationStream`. Its nonempty production list calls `render_stream_entry` at `:749`, supplying `MessageInteraction`; collapsed/expanded run and stage special cases occur immediately above that call.
- `crates/vega_ui/src/conversation_stream/render_rows.rs:98` passes `Some(interaction)` to `render_entry_internal`. The match at `:156–269` covers Subagents, RunActivity, RunActivitySegment, ContextCompaction, User, UserImages, Screenshot, Assistant, Tool, ToolGroup, ArtifactGroup, Permission, Plan and Summary.
- Production row additions must also be included: footer `render.rs:779–783`, ordinary controls `:788–790`, run/MCP notices `:791–816`, timestamp prefix `:821–823`. The rail is constructed at `:670–674` and mounted at `:964–968`; checking only the StreamEntry match would miss the confirmed gaps.
- `phase.rs:669–711` renders expanded run details. Operations call `RunActivityGroup::render_child`; assistant details call `render_entry_with_selection` at `:700`, retaining the NSH reading route.

In the rest of this document, unqualified conversation files are under `crates/vega_ui/src/conversation_stream/`; other paths are explicit.

## Confirmed remaining work and smallest repairs

### C1 — visible timestamp separators

**Exact route:** production row `render.rs:821–823` → `timestamps.rs:319`, `ConversationStream::timestamp_prefix` → GPUI `div`, `.text_size` and `.text_color` at `:329–339` → direct string children `display.day` and `suffix` at `:340–345`.

This is visible glyph rendering, not time calculation or measurement. `mod.rs:350` enables the module without a development/test feature guard. `timestamp_prefix` returns a row when the actual entry key has a valid timestamp prefix (`timestamps.rs:320–322`). Timestamp adjacency at `:35–48` creates separators for real timed-message gaps; absence in a short/current conversation does not make the path dormant. Footer timestamps already owned by NSH do not replace this distinct separator.

**Smallest repair:** retain timestamp collection, formatting, identity, placement and height accounting. Replace only the separator's visible day/time content with a retained NSH leaf, preserving its centered layout, day weight, exact localized accessible label, color, padding and text. The existing `conversation_status` measurement/Surface pattern can be reused or extended by its owner; no new timestamp model or business-policy changes are necessary. Keep the separator noninteractive. Parent should verify old/history gaps and locale changes, not just a no-separator example.

### C2 — ordinary-send failure/status and actions

**Exact route:** production row `render.rs:788–790` → `ordinary_submissions.rs:605`, `ConversationStream::ordinary_controls` → GPUI text container `:651–662`; retry/check-status label `:709`; diagnostics label `:753`. The action controls also draw GPUI focus borders at `:683–685` and `:727`.

This is a live exceptional-state product path. Entry matching and final-row identity checks are at `:610–626`; controls are projected at `:628–631`. Diagnostics can render independently of the status/action when an attempt did not start and has failure/abnormal/sealed evidence (`:632–640`). Unknown/failed states are produced by the existing controls decision at `:48–100`. A green normal-send snapshot or a suppression condition does not close these branches. `mod.rs:12` includes the module unconditionally.

**Smallest repair:** move the visible message/detail and existing named action/diagnostic buttons into one retained NSH row projection, with natural height. Keep the existing submission/thread/attempt association, suppression predicates, state-derived actions, focus order, keyboard activation and diagnostic clipboard operation in the existing controller. Use the same action labels and existing event payloads; add no explanatory UI copy. Cover message-only, action-only/as applicable, diagnostics-only, combined, stale/changed attempt, keyboard focus and disappearance. This is a renderer replacement, not a re-audit or redesign of retry, preparation or execution security.

### C3 — message-position rail and preview

**Exact route:** nonempty stream `render.rs:670–674` → `message_anchors.rs:341`, `render_message_anchor_rail` → mounted overlays `render.rs:964–968`.

- Actual rail marks are GPUI child rectangles with selected/default widths and backgrounds at `message_anchors.rs:612–624`. This is visible paint even without a `paint_quad` call.
- Hover/keyboard selection resolves a preview at `:396–413` and calls `render_message_anchor_preview` at `:445–451`.
- That helper starts at `:638` and constructs GPUI rounded border/background/shadow at `:654–658`, text styling at `:659–660`, and the direct clamped label child at `:661`.
- The production prerequisites are sufficient workspace width (`:335–338`), `geometry.should_show()` (`:368`) and an eligible selected anchor for the preview (`:433–445`). It is not demo-only or permanently unreachable. `mod.rs:333` enables this module unconditionally.

**Smallest repair:** migrate the marks and preview decoration/text to retained passive NSH leaves, preserving exact mark geometry/selection colors, preview width/height and three-line limit. Keep the existing geometry projection, location request, hover/keyboard selection, focus, wheel routing and overlay positioning in Fast. If preview events remain outside the Surface, ensure the passive NSH child does not consume them. This does not require changing message navigation or source-anchor algorithms. Parent should check shown/hidden rail, selected/unselected marks, hover and keyboard preview, width changes and clipping.

**Do not misclassify the rail observer:** `message_anchors.rs:350–365` captures list geometry and has an empty paint callback at `:362`; that observer is allowed. The marks and preview described above are the actual residue.

## Requested interaction-card and foreground coverage

These rows record actual rendering endpoints, not merely names containing `native` or `nsh`. `Surface::new`/`new_transparent` is paired with NSH element construction in the cited leaf. Card paths without a crate prefix are relative to `crates/vega_ui/src`, while run/thinking/composer and other conversation paths follow the conversation directory convention above.

| Reachable surface | Focused route and actual visible owner | Inventory result |
|---|---|---|
| User/assistant reading, code and errors | `render_rows.rs:192–215` selects reading; `:465–474` calls `selection_menu::body::render_model`; `selection_menu/body.rs:1529`, `:1577`, `:1712` reach the real NSH body Surface. `render_rows.rs:1169–1170` delegates ordinary error text to `conversation_status::notice`. | NSH visible body; no new legacy-body finding. |
| Tool card, including expanded detail | `render_rows.rs:226–230` → `crates/vega_ui/src/tool_card/nsh_tool.rs:257–284`; `render_with_title` mounts its Surface at `:330`; NSH `render` begins `:479`, detail renderer `:735`. | Header/detail are NSH; outer indentation and mounted layout remain Fast. |
| Tool group and scrolling | `render_rows.rs:232–235` → `crates/vega_ui/src/tool_activity_group.rs:236–285`; native group header at `:252`, child tools at `:281`. `tool_activity_group/nsh_header.rs:185`, `:343` build its NSH Surface/view. `disclosure.rs:196`, `:561–576` build NSH scroll chrome. | Fast grouping/scroll handling is not a visible legacy card. |
| Run/stage headers, divider and details | `run_activity.rs:1463–1505` → `newrunactivity/mod.rs:763`, Surface `:870`, divider Surface `:888`; `render_child` match `run_activity.rs:1562–1580` covers Thinking, Tool, ToolGroup and Compaction. Stage header calls native renderer at `:1919`. Expanded assistant details use `phase.rs:700`. | NSH headers/dividers and child views; sticky composition stays Fast. |
| Thinking header/body/omission | `thinking.rs:524–548` → `newthinking/presentation.rs:251–260`, header/omission Surfaces; `thinking_body.rs:1`, `:106–115` call NSH reading `render_fading`. Stage source uses `thinking.rs:283–308`. | No separate legacy thinking-text renderer reached. |
| Permission card, including subagent permission | `render_rows.rs:239` → `crates/vega_ui/src/permission_card/nsh_permission.rs:31–45`, NSH `render` at `:135`. `subagents.rs:69–80` constructs only the Permission variant for its `render_entry` call and mounts active questions separately. | NSH permission card; not the legacy User/Assistant branch. |
| User question, choices, note and actions | Composer mounts `active_user_question` at `render.rs:294`; subagents mount it at `subagents.rs:79`. `crates/vega_ui/src/user_question_panel.rs:206–207` → `user_question_panel/nsh_question.rs:119–153`; Surface at `:126`, NSH multiline input at `:413`. | NSH visible panel and editor. The `TextInput` allocated at `user_question_panel.rs:65` is controller draft storage: snapshot reads it at `nsh_question.rs:64`, native Note command updates it at `:103–107`; it is not mounted as a GPUI widget. |
| Artifact compact card, rows, group and menu | `render_rows.rs:238` → `crates/vega_ui/src/artifact_card.rs:497–499`; compact Render at `:79–81`, rows at `:295–302`. `artifact_card/host.rs:577–601` creates the NSH Surface; menu uses `:707`; `artifact_card/nsh_view.rs:758`, `:801–843` build the actual menu/mode views, including recovery mode. | NSH visible artifact surfaces; no hidden GPUI fallback established. |
| Plan and summary outcomes | `render_rows.rs:240–269` → `crates/vega_ui/src/plan_card.rs:204–220`, NSH `render_plan` at `:258`; failed/stopped summaries use `plain_error_notice`, completed summary is deliberately zero-height. | NSH visible plan/error content; no separate SummaryCard GPUI mount in this dispatch. |
| Composer editor, shell, actions and selectors | `render.rs:347–413` composes shell/actions/input/buttons. `nsh_composer.rs:5–14` → `crates/vega_ui/src/text_input/nsh.rs:700`, editor Surface at `:882`. `composer_actions.rs:993–1040` routes shell and controls to native views; control Surface at `composer_actions/native.rs:723`. File/model wrappers `render.rs:481–501` call NSH selector methods; selector Surface at `composer_selectors.rs:691`. | Editor, shell, buttons and selector pixels are NSH. The transparent border `render.rs:334–335` and inherited text color `:857` alone do not establish visible GPUI residue. C2 is a separate row addition, not these composer controls. |
| Context, thinking slider, utility and pending messages | `render.rs:485–496` mounts the native context indicator; `foreground_chrome/context_usage.rs` constructs its NSH view/canvas. `thinking_slider/native.rs` owns the slider view. Utility uses `utility_bar/host.rs` + `utility_bar/view.rs`; pending rows compose `native::Chrome` leaves (`pending_messages.rs:536`, `:747`), with pixels in `pending_messages/native/pixels.rs`. | NSH visible controls; Fast hosts/events are retained. |
| Attachments and screenshot | `render_rows.rs:198–199` → `attachments.rs:277–283` presentation groups, or `screenshots.rs:28–35` → `crates/vega_ui/src/image_preview.rs:271–277` ThumbnailHost. `image_preview/nsh_preview.rs:135–145`, `:745` builds the NSH preview. | NSH thumbnails and attachment presentation, not legacy GPUI image content. |
| Error/loading/location, notices, automation and permission feedback | `conversation_status.rs:344`, `:478` owns status text/compaction; location delegates at `render.rs:529`. Run/MCP notice Surface `run_notice/presentation.rs:242`; computer-use `computer_use/presentation.rs:307`; reasoning `reasoning_feedback/leaf.rs:353`; thread permissions `thread_permissions/native_leaf.rs:373`; automation permission `automation_permission/native/leaf.rs:399`; privacy `privacy/control.rs:388`; recovery `shell_recovery/nsh_view.rs:422`, `:697`. | Visible states use real NSH leaves; this is ownership inspection only, not business/security or native recovery acceptance. |
| Foreground/fades, footer/copy/menu | `foreground_chrome.rs:68` builds the passive background Surface. Footer Surface `message_footer.rs:366`; copy receipt `copy_receipt/native.rs:472`; selection popup `selection_menu.rs:525`; resume-tail `resume_tail.rs:321`. | NSH visible chrome/text. Paint forwarding to those child surfaces is not a second legacy renderer. |

## Explicit non-gaps and excluded paths

- Reused [production code residue closure](nsh-production-code-residue.md): current `render_rows.rs` and `inline_code.rs` hashes match that document exactly. A focused `crates` search still finds only benchmark `bench.rs:738` and subagent Permission `subagents.rs:77` as external `render_entry` callers. `crates/vega/src/main.rs:47–54` routes explicit benchmark startup to a separate window and returns before normal startup. The benchmark/legacy `markdown_blocks`/`code_header` chain is not a newly found production conversation body. Do not repair it to manufacture additional migration work.
- `user_disclosure` still uses `inline_code` flow/layout types for measurement; the prior closure explicitly protects that role. No removal or replacement of its measurement was attempted.
- Reused [production icon/tooltip implementation record](nsh-production-icon-residue.md). Shared icons and tooltips are not reopened here. Similar `svg`, `icon` or text helpers inside NSH leaf modules are not GPUI paint merely because they share names with older helpers.
- Checked the actual observer bodies: foreground height `render.rs:133–150`, anchor layout `message_anchors.rs:350–365`, pending queue bounds `pending_messages.rs:724–731`, selection body/menu geometry `selection_menu.rs:713–736`, `:905–921`, resume-tail geometry `resume_tail.rs:404–458`, artifact card/menu geometry `artifact_card/host.rs:816–849`, `:923–952`, and run detail anchor settlement `run_activity.rs:1534–1541`. They measure, record mount geometry or settle scroll state; they do not draw product text/paths/quads. A callback executing in the paint phase is not itself visible paint.
- `foreground_chrome/context_usage.rs:434` is an **NSH** canvas, not a GPUI canvas escape. Passive background/scroll/shimmer surfaces and custom `self.child.paint` forwarding were distinguished from independent GPUI drawing.
- Benchmark, feature-gated development tests, host regressions and harness source were not treated as production reachability proof. There is no feature/test guard excluding C1–C3. Conversely, controller entities and TextInput constructors alone do not prove a visible product widget.

## Source identities and investigation boundary

SHA-256 of the inspected key dirty files:

| Repository-relative file | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/render.rs` | `64174d622992fd53901ffa16787a42b15914067b331b648ec40a900b96163712` |
| `crates/vega_ui/src/conversation_stream/render_rows.rs` | `b9ad8d74709c73646e3900f6237536139700c3036e353e0798a0e4b3d0200409` |
| `crates/vega_ui/src/conversation_stream/inline_code.rs` | `d22481b98434f6462c6fa17e3d0ba5df2c1fdeb5b8154078cc356205a69244f2` |
| `crates/vega_ui/src/conversation_stream/timestamps.rs` | `a78a4d05406b8c06edbe260ad38a852322c2e4004264d75279eb540ed3514753` |
| `crates/vega_ui/src/conversation_stream/ordinary_submissions.rs` | `15f98bc6f0980004d039d269a8e95fa6ae6ccbbb76f12de0840d50ee2e258331` |
| `crates/vega_ui/src/conversation_stream/message_anchors.rs` | `e7fa14157ff9d3f3218c3a801a0945935ec87249e4649bbb63632d123675ab42` |
| `crates/vega_ui/src/tool_card/nsh_tool.rs` | `6e040fa3d224bbc103c3005b5d89f3cdc1cf7b55028d4389941d8be8ab24f400` |
| `crates/vega_ui/src/permission_card/nsh_permission.rs` | `a01008f48fd58c8e691bff9a269f7ac853b912df8cb9b75efb2ea068dc963492` |
| `crates/vega_ui/src/user_question_panel/nsh_question.rs` | `af623fa51247ee76c7538552c138ad1bc4fbe13ee0b66410a842871c9c2ae6af` |
| `crates/vega_ui/src/artifact_card/host.rs` | `e47b3876d3c76c10b4ccbce54632d500ca7ddc4a16cb2abec99754232170a735` |

Additional identity: sorted, deduplicated repository-relative Rust paths comprising all `conversation_stream/**/*.rs`; root files and module trees for `tool_card`, `tool_activity_group`, `permission_card`, `user_question_panel`, `artifact_card`, `interaction`; plus `disclosure.rs`, `scroll_edge_fade.rs`, `plan_card.rs`, `summary_card.rs`, `image_preview.rs`, `text_input/nsh.rs`, all under `crates/vega_ui/src`. For each path, concatenate lowercase SHA-256, two spaces, path, LF in path order; SHA-256 of that concatenation is `9e6258a26e3f89a29d585bfd326a8b2628cf280d8bb2167c4a78ca823b1c9936` over **163 files**. This is an identity set, including development support; it is not a claim that every line of every file underwent independent review. Concurrent author work makes this an observed snapshot, not an atomic repository freeze.

Reference identities: `nsh-full.md` = `189b0b262ff1a6935f57e948207a30c0491ce9ef4006e29e391307a8762353f1`; `nsh-production-code-residue.md` = `46f71238daf8cdae7dd6b870e02b08ea9dc5eb1011026d4db5797b75eaf25c68`; `nsh-production-icon-residue.md` = `49ea5bd5a368cb5b10af6867b9111b59267b51474621620d24444cf4869d780d`.

Investigation used read-only file reads, focused `rg` symbol/style/call-site searches, numbered ranges, SHA-256 and read-only Git identity/status. Paint/text/path searches already performed by the parent were not treated as proof of reachability; this lane concentrated on actual dispatch and visible widget construction, including rare states and overlays. No application, native automation, compilation, Cargo/rustc version query, test, CI query, network, vendor inspection/edit or Git mutation was performed. No security/business behavior re-audit was attempted. Tool-output previews were narrowed for decision-relevant call sites rather than treated as complete file reads.

## Verification and stop-write handoff

Parent-provided current evidence, **not rerun or independently verified by this lane**: new UI snapshot 347; native 36; geometry 24; source 14; scroll 16; icons 3; settings 3; recovery 2 GREEN, and strict workspace/all-target/features GREEN. Those results do not prove there is no GPUI residue: C1–C3 show the remaining ownership gaps. No new runtime pass or failure count is claimed here.

This document is the sole authored artifact. Final checks are limited to UTF-8, terminal newline, no trailing whitespace, local Markdown link targets and the cited key-file hashes. The document's own final hash is handed off outside the file. All compilation and behavioral validation remain with the parent. After the remaining implementation is complete, the parent must obtain the applicable final-version checks, consolidated non-implementer independent review and user/platform acceptance; none is replaced by this inventory.

**Stop writes after this document.** Handoff C1–C3 to the parent for minimal renderer implementation; do not mark the conversation migration, full native acceptance or #907 complete on the strength of this record.
