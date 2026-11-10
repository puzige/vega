# #907 Settings production implementation closure

## Purpose and exact scope

This is an **implementation-closure input for the parent**, not independent review, correctness review, native acceptance, or a declaration that all four implementation lanes are complete. The user requires all four lanes to finish before consolidated independent review.

- Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`
- Branch observed: `feat/907-nsh-full`
- HEAD observed: `0b5ea426abfd8d4358489e916273d89474fb190f`
- Evidence is the **dirty working content**, including untracked migration files, not HEAD alone. SHA-256 identifiers below bind the inspected entrypoints and focused child ranges.
- Read scope: current settings production render callchains and relevant acceptance in `docs/907/nsh-full.md`; the shared scroll-edge fade was followed only because the settings root directly invokes it. No repository-wide UI survey or service/security correctness investigation.
- Write scope: this new document only. No Rust, Cargo, vendor, shared-document, Git, network, build, or test mutations/execution. Cargo remains centralized with the parent.

Applicable instructions read: root `AGENTS.md`, `docs/vega-exec-guide.md` (scope, architecture/security boundaries, evidence and delivery rules), and `docs/vega-design-guidelines.md` (especially §§1, 11, 13). No nested `AGENTS.md` was found under `crates/` or `docs/`. The current explicit read-only/source and single-document-write authorization controls this task; no fetch/update was performed.

Acceptance used: `docs/907/nsh-full.md:7-13` (NSH product UI, Fast host, four-lane ordering), `:39` (all settings fields/routes and credential boundary), and `:109-186` (settings shell, catalog, MCP, Skills, Agents, pricing, capabilities, usage, appearance, permissions, default model, input preferences, updater, reset). Historical progress prose is not evidence of current reachability.

## Result for the parent

**No concrete remaining visible GPUI/Fast painting or old product-control subtree bypassing NSH was found in the inspected current settings production render closure. No additional settings painting repair is proposed.** This is a scoped static implementation finding, not proof of runtime mounting, functional completeness of every control, keyboard correctness, or native pixels.

The natural-height `Label`/LabelElement work and the parent's established mounted 3/3 result are inputs, not reopened work and not tests rerun here. This task neither repairs that code nor reclaims its verification.

## Actual production roots and callchains

All shorthand paths in the next two sections are relative to `crates/vega_ui/src/`; e.g. `settings/render_impl.rs:97` means `crates/vega_ui/src/settings/render_impl.rs:97`.

`settings/mod.rs:184-225` registers the current children, including `nsh_shell` and `nsh_tail`; `settings/mod.rs:215` registers `render_impl`, and `:223` exports `SettingsView`. **The production implementation is `impl Render for SettingsView` at `settings/render_impl.rs:97-98`, not the disconnected `nsh_settings.rs` projection page.**

The root synchronizes presentations at `settings/render_impl.rs:117-133,155`; embeds navigation at `:194-200`, heading at `:228`, pending-navigation guard at `:229-232`, model navigation at `:233-237`; dispatches real sections at `:253-275`. Six sidebar destinations coexist with internal model reasoning/pricing sections: their presence in dispatch is not an obsolete top-level demo.

| Reachable product region | Actual production chain to NSH |
|---|---|
| Sidebar/back and section navigation | `settings/render_impl.rs:199` → `settings/nsh_shell.rs:400-405` → `render_shell` `:119`, `Surface::new` `:138` → NSH `render` `:501`; icon and text children `:488-499`. |
| Page heading/global error | `settings/render_impl.rs:228` → `settings/nsh_shell.rs:407` → same surface factory `:138` → heading branch `:502-534`, including NSH title `:522-524` and actual error `:526-532`. |
| Dirty-navigation guard/model breadcrumbs | `settings/render_impl.rs:229-237` → `settings/model_configuration/nsh_navigation.rs:304-317` → `render_navigation_surface` `:320`, `Surface::new` `:335` → NSH `render` `:493`; outer host `:389-394` only sizes the Surface. |
| General appearance | `settings/render_impl.rs:256-257` → `render_defaults` `:55-94`, `:63` → `settings/nsh_appearance.rs:57-66` → NSH `render` `:252`; Surface host `:135-140`. |
| Default permission picker | `settings/render_impl.rs:72-80` → `settings/permissions/nsh_permission.rs:144-153` → NSH `render` `:368`; host assembly `:221-240`. |
| Authorization-reset confirmation | Permission picker host `settings/permissions/nsh_permission.rs:233-238` adds reset only outside Idle/Completed → `settings/permission_reset.rs:112-119` → NSH `render` `:299`; Surface host `:174-179`. This remains the real conditional confirmation route. |
| Default model | `settings/render_impl.rs:81-89` → `settings/nsh_default_model.rs:83-91` → NSH `render` `:250`; host `:164-169`. |
| Input preferences | `settings/render_impl.rs:91` → `settings/nsh_input_preferences.rs:61-69` → NSH `render` `:258`; host `:145-150`. |
| Updater/release notes | `settings/render_impl.rs:92` → `settings/updater/nsh_update.rs:136-144` → NSH `render` `:313`; host `:220-225`. Expanded notes are `nsh_base::text_view().markdown(...)` at `:558-586`, not a GPUI Markdown child. |
| Provider list/detail/model editor | `settings/render_impl.rs:253-255` → `settings/provider_management.rs:1238-1247` → `settings/provider_management/nsh_providers.rs:946-958` → NSH `render` `:2165`, `view()` `:2187`; provider/model Surface hosts `:1037-1051`. |
| Model basic/capability/catalog composition | Model-editor branch `settings/provider_management.rs:1243-1244` → `render_model_basics` `:1215-1235` → provider Surface `:1220`, image capability `:1221`, catalog `:1222`. The parent `div` only composes/scolls those three NSH children. |
| Image capability and reasoning efforts | Image branch above → `settings/model_capabilities/nsh_capabilities.rs:318-323`; reasoning root `settings/render_impl.rs:259-261` → `settings/reasoning_render.rs:4-9` → `settings/model_capabilities/nsh_capabilities.rs:310-315`; both → `render_capabilities` `:211-221` → NSH `render` `:491`, `view()` `:500`. |
| Model catalog candidates/mapping | `settings/provider_management.rs:1222` → `settings/model_catalog/nsh_catalog.rs:202-211` → NSH `render` `:635`, `view()` `:661`; host `:303-308`. |
| Pricing | `settings/render_impl.rs:262-264` → `settings/nsh_pricing.rs:298-307` → NSH `render` `:662`, `view()` `:722`; host `:419-424`. |
| Usage and charts | `settings/render_impl.rs:265-267` → `settings/usage/nsh_usage.rs:98-107` → NSH `render` `:856`, `view()` `:865`; host `:173-178`. Chart constructors `:411,434,477`; NSH `canvas` callsites `:585,683,780`. |
| MCP | `settings/render_impl.rs:268-270` → `settings/mcp/nsh_mcp.rs:715-727` → NSH `render` `:1228`; host `:861-866`. `settings/mcp.rs:15` registers that implementation. |
| Skills | `settings/render_impl.rs:271-273` → `settings/skills/nsh_skills.rs:512-521` → NSH `render` `:683`; host `:575-580`. `settings/skills.rs:12` registers it. |
| Agents/native preparation/Codex | `settings/render_impl.rs:274-275` → `settings/agents/nsh_agents.rs:228-244` → NSH `render` `:574`; NSH toggles/prepare/recheck/status composition `:603-648`; host `:329-334`. `settings/agents.rs:4` registers it. |

## Suspected bypass candidates and concrete dispositions

These are the candidates encountered by focused searches, **not unresolved defects**. A mechanical search hit is insufficient to call something legacy painting; backend imports and the actual child constructor matter.

| Candidate and root-reachable chain | Disposition / minimal repair |
|---|---|
| Root Fast `div`, text styles, page/content backgrounds: `settings/mod.rs:215` → `settings/render_impl.rs:156-217`. | Allowed layout/event host. Background children at `:185-190,207-212` call `settings/nsh_tail.rs:45-55` → host `:243`, `Surface::new_transparent` `:276-285` → NSH pixels `:406-418`. Root `.text_color/.text_size/.line_height` at `:191-193` do not themselves draw glyphs. **No repair.** |
| Group border/radius: general defaults `settings/render_impl.rs:65` → `settings/helpers.rs:49-61`. | Fast border is explicitly transparent (`:58-59`), retaining box geometry; actual group child `:61` → `settings/nsh_tail.rs:58` → same Surface → NSH background/border/radius `:414-418`. **No repair; do not delete geometry merely because border methods remain.** |
| Field divider/title/description: `settings/render_impl.rs:66-89` → `settings/helpers.rs:64-132,156-174`. | Fast field border is explicitly transparent at `:84-85`; visible rule is `nsh_tail::rule` `:86`, text children are `nsh_tail::label` `:97-117,166-173`. Those terminate in NSH `text` at `settings/nsh_tail.rs:430-444`. Inherited text styles are not an additional GPUI text child. **No repair; established natural-height behavior is not reopened.** |
| Custom Fast `Element::paint` in passive labels/pixels: above shared-label chain → `settings/nsh_tail.rs:178-188,387-401`. | Delegates to child paint; host child is the Surface entity at `:308-315`, not a GPUI text/quad primitive. NSH owns visible pixels `:406-478`. Measurement, width readiness and masks are host responsibilities. **No repair.** |
| Fast navigation focus/AX node: `settings/render_impl.rs:199` → `settings/nsh_shell.rs:263-301`. | Transparent event/focus/AX proxy: `track_focus`, key/mouse capture, and an absolutely positioned semantic Button node at `:284-298`, without background/border/text/icon painting. Actual navigation content is the Surface entity `:263`; NSH button glyphs at `:488-499`. **No painting repair; preserve host keyboard/AX bridge.** |
| Model-editor Fast scroll wrapper: `settings/render_impl.rs:254` → `settings/provider_management.rs:1243-1244` → `:1223-1235`. | Only dimensions, scroll, flex/gap, and the three NSH children. **No repair; do not replace or remove model/service routes.** |
| All other per-page Fast wrappers in the production table, including reset and navigation guard. | Focused reads of each mount found dimension/layout/Surface composition, not old visible controls. Empty `div` early returns are absent-presentation paths, not a second product renderer. **No painting repair.** |
| Usage `Painter` and `canvas`: `settings/render_impl.rs:266` → `settings/usage/nsh_usage.rs:98-107,856` → chart children `:585,683,780`. | `nsh_core::prelude::*` at `:4` supplies the chart canvas/Painter; GPUI imports at `:3` are App/Context/Window. Not GPUI canvas painting. **No repair; retain real charts.** |
| Icon enum/SVG hits: shell chain → `settings/nsh_shell.rs:489-495`; default-model chain → `settings/nsh_default_model.rs:233-240`; permission chain → `settings/permissions/nsh_permission.rs:326-328`. | Icon enum/source reuse does not mount an old GPUI icon. `svg` is an NSH constructor; source strings feed NSH SVG. **No repair.** |
| Shared scroll-edge fade: `settings/render_impl.rs:277-280` → `scroll_edge_fade.rs:470-501` → metrics delegation `:261-268` → `scroll_edge_fade/nsh.rs:260-285`. | Fast forwards existing child paint and clips prepared embedded edges; edges originate in `Surface::new_transparent` at `scroll_edge_fade/nsh.rs:198-200`, with actual NSH gradient in `:291-314`. **No repair in settings or shared fade.** |
| `TextInput` entity creation/read/update in settings state/controller files. | Production render dispatch and every Fast child-construction candidate were checked for visible mounting. No old input entity child was found on these chains. Entity existence is not painting evidence and is allowed controller ownership. **No migration/removal solely because `TextInput` remains.** |

`settings/render_impl.rs:159-168` also retains a Tab event handler. It is **event routing, not visible GPUI painting**. The acceptance restriction at `docs/907/nsh-full.md:113` still applies; this painting-only task does not certify that handler's behavior or open a separate keyboard correctness investigation/repair.

## Preserve existing business/security/service ownership

No source change is requested by this result. Any later painting-only follow-up must retain the existing routes, not replace them with static/demo controls or move authorization into NSH:

- Back/section changes retain `on_back`, provider cancellation, MCP OAuth cancellation, and `request_settings_section`: `settings/nsh_shell.rs:326-344`; draft/permission-close guards are still at `settings/render_impl.rs:11-25`.
- Default model uses `select_model` at `settings/nsh_default_model.rs:126`; the original save path is `settings/render_impl.rs:28-33`. Permission choice uses `select_mode` at `settings/permissions/nsh_permission.rs:183`; reset emits the original `PermissionResetRequested` at `settings/permission_reset.rs:142`.
- Input preferences hand off to the existing field-specific functions at `settings/nsh_input_preferences.rs:104-107`. Pricing hands off at `settings/nsh_pricing.rs:375`; capability ownership remains the typed event at `settings/model_capabilities.rs:141`.
- MCP/Skills service owners remain `McpServerSettingsService` (`settings/mcp.rs:12,163`) and `SkillSettingsService` (`settings/skills.rs:7,16`). Do not infer secret exposure from ordinary controller entities, put credentials into normal projections, or bypass original confirmation/service routes.
- Agents retains original native actions/Codex save handoff (`settings/agents/nsh_agents.rs:10,287,616-623`). Updater note links retain the existing classified-link handoff (`settings/updater/nsh_update.rs:569-585`), with images rendered as alt text (`:565-567`). These are boundaries to preserve, not a fresh security audit certification.

## Focused searches and exclusions

Commands were read-only `rg`, focused `sed`/line-numbered reads, file listing, SHA-256 reads, and Git status/branch/HEAD observation. No old diff was used as the inventory of current routes.

Searches within `crates/vega_ui/src/settings/**/*.rs` included:

- `fn render`, `impl Render`, module registration, `Surface::new`, explicit `gpui_kit::div`, unqualified `div`, `impl Element`, `.child/.children`, and `.into_any_element` to identify real hosts and child construction.
- `bg`, borders/border color, text styles, hover/focus styles, string/text children, Button/Label/Icon, canvas/SVG/images, and direct paint primitives to locate possible product drawing.
- `paint_quad`, `paint_path`, glyph/text/image/SVG paint names, `StyledText`, and custom `.paint` delegates. No direct Fast product primitive was found on the inspected reachable chains; delegates were followed instead of counted as gaps.
- `TextInput`, input-entity child expressions, cross-module render calls, `cfg`, `nsh_settings`, `render_nsh`, and `nsh_presentation` to distinguish controller state, feature validation, and disconnected historical projection code.

Broad paint searches were followed by namespace-aware candidate reads; NSH files were **not excluded from the overall scan**. A separate non-`nsh*.rs` scan was only a supplemental check for Fast residues in adapters/helpers.

Exclusions / reachability decisions:

1. `settings/nsh_settings.rs` is a disconnected historical projection renderer: current `settings/mod.rs:184-225` does not declare `mod nsh_settings`; the settings search finds `render_nsh`/`nsh_presentation` only inside that file. It is not the root, not a fallback, and not a missing production migration requirement. No cleanup proposed.
2. `settings/nsh_tail/mounted_validation.rs` is feature-gated development validation (`settings/nsh_tail.rs:12-14`, exports `settings/mod.rs:203-208`), not an alternate settings product route. The established mounted 3/3 was not rerun.
3. `*/tests.rs`, `provider_management/development_tests.rs`, and `#[cfg(test)]` tails were not treated as production UI. Search results from test tails were excluded from production conclusions. Their existence is not a claim they ran or passed.
4. Other product lanes, demo/preview surfaces outside this settings closure, runtime service internals, dependency/vendor backends, Rust test cleanup, and UX redesign were not investigated. Shared fade was followed only to its settings-used NSH pixel leaf, not audited globally.

## Content identity (SHA-256)

Full-file hashes identify the content versions containing the inspected focused ranges; they do not mean every line was correctness-reviewed. Paths below are worktree-relative.

```text
27d2c38226861c2cf860efcdb9ff70cf15284ffd07a27d7f5296126febb895a0  AGENTS.md
790d59e2a106b8faec06610911f48fb4b8fc91a04079158bf0815f06d02b3fa6  docs/vega-exec-guide.md
a6fe07fe4ce599e495adc901e5bfd932190b53b127b81111c4ae1096af77c483  docs/vega-design-guidelines.md
189b0b262ff1a6935f57e948207a30c0491ce9ef4006e29e391307a8762353f1  docs/907/nsh-full.md
e9e26ff27521e186b6082b6d3b2666372c3e9294347ea149a1128f69cfafee20  crates/vega_ui/src/settings/mod.rs
98f25a61151d6a4bfb477b1f6bfae06591d8cee766ff49b370ce328ac1efba29  crates/vega_ui/src/settings/render_impl.rs
12d83dba9f6205c4a6b49aac1e59afb930e34a8a4e282e5690e99f05dfa067b6  crates/vega_ui/src/settings/helpers.rs
29483d631b4432a119711b4ef95ebc96eb6e11356a2d4c02c2b2dcee1679d5de  crates/vega_ui/src/settings/provider_management.rs
838fcef0a2f8e5a7a98f92600ca70b8c0372a3b843861a24d3b64617c444033f  crates/vega_ui/src/settings/reasoning_render.rs
001c59fa457a543d5cbec1291b3131a0808b5b2176f271b88b8c5036c3cfb15a  crates/vega_ui/src/settings/nsh_shell.rs
41d503ad62c7a00beaa6f1f3d76e8a4fce0171eefcbc1e244298bbb738fd5b41  crates/vega_ui/src/settings/nsh_tail.rs
5edeb9a3c6cf651e03c0300a3427f6c1e08ef18c5b8014c8a3597cf691bd2525  crates/vega_ui/src/settings/nsh_appearance.rs
7c1ff6ff7d6c5472792630e91a9f1120ab2c06659b032551f30d322821334ace  crates/vega_ui/src/settings/nsh_default_model.rs
1a5fd80537cc1ed7098d29f52beefdf7235742a914813d1d4cd0c1bb35474346  crates/vega_ui/src/settings/nsh_input_preferences.rs
520910409984888810821f2eadd6b81b0550a63d18ec40d6628a7d3c7617ac63  crates/vega_ui/src/settings/nsh_pricing.rs
6db4a665c21f9fc665970722a43bdc091707de4a38a16db7e1281a333e9e3323  crates/vega_ui/src/settings/permission_reset.rs
c193e77f87f5fa41f6371778318e745921c885cc77df0ff7d4c81dd76e685bbf  crates/vega_ui/src/settings/permissions/nsh_permission.rs
1a7a10c56387496f207347e1bcbe35358293a47847e0e39b14baf300ebcfbe40  crates/vega_ui/src/settings/model_configuration/nsh_navigation.rs
e9defe9cf9b231357b2e18b7add3e7bd992766a934beaae58f4b087cf78da045  crates/vega_ui/src/settings/model_catalog/nsh_catalog.rs
3f0856cfd433592754be956098944d10ad4ae20d0a62fbc75f7e691a14165292  crates/vega_ui/src/settings/model_capabilities/nsh_capabilities.rs
e8b4acc2036916d4057b68c61023ddba63a03114bda3449a76dbcde968fd696a  crates/vega_ui/src/settings/provider_management/nsh_providers.rs
512d30f50f47e2060bd0729843a3dca915fe4256852d0ff6683334d29ba9e8a5  crates/vega_ui/src/settings/mcp/nsh_mcp.rs
1ab289811737d962ccd05d9e5fd48fa5a2a7bce99513d527cb0a98a246d0af30  crates/vega_ui/src/settings/skills/nsh_skills.rs
148e3c4564ea8c50b7ee9f2dd33d9d97644640ba30818c9820d5c9b16e715522  crates/vega_ui/src/settings/agents/nsh_agents.rs
8b0b2db5c0b6bd717c4172fc2206a8b68d8b235cc60f9a8a4e7f9de89a738fa3  crates/vega_ui/src/settings/usage/nsh_usage.rs
3fe6a86bf9cd2ad86d8dcec02a1df4eb3835985069ca2892e29a794e2a32d1fe  crates/vega_ui/src/settings/updater/nsh_update.rs
38ccc4779795e183753340be4ba70beb01074bde10281a6da9fde94022b7e955  crates/vega_ui/src/settings/nsh_settings.rs
65afc6b852a14b4dd7c8023d53ee163d9ef3b6bd37d44ccb05e3131555fd0c9c  crates/vega_ui/src/scroll_edge_fade.rs
9c8414ec3654a04fcf6b87f4003538dd2b1496af4532cab8cf975a7cd26d07ce  crates/vega_ui/src/scroll_edge_fade/nsh.rs
```

## Validation boundary and handback

Performed: static production-root/candidate tracing, namespace/reachability exclusions, current branch/HEAD observation, and source-content hashing. This document is the only artifact written by this task.

Not performed: Cargo/rustc invocation or version probing, compilation, Rust tests, mounted validations, network/CI queries, Git mutation, application launch, native screenshots/input, cross-platform acceptance, independent review, or business/security correctness certification. Do not convert this result into any of those statuses.

Parent action: no newly identified settings painting implementation item from this closure. Continue the other authorized implementation lanes; only after all four are ready perform consolidated independent review and the parent's centralized validation. If these hashed roots change, refresh the affected static chain rather than treating HEAD alone as the same source version. Stop writes after this document's read-back/static verification.
