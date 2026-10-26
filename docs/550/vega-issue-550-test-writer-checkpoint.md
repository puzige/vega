# #550 先行测试 writer 恢复检查点

状态：**BLOCKER / 可独审的测试接线补丁，不是完整验收包 READY**。生产未实现，未独审，未取得 Actions RED/GREEN；不得据此进入生产实现。原冻结 spec 保持原文及原 SHA-256，不扩写验收。

## 精确版本与实际改动

- HEAD：`2ba904f7074115de5ee89cc18c376c083f803357`，branch `feat/550-disclosure-motion`。
- 原 spec：`vega-issue-550-disclosure-motion.md`，SHA-256 `0cfd2210aebee0638a473a753e01196f09146c552cb0ca23bbe54e381afeb6b9`。
- 本轮新写 16 个 GPUI 行为测试；未从旧 ref 恢复任何测试文件或全集。测试文件放在 `thinking` 的测试子模块，可访问真实 Thinking 构造与只读来源，不改构造函数可见性。
- 新增 `disclosure_paint_probe.rs`：两个真实 disclosure helper 构造时捕获 owner、expanded、enabled、真实 FocusHandle；包装真实 KitIcon 对应 AnyElement，原样委托 request_layout/prepaint/paint。paint 后记录实际 asset、资产路径、glyph bounds，以及当前无 transform 的零旋转事实。显隐观察原 Div 的 `Interactivity::compute_style` 最终 opacity，不复制 hover/focus 判断。
- `icons.rs` 观察点位于实际 `kit_icon` 的 `.into_any_element()` 之后，不存在 IconElement 或 AnyElement.rotate。测试通过真实 `IconNamed::path(icon_name(kind))` 加载并用公开 SvgRenderer 栅格化同一资产，避免空 asset source 被生产 `.log_err()` 吞错后误认为成功。此阳性对照加委托 paint **不是最终 Scene transform 的回读**。
- 测试使用锁定 GPUI 的公开 `HeadlessAppContext::with_asset_source(NoopTextSystem, Kit Assets)`；无 GPU/CUA/系统窗口操纵。NoopTextSystem 的几何是确定性测试几何，不是用户真实字体/观感证据。
- 所有 `window.draw` 均通过非 typed `HeadlessAppContext::update_window`，避免 typed WindowHandle::update 借用根实体时再次 render 根实体。几何 wrapper 直接记录真实 Element bounds，不使用绝对 canvas 当父 bounds。
- 生产文件仅 test 接线和非 test 恒等包装；函数签名/生产行为不变。`tool_card.rs`、`tool_activity_group.rs`、`run_activity.rs`、`phase.rs`、`core.rs`、`message_anchors.rs`、interaction、theme 均逐字节保持 HEAD。
- Cargo 仅 dev-only `gpui_kit` 的既有 test-support；离线必要 lock 解析 914 → 924 包，914 个既有 package 身份/来源/checksum 保留，增加 10 个 feature 传递包，无依赖升级、无新增直接依赖。锁文件增加 GPUI test-support 的 backtrace/proptest 等边；同名版本消歧不是升级。
- 没有 commit/push/PR/Issue/Project 操作、嵌套委派、主工作区写入或生产实现。

## 冻结矩阵 → 当前测试

以下为源码断言映射，**不是运行覆盖率或通过结果**。位置均为 `crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs` 的函数名。

| Spec | 当前测试/真实边界 | 尚未闭合 |
|---|---|---|
| D01 | `production_glyph_probe_reaches_both_helpers_and_real_endpoint_assets`；`disclosure_turns_while_production_content_changes_in_the_same_event`：Thinking/Tool/ToolGroup/Run/Stage 真 header mouse/Space，立即 expanded/row_count，60ms painted asset/方向、120ms target | 完整 a11y 属性/树未观测；旋转变换读回见 B1 |
| D02 | `disclosure_retargets_from_the_painted_angle_without_queuing_or_resetting`：40ms→反转20ms→再展开，瞬间连续、终态、callback 排空；`independent_disclosure_owners_never_share_the_transition_channel` | B1 的未来非零 rotation 接点 |
| D03 | `initial_mount_remount_and_unrelated_stream_redraw_adopt_current_direction`；历史 hydration 测试 | 当前 remount 是同 Entity 从实际元素树卸载/重挂，不冒充完整虚拟列表 recycle；历史流式身份有待 CI 前置验证 |
| D04 | `collapsed_arrows_remain_hover_and_keyboard_discoverable_without_extra_focus_targets`：真 mouse move 与真实 header focus，记录最终 opacity；`empty_thinking_has_no_expand_action_or_keyboard_trap`：空源不触发 | 完整 Tab/Shift-Tab 顺序、无详情 Tool 的 disabled/no-content 前置未齐；测试名不替代这些断言 |
| D05 | `reduced_motion_and_offscreen_disclosures_settle_without_replay`：Reduce Motion、卸载/重挂；`inactive_disclosure_adopts_truth_and_does_not_replay_when_reactivated`：真实第二 headless window 激活→原窗 inactive；终态 callback 排空 | 离屏但仍 mounted、hidden/disabled 中途切换的精确门控未齐；原 Thinking shimmer callback 隔离回归未齐，不用整窗零帧冒充活跃 shimmer 不变量 |
| D06 | `long_bodies_keep_geometry_and_owner_identity_across_theme_and_window_width`：真实 Element/glyph bounds、240/480 宽、浅深；`settled_arrow_frames_do_not_rematerialize_messages_or_change_the_reading_anchor`：真实 list bounds、测量 cache、materialization counters、anchor | **B2：cache/materialization 不等于零 item invalidate/零正文重测**，未宣称完整 D06 |
| D07 | `first_trusted_final_collapses_reading_selection_and_focus_then_enter_reopens`：真实事件、阶段手动展开、真实鼠标拖选、源焦点前置、首次可信 Final、清隐藏选区、同可见 run header 回焦、Enter再展开、非跟随 anchor | 尚缺单独纯上滚/纯持焦/纯手动展开的组合，拖选位置等可达前置待 Actions；不把 WIRING 前置失败算 RED |
| D08 | 同上：重复 Final、MessageFinished、后续时钟 redraw 后手动展开保留；completion_response 精确 Final 源文 | 流式生成中连续操作场景仍未完整齐备 |
| D09 | `final_does_not_steal_composer_or_other_turn_focus_and_selection`：实际 composer/其他可见 run header FocusHandle；`visible_final_and_other_turn_selections_survive_first_collapse`：真实 Final 与其他轮拖选、选择/焦点保留；list anchor 对照 | **B3：同阶段 Artifact 焦点/选区未接通**；无伪造隐藏 focus |
| D10 | `invalid_or_missing_final_and_failed_or_cancelled_turns_keep_unknown_body_visible`：坏范围、缺 Final、Interrupted/完成、真实失败 Tool；`historical_final_missing_and_corrupt_metadata_do_not_hide_unknown_source`：三轮真实 HistoryPage、缺/坏/可信 metadata、原文保全 | 权限入口真实前置未齐；失败工具状态断言不冒充安全入口可见性 |
| D11 | 长输出、窄宽/浅深真实 renderer 几何；原文与焦点事件即时 | 长 Thinking 流式交错/输入响应测试未完整齐备；真实观感/性能按原 spec 留用户手测，不跑 profile |

当前所有测试均保留，不 ignore、不放宽断言。矩阵缺口是**原冻结验收尚未完成的测试接线工作**，不是缩减验收；不能将此补丁称为“全部测试已齐”。

## 具体 BLOCKER 与下一验证

### B1：基线 actual paint 可接，未来 SVG transform 无公开读回

原 API 证据：锁定 registry `gpui-component-0.6.1/src/icon.rs:84–90` 的 transformation 私有，`:164` rotate 写入 transformation，`:188–190` 交 Svg；`gpui-pre-0.3.4/src/elements/svg.rs:140–154` 在 paint 内计算 transformation 并交 `Window::paint_svg`；`window.rs:4605–4669` 插入 MonochromeSprite。Window.painted_quads 不包含这个 transform。`Scene.monochrome_sprites` 虽公开，但当前 Window 没有公开的 scene getter；`PlatformHeadlessRenderer` 需要非 facade 导出的 image::RgbaImage 签名，不能在本轮擅加直接依赖或改 registry。

本轮 recorder 的 `rotation=0` 仅对应**当前未调用 rotate 的真实 kit_icon**，displayed_angle 只区分 Right/Down 资产端点，不运行 transition 算法，也不制造中间角度；未来生产实现不可以继续用常量 recorder 证明 GREEN。下一动作：父独审这个实际委托 paint 接线；首轮现有 draft Actions 必须先见阳性对照成功，再将中间 painted orientation 断言失败判为行为 RED。若前置/借用/类型失败，只修 harness 并独审，不算 RED。非零 rotation 的只读观测接点必须在任何生产实现前另行审定和冻结，不能实现后追改期望。

### B2：完整 D06 精确测量边界超出本轮允许观察文件

当前允许 observer 仅 icons/disclosure。`core.rs::invalidate_item:832+` 同时执行测量 cache 删除和 ListState::remeasure_items；ListState 的实际测量状态私有，缓存/最终 bounds 不变不能排除一次额外重测。本轮没有写 shared core/message_anchors 来掩盖缺口。下一动作：父协调允许纯 cfg(test) 只读 invalidation/真实测量 recorder（不改生产，不碰 #344 算法），或提供现有获准可用的等价生产观察边界；在生产未改前补齐 D06。

### B3：真实 Artifact 身份不能在本轮 UI-only 范围伪造

`vega_conversation/src/types/artifact.rs:13–17` 的 ArtifactCardId 字段 crate-private，`:39+` 的真实 ArtifactCard projection 必须携带该能力。ArtifactService 的真实构造还要求 GitWorkspaceService，既有测试白名单未授权改 conversation fixture 或运行 Git/服务。下一动作：父提供真实、安全的内存测试 capability 构造入口，或明确授权 conversation 的测试-only fixture 接线并独审，再写同阶段产物焦点/选区回归。不 transmute、不伪造 ID、不用隐藏无挂载 FocusHandle 替代。

D04/D05/D07/D10/D11 尚未齐备的组合继续按原 spec 写测试，不能随生产实现再补。上述包通过完整前置独审和有效 RED 前，全部生产继续不动。不要将 B1–B3 当成产品 RED 或“可以先实现”。

## 静态执行记录

本轮命令仅静态，不运行 Vega。下列均真实执行：

| 命令/检查 | exit | 事实 |
|---|---:|---|
| `git status --short`、branch、`git rev-parse HEAD`、spec shasum | 0 | 与接续包 HEAD/spec 一致；父已 fetch+ff，不重复改 Git/index |
| 首轮 `rustfmt --edition 2024 --check <两新文件>` | 1 | 仅格式差异；不是 RED |
| `rustfmt --edition 2024 --config skip_children=true <本轮六 Rust 文件>` | 0 | 格式化，无编译 |
| `cargo fmt --all -- --check`（最终绘制/几何接线后重跑） | 0 | workspace 格式通过，不代表类型/借用/行为通过 |
| `git diff --check` | 0 | 差异空白检查通过 |
| Python 静态投影、manifest/16 测试定义/无注释-ignore 清点 | 0 | 四个混合生产文件移除 test 接线及恒等包装后等于 HEAD；受保护生产文件不变 |
| `cargo metadata --no-deps --offline --locked --format-version 1` | 0 | 只证 manifest，未解析完整图 |
| 首轮完整 `cargo metadata --offline --locked --format-version 1` | 101 | test-support 需要 lock 更新；接线故障，不是 RED |
| `cargo metadata --offline --format-version 1` | 0 | 必要离线解析，新增10个传递包 |
| Python lock 身份审计 | 0 | 914个既有包不删、不换版本/来源/checksum，914→924 |
| 最后完整 `cargo metadata --offline --locked --format-version 1` | 0 | 完整依赖静态解析成功，无编译/测试 |
| `gh issue view 550 --repo puzige/vega --json body --jq .body` | 0 | 原始 Issue 正文只读核对，未修改远端状态 |

未运行：Vega、build/check/Clippy/test/bench/profile/CUA、Actions。未取得 Rust 发现/执行/通过/失败数量；16 是源码测试定义数，不是 Nextest 发现数。所有类型/借用/真实绘制、测试前置与 RED 待父独审及现有 Actions。

## 内容标识与恢复

| 文件 | SHA-256 |
|---|---|
| `Cargo.lock` | `d7c701e424bae4802ca725b937899e3875d0d0293b7d3be5558ff707849ab888` |
| `crates/vega_ui/Cargo.toml` | `3f27cebbfc927e48576b21a78e2684c05ff4da14c2a70cb56e7851cd175edde5` |
| `crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs` | `4e29d257bad073cfd64d70989b1c00e618a8053485fc5ec7a28cdf8b465dacc4` |
| `crates/vega_ui/src/disclosure_paint_probe.rs` | `c1bd496ccc12c8ad6fc5c49d21a13cad80b4b2cef593206009c59097ed9193c4` |
| `crates/vega_ui/src/disclosure.rs` | `452ea09462bda132faad14dba7a0b9eb8debf5fe6df37f6443993c69f19522a9` |
| `crates/vega_ui/src/icons.rs` | `c491555858e051136371d39d6c01f169a4a4780ac9b4f2c475cc01928c272124` |
| `crates/vega_ui/src/conversation_stream/thinking.rs` | `c4a1d44c2b2073b007cfd9801d3604ed2264ade2693d671fc1d2ea042b8f4836` |
| `crates/vega_ui/src/lib.rs` | `b075ff03bc181db7933ea22bcfcb0b5c25105ba7b657e0bdbd7a3ff9facbe50e` |

最终报告另附包含未跟踪文件的内容 bundle hash，不只 git diff --stat。本文件不记录自身 hash。未提交本地产物只能作为父本轮接回版本，不满足最终 PR 可恢复源码要求；父须在正式流程固定完整 SHA/原始 patch 与 Actions 证据。

写入者将在最终 hash/静态复核后明确停止本轮全部写入；父未 followup 前不做任何生产实现、Git 或外部状态变更。
