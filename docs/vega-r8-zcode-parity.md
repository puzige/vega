# R8 ZCode visual parity — authorized task and handoff

2026-09-05. User explicitly requests pixel-level recreation using the installed ZCode UI. This task supersedes R4 geometry and neutral colors in vega-ui-spec; existing controller, security, persistence, streaming and keyboard contracts remain mandatory.

## Contract

At the measured 950×768 reference window: sidebar 330px, shell light #f2f2f2, inset content light #fafafa with 4px outer inset and 12px radius. Sidebar 8px horizontal padding and 34px task rows; no large brand header. Only existing functional actions appear. Unsupported automation/plugin/search features are not fabricated. Sidebar contains actual project/task controls and settings.

Empty threads center a 28px semibold heading and real composer as one group. Composer uses 16px radius, white elevated surface, 16px side margins, 12px inner inset and a gray project/branch bar. Controls retain Ask/Plan/Execute, permissions, model, Thinking, branch, @file, and token/cost. Active threads keep the same composer at the bottom. No proprietary ZCode logo/assets are copied. Unsupported suggestion actions are omitted.

Settings replaces task sidebar with a compact 64px functional back rail. Main heading 24px, white 12px-radius content card. Real Providers/defaults/Thinking/pricing sections become selectable navigation; existing forms, validation, save/retry and keyboard handlers remain intact. Form drafts survive section changes.

All new colors and font sizes reside in theme tokens. No new dependency, schema, backend semantics, performance runs, installed-app replacement or real credentials/config changes.

## Task / acceptance

1. Implement shell/sidebar/composer/settings geometry from measured reference.
2. Run UI tests, workspace fmt, strict all-target clippy, workspace tests and build; retain raw failures and logs. Existing exact neutral-palette assertions may be updated to this explicitly revised palette; behavioral assertions may not be weakened.
3. Package an owned signed native app with isolated fixture config. Main agent owns CUA review at 960×628 and larger and same-size reference comparison; build/test success alone is not visual acceptance.
4. Deliver commit and raw evidence to main for review/integration. Pixel identity cannot be claimed for intentionally different available features/content.

## Handoff

Implementation delivered for local integration; scoped native acceptance completed by main. Performance deferred by user. No other scope changes authorized.

## Reference refinements

Transparent native titlebar preserves window controls and draggable toolbar. Icons are original thin monochrome paths, not Unicode/emoji substitutes. Creating a task exits settings after successful creation. Settings section navigation is real and retains drafts.

- Type: implementation
- Status: delivered for local integration
- Recipient: main agent

## Final scope clarification

> ⚠️ **部分取代（2026-09-15，R69）**：本节中「Before a thread exists … clicking creates through the ordinary production path」的**点击才创建**折中已被 [R69 Home lazy draft composer](vega-r69-home-lazy-draft-composer.md) 取代。首页现在直接渲染真实 Composer，任务在**首次提交**时才落库；`新建任务`/⌘N 也不再急切 INSERT。本节其余内容（模式/权限菜单、上下文条、分支选择器、设置间距）不变。

Mode and permissions use compact real menus; current values remain visible, existing option handlers/focus semantics remain. Token/cost appears in the context strip to avoid an extra composer footer. Unsupported project-name projection is not invented: the existing branch selector remains the truthful context action. Before a thread exists, the same centered heading/card geometry uses an explicit “新建任务并开始输入” action; clicking creates through the ordinary production path and opens the editable composer. It is not a fake text input and never auto-sends. Main approved this smaller lifecycle-preserving scope. Large settings windows add 46px vertical spacing; compact windows conserve form height. Existing rendered pricing/Thinking tests explicitly route to their now-selectable sections; all behavior assertions retained. Pricing preflight uses a one-shot visible pricing route.

Active conversations place project/branch/token in the header and retain only the ~111px input card at the bottom; the branch popup flips below the header and remains a deferred overlay. Header project labels truncate. Empty threads retain the 36px context strip. Native review found the app-global Escape→CloseSettings binding winning over raw key events; compact menus now use a scoped Escape action, with the production-global binding and active handler included in the keyboard regression.

The active global handler made the Escape regression reproduce. Registration order alone failed. GPUI ranks an unscoped binding at the deepest context, so the final fallback is scoped to `VegaWindow` on both normal and Diff roots. Nested menu/file/permission contexts can win; the regression checks compact Escape avoids the global handler and a subsequent closed-menu Escape reaches the fallback. Original fallback responsibilities (settings, rename, delete overlay) remain. Tool cards already collapse output by default; their 48px safe summary rows remain an intentional nonpixel residual versus ZCode disclosure rows.

Settings takes focus on its initial section navigation only on first route creation; returning restores composer focus. This prevents the scoped window Escape fallback from losing its dispatch path to the hidden previous input. Rerenders do not steal focus from form editing.

## Verification freeze and delivery

- verified_at_utc: 2026-09-05T16:31:52.158106+00:00
- branch: feat/r8-zcode-parity
- source_contents_sha256 (Rust/TOML/lockfile, including tests): `ec9cda999fbe2f4dbef6a4293cd7519bdc88554176d64f2ae7fe0d3ecdceb485`
- Shared build target used; no dependency/DDL changes; runtime tree has no UI dependency.

| Requirement | Evidence | Result / raw log basename |
|---|---|---|
| Final formatting | cargo fmt --all -- --check | PASS; vega-r8-delivery-fmt.log |
| Final UI/theme | cargo test -p vega_ui -p vega_theme --locked | 136 + 6 PASS; vega-r8-delivery-ui.log |
| Final strict lint | cargo clippy --workspace --all-targets --locked -- -D warnings | PASS; vega-r8-delivery-clippy.log |
| Final workspace build | cargo build --workspace --locked | PASS; vega-r8-delivery-build.log |
| Production Settings initial focus | pricing_settings_and_agent_preflight_production_e2e exact | 1 PASS; vega-r8-final-settings-focus2.log |
| Pre-Escape full workspace development snapshot | cargo test --workspace --locked --no-fail-fast | 901 PASS / 0 FAIL / 1 ignored; vega-r8-final-workspace-test.log |
| Post-Escape app package | cargo test -p vega_ui -p vega -p vega_theme --locked | app59 PASS / 1 FAIL; vega-r8-postescape-ui-app-test.log |
| Same Diff exact isolation | diff_refresh_intents_keep_content_during_background_and_retry exact | 1 PASS; vega-r8-postescape-diff-isolated.log; does not erase package failure |
| Native UI | main-owned real app at 960×600 and 960×750 | scoped geometry, modes/permissions/model, @ acceptance, menu/file Escape, immediate Settings Escape, Light/Dark PASS |

Raw logs live under /private/tmp with the basenames above. External native artifact provenance records all raw-log SHA256 values and signed/source binary hashes. The earlier full-workspace run is explicitly a pre-Escape development snapshot; its exact aggregate source hash was not captured then and it is not claimed as the final tree's full test result.

## Preserved failures and accepted residuals

- The post-Escape app run reached Diff retry terminal `GitFailed`, with existing content retained (one file, +1/-0). The same test passed once in isolation. Main accepted this previously known intermittent area as a local-review residual; no Diff assertion, timeout, backend code, or failure log was weakened/deleted. This is not an all-green final app package or release approval.
- Escape red: active app-global handler consumes the key while compact remains open. Binding-order-only attempt also failed. Root-context scope green includes zero global callbacks for nested Escape and one callback for subsequent closed-menu fallback. Native confirmed menu and @file dismissal.
- Initial Settings focus test caught requested Pricing section versus default section mismatch; constructor targets the requested section before focus, exact production E2E green.
- Earlier UI test depended on directly focusing now-hidden segmented controls; updated to real compact trigger plus arrow navigation, retaining all event/state assertions. Two interim test compile failures were borrow-check issues in that new test. Strict lint initially found the now-unused old focus helper; removed helper, retained assertions. All raw logs retained.
- Tool safe-summary rows, missing unsupported automation/plugin navigation, original Vega identity, and explicit no-thread create action are intentional scoped differences. This is not a claim of full-client pixel identity.
- Performance tests deferred by user; real provider/network/billing not exercised; ignored Keychain test not counted as pass. No push, master merge, release or installed-app replacement performed by implementation agent.
