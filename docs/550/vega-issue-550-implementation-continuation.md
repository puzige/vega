# #550 折叠箭头实现续接

## 当前阶段与保存版本

2026-10-23 续接；已保存原始 28 个开发测试及支持为 `6b675d2608dd9653f21acf25109192264cd67075`，源码 SHA-256 `db70f73a94dbee5adc32c9c067b5f0d8524290424603c3b9f4a5211f79a65ea8`，[源码](https://github.com/puzige/vega/blob/6b675d2608dd9653f21acf25109192264cd67075/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)、[patch](https://github.com/puzige/vega/commit/6b675d2608dd9653f21acf25109192264cd67075.patch) 已读回 blob 一致。源码定义数不等于发现/执行数，未取得本卡云端结果。

已正常 merge master `9f9fa092a3ca391a65f08419c9206f32979119e6`，本卡 merge head `e24c6b253da99bcf7cdac32f56144f8597f81c18`。当前仓库政策取消强制 TDD，旧规格的等待 RED/冻结后才实施条款为历史；D01–D11 和 #416 全部产品边界不变。Rust 测试仍不得进入 master，失败不能靠删测/放宽断言消解。

## 实现范围

1. 仅两处 disclosure helper 的箭头：单个 ChevronRight，收起 0、展开 π/2，120ms 默认 ease_out_cubic，稳定 owner/property key，正常连续重定向，初挂/重挂终态。
2. 消费 #549 单一 writer 的 `Motion::DISCLOSURE_TURN_MS` 和 `interaction::decorative_value`，不自建曲线/计时/activation 引擎。真实 glyph 可见性由实际 GPUI 样式及 content mask 决定，不能用 expanded 单一布尔值冒充。
3. 字形固定 12×12 或 16×16、原 slot 不变；header/body/aria-expanded/focus/actions 即时，只有绘制方向插值。原 group_hover、expanded、keyboard-focus 显隐、禁用、无内容、Tab 顺序保留；不为了尾帧把已隐藏箭头继续显示。
4. 隐藏、clipped、失活、Reduce Motion、禁用时终态且停止装饰帧；无 inactive draw 即快速恢复不重播。业务流式/permission/terminal/输入活动不暂停，不改 shimmer。
5. #416 首次可信 Final 收起一次及真实选择/回焦/产物/其他轮/权限入口完整保留；不借此改历史/消息缓存/队列/导航算法。

## 已核 API 与 harness 修复

- KitIcon::rotate 必须在类型擦除前；gpui-component 0.6.1 的 into_svg 为私有，Svg::with_transformation 是公开且只改绘制，不影响布局。若需 paint-time 可见性/旋转，选最小透明 Element 接线，不修改 registry、不凭空调用 AnyElement.rotate 或私有 opacity API。
- 实际构造/绘制使用的角度必须与开发 paint recorder 相同；常量、仅目标、另算采样或重写镜像决策不能当绘制证据。证据仍是配置参数/真实委托 paint，不是 GPU transform 独立读回或用户观感。
- 本包宏生成依赖 `gpui::run_test`，Cargo 名称是 gpui_kit；测试模块目前未显式提供 `use gpui_kit as gpui`，需要按锁定宏源码修正，必要时消歧 builtin test。此类导入/格式/API/fixture 修正不改变行为断言，不提高递归上限。
- 允许最小 helper/caller API 接线，若改变 `&Window` 为 `&mut Window`，五类生产 header 及开发 fixture 全部一起核对。不改权限、Final、失效/测量算法以迁就测试。
- 2026-10-23T03:34Z 静态复核发现测试 `pending_permission_remains_visible_and_usable_after_trusted_final_collapses_process` 使用不存在的 `PermissionDecision::AllowOnce`。真实 `types/permission.rs` 返回变体是 `Once`，`permission_card.rs` 的一次允许动作也返回 `Once`。这是保存包的测试 API 错误，后续仅将匹配变体修为 `Once`；仍须验证真实队列许可、可见入口和原动作结果，不改产品返回类型或减弱断言。

## 暂停后恢复检查点

2026-10-23T03:28Z 已确认旧子执行不存活，保存测试源码仍为原 SHA-256；本卡没有产品源码改动。父已 fetch 并正常 merge 新基线 `5a1ec8ad26fa53fd6210a8e58b6ad85e739dd8bb`，任务 HEAD `c8a9e1039ffe337af5fff51ff9b7b1565ba741ea`。新基线的许可来源/展示资格与阶段恢复实现保留，不从旧 ref 复原。本阶段继续等待共享接口停写后串行接入；宏导入、上述枚举及必要格式错误归测试接线，尚无编译或行为执行结果。

下一阶段由父集成已停写共享提交后，复用第二层 writer 实现；停写固定版本后独立审查与现有 Actions。只允许本地静态/格式/离线元数据检查，无本地执行 Vega、编译、check、Clippy、test/nextest、bench/profile 或 CUA。最终开发验证、证据固定、独立删测后，对最新无 Rust 测试 head 跑完整 required CI，再合并等待用户手测。

## 2026-10-23 中断保存检查点

旧实现执行 `01M5KQETE3R8NRARENX5G0XZMK/01M5KQETE3NRMWMH4MAMDZNSQ9` 已 interrupted，非 READY。实际保存 `disclosure.rs`、`disclosure_paint_probe.rs`、`disclosure_motion_tests.rs` 三文件 202+/158-；该 tracked diff SHA-256 `eac4c44f84d4d0fef984439cb57a4e4a25492dfa597cce2ca0d1bab0d26df8c0`。只有 paint-time SVG/样式与 mask gate、真实 recorder 及两个 API 接线修正；五类 caller、Final、permission/list/shimmer 算法未改。尚无云端验证。

父 fetch 后正常 merge master `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`，本卡 HEAD `1560e8aeaff15e41e31e5f40532a94bc13165787`；保留 #344 core/message_anchors 新行为和全部 28 项原断言。共享源已以 `4c3c75b9977748c0a053739244873b7d4284a757` 接入，不重复实现。下一阶段只收敛此保存补丁的透明委托、生命周期及真实 API/fixture 复核，固定最终内容并 STOP，供非写入者审查和现有 Actions；不重做大范围调查。

## 2026-10-23 窄范围最终格式续接

- 本轮读回 HEAD 与上述三文件 diff SHA-256 均匹配中断检查点；三个源码 SHA-256 也与父指定值一致，格式化后仍逐字节不变。保存实现使用 paint-time 实际 SVG 样式与 content mask 判定可见性，复用共享 120ms sampler；绘制与 recorder 使用同一角度。宏所需 `gpui` re-export 与 `PermissionDecision::Once` 是已保存的 API 接线修复，不改变行为断言。
- 五类生产 caller、Final、list/permission/shimmer 算法未改；28 个测试定义及原断言保留。定义数不是 CI 发现/执行数；未取得 CI 结果，不宣称 GREEN。
- 之前只有单文件格式检查通过；父实际 `cargo fmt --all -- --check` 退出 1，失败位于测试独占支持，不能记作此前全量格式已通过。本轮获准执行 `cargo fmt --all`，实际退出 0；只格式化 `core.rs` 四处测试记录、`phase.rs` 一处测试记录、`disclosure_layout_probe.rs` 的 `Item` literal，以及 `lib.rs` 的 cfg(test) 模块排序，无新增算法或生产行为改动。
- 最终 `cargo fmt --all -- --check`、`git diff --check` 与实际 tracked diff SHA-256 随停写回报记录。未执行 Vega、本地编译/构建/check/Clippy/测试/nextest/bench/CUA；未更改 Git/index、依赖、CI、共享 motion 或 feature。后续由父提交、非写入者独立审查并使用现有 Actions 验证；开发验证留痕后，合并前仍须清理 Rust 测试及独占支持，对最终无 Rust 测试 head 跑完整 required CI。

## 2026-10-23 原始云端编译失败后的窄修复

- 原始首次云端 [Actions 39919579054](https://github.com/puzige/vega/actions/runs/39919579054) 对应实现 `fcf67c8b9038b298acff361bdfdc0b3aedce853e`：Nextest job `120301289770`、Clippy job `120301290193` 和 required 汇总 `120303414725` 均失败。原始日志 SHA-256 已核对为 `f28f302860fae839cb3f2d4cbfc5e7b8876a89ed7100fda505d97447208620ff`。失败发生在编译阶段、Nextest 发现/执行之前；没有取得发现/执行计数或行为结果，不是测试行为失败、GREEN 或 `no Rust tests`。
- 父已 fetch 并 merge master `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae`；本轮修复基于 HEAD `7cf573b4659e957d5e13f7df9294a882a0d375a6`，保留 #575 context usage。此次单一 writer 不执行 Git/index 修改、提交、fetch、push、PR 或 CI 操作。
- 分类与实际修复仅限以下五处，不改变验收：
  1. `disclosure.rs` 的 `self.inner.id()` 被解析成消费 receiver 且需要参数的 `InteractiveElement::id`，产生 E0061/E0308。改为 `Element::id(&self.inner)`，继续委托真实 SVG identity，不伪造 ID、不返回固定 `None`。
  2. 同文件 `Transformation::rotate` 需要 `Into<Radians>`，`f32` 不满足，产生 E0277。仅将同一采样值包装为 `gpui_kit::Radians(radians)`；实际 paint 和 recorder 仍使用同一个原始 float。
  3. `icons.rs` 的 test-only `configured_chevron_paint_control` 存在相同 E0277；实际 `KitIcon::rotate` 同样包装 typed `Radians`，observer 仍记录原始 float。
  4. `disclosure_motion_tests.rs` 的 fixture 通过私有 `artifact` 模块访问服务，产生 E0603。改用既有公开 re-export `vega_conversation::ArtifactService::in_memory_ui_fixture`，不扩大 API 可见性。
  5. 同测试模块的显式 `gpui` re-export 被原始 Clippy `-D warnings` 证实冗余；仅删除该导入。此诊断取代上文保留/补充显式宏导入的旧建议，父模块 prelude 的现有上下文保留，不增加 alias、allow 或递归上限。
- 前两处是生产实现的 API/类型编译错误，后三处是开发测试支持/接线错误；不是验收变更，也没有证据将其归为外部环境失败。仅修改 `disclosure.rs`、`icons.rs`、既有 `disclosure_motion_tests.rs` 并追加本文；D01–D11、28 个测试定义及全部断言/期望值保留，无删除、ignore 或放宽。原始保存源码 `6b675d2608dd9653f21acf25109192264cd67075` 的 Git blob 已读回，SHA-256 仍为 `db70f73a94dbee5adc32c9c067b5f0d8524290424603c3b9f4a5211f79a65ea8`。28 只是源码定义数，不是发现、执行或通过数。
- 本地 `cargo fmt --all -- --check` 与 `git diff --check` 均退出 0；精确源码 delta 静态核对退出 0，确认只有上述五项替换，断言/期望值正文未改。未运行 Vega、编译、构建、check、Clippy、测试、nextest、bench 或 CUA；不改 Final、permission、list、shimmer、共享 motion、依赖或 workflow。
- 修复后的 Actions 与新提交 head 的非写入者独立审查均待父安排；旧独立审查已中断，不能视作有结论。继承 opacity 与没有 inactive draw 时的证明边界继续留作审查限制，不在本次编译修复中扩建架构，也不把既有 recorder 证据称作独立 GPU/观感验证。停写回报附最终真实 Git diff SHA-256；后续开发验证留痕、独立删测和最终无 Rust 测试 head 的完整 required CI 仍由父负责。

## 2026-10-23 首次实际行为结果与有界修复计划

`20534df54aff4de6581b04e887fb18ac56148bb4` 的 [Actions 39922954546](https://github.com/puzige/vega/actions/runs/39922954546)：Nextest `120312339386` 实际 `28 tests run: 1 passed, 27 failed, 0 skipped`；Clippy `120312339053` failure（唯一诊断 manual_noop_waker）；required `120313817592` failure。failed-log SHA-256 `40a9dcfd50e0d7431eb389c7ea3bc0626b7c9d58b1aee4025f76c16d4fa79068`。这次编译 API 已修复且行为确实执行，不能沿用首次“未执行”。唯一通过项为真实配置角度 paint control；不证明五类完整动效通过。

父已按原始 27 个根失败归类：16 项止于 `tool():62` 的 apply_finished；10 项 StreamSurface already being updated；1 项 ConversationStream leaked handles。第一类的 proposed→Success 在 reused=true 时本来合法，不能误改生产状态机；真实 `success_truncation_valid` 要求 reused=true 时 truncated=None，而 fixture 提供 Some(false)，导致严格 projection 判 corrupt。下一最小修复仅纠正 fixture metadata，保留 apply_finished 与所有业务断言。第二类 typed WindowHandle::update 会借用 root，fixture 内 focus/事件可同步触发该 root observer；核对实际回调后，将不需要 root 的 window 操作移到已有非 typed update_window，不改生产观察或吞 reborrow。第三类 StreamFixture 字段按声明顺序先销毁 cx，再销毁保留 stream handle；将真实 entity handle 先 drop，再 shutdown App，保留泄漏检测，不泄漏/forget/禁用 detector。Clippy 改用标准 Waker::noop，保留真实 queue/action/future 轮询断言。

父已正常合入最新 master `988e1e37271c666f05ff8eae46d662324e91afd6`（#535 continuation），当前 HEAD `1068c46aa105faf34fd644711b7a70c22a315f28`；不回退它对同 run/Final/阶段投影的变更。上述修复仅现有开发测试接线与文档，不改 Final/权限/list/shimmer/共享源/依赖或 workflow，28 项断言保留。

### 独立审查与仍未闭合的 D05 证据

非写入者对精确 `20534df54aff4de6581b04e887fb18ac56148bb4` / base `b032d0d2ab9ee057137f3e5ad2c0e517011ea1ae` 已返回有界静态审查：没有新增已证实产品缺陷；P2 验证问题是 `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion` 的 blocker activate/run_until_parked 会触发真实 inactive draw，当前断言不能证明“没有 inactive draw”。后续须保留原角度/续帧断言并增加真实 root render/paint 的阳性对照和无中间 draw 检查；锁定 public API 尚未取得可恢复抑制绘制的路线，需明确作为未决，不能假装已覆盖或删回归。继承 opacity 只读到 local style，静态扫描未找到当前 disclosure 位于 zero-opacity ancestor 的实际消费路径，属于证据限制而非已证实产品缺陷；不因此扩大框架 API。

D05 缺口与实际 27 项失败均解决前，不清理测试或合并；已知框架限制不掩盖其他失败，反之亦然。

## 2026-10-23 实际行为失败后的窄范围 harness 修复

- 本次停写基线为 `1068c46aa105faf34fd644711b7a70c22a315f28`，保留父先前未提交的上述行为结果/审查记录，仅追加本节。原始 `/tmp/vega-550-20534-failed.log` 已读回相关根失败且实际 SHA-256 为 `40a9dcfd50e0d7431eb389c7ea3bc0626b7c9d58b1aee4025f76c16d4fa79068`；原 run `39922954546` 仍是实际执行 28、通过 1、失败 27、跳过 0，Nextest `120312339386` / Clippy `120312339053` / required `120313817592` failure，不把本轮静态修复称作行为通过。
- 16 项首个失败属于 fixture metadata 错误：只将 `tool()` 的 `truncated: Some(false)` 改为 `None`，符合真实 `success_truncation_valid` 对 `reused=true` 的约束。保留合法 proposed→Success、原输入（无 timeout 使用解析默认值）、`apply_finished` 严格断言及所有行为期望；未增加 approval 或改生产状态机。
- 10 项 root reborrow 属于 harness 接线错误。锁定 GPUI `window.rs:6950–6955` 的 typed update 确实通过 `view.update` 借用 root；`focus` 会 refresh，`dispatch_key_event` 在 dirty 时先 `draw`（`window.rs:5606–5609`），故在贷款尚未归还时可重入 root render。仅把现有测试中 55 处不使用 root 参数的 window 操作/查询改为已有非 typed `HeadlessAppContext::update_window(window.into(), |_, window, cx| ...)`，闭包正文、选择/焦点/输入/权限动作和断言不变；需要真实 view 的更新保留 typed API。现有 draw helper 原本已非 typed，本轮未把它算作新修复。不改生产 notify/observer/业务回调，不吞 panic。
- 1 项泄漏属于 fixture 生命周期：`StreamFixture` 字段改为 stream、window、cx 声明顺序，真实外部 stream handle 先释放，随后 HeadlessAppContext shutdown；不 forget、不关闭泄漏检测、不改原 entity release 断言。唯一 Clippy 诊断改用标准 `Waker::noop()`，移除自制 Wake 实现，仍在真实可见 `PermissionEnter` 后 poll 原 PermissionQueue future 并严格检查 `PermissionDecision::Once`。
- D05/P2 仍未闭合。本轮在现有 fixture 的 `ProductionHeader::render` 入口加入实际 root render 计数，先用显式 refresh/draw 严格检查计数增长的阳性对照，再在原快速失活/恢复测试中检查失活后以及恢复后、显式 draw 前计数均完全不变；原中间角度、终态和停止续帧断言全部保留。计数不是另造 sampler 或目标值镜像，也不是独立 GPU 读回。锁定 TestPlatform 的 active-status 调度经 foreground executor（`platform/test/platform.rs:284–303`），原 `activate/run_until_parked` 仍可能触发自动 inactive root render；新增严格前提预期会明确暴露该 framework/harness blocker，未取得无中间 render 的可恢复 public API 路径。因此不能宣称 D05 已覆盖或本轮可全绿，下一步须由父审查该控制并解决真实 no-inactive-draw harness 路线，不扩生产框架/私有 API/registry/依赖来消解它。继承 opacity 证据限制保留。
- 最终本地只执行快速格式/静态检查：单文件 rustfmt、`cargo fmt --all -- --check`、`git diff --check` 均退出 0。精确授权替换再交 rustfmt 的静态比对退出 0，实际源码字节完全匹配：五项 fixture/lint 替换、55 处 root-independent API wrapper 与最小 render-counter 接线/新增严格前提；全部 28 定义/名称、原断言/期望及 D01–D11 保留，无 ignore/allow。最初自写静态归一化脚本未处理 rustfmt 的闭包括号变化而退出 1；改为直接以 rustfmt stdout 比对完整预期源码后退出 0，属于静态观察脚本修正，不是修改产品/断言来过检。
- 仅修改现有 `disclosure_motion_tests.rs` 和本续接文档；不运行 Vega、本地编译/构建/check/Clippy/test/nextest/bench/CUA，不调用 Git/index 修改或 fetch/push/PR/CI。新提交、Actions 和独立审查由父负责且均待执行；本轮修复没有运行结果。父须按新 head 的实际失败证据分类处理，不能重复旧 cloud run 冒充本轮验证；D05 和原失败真正解决前不清理测试或合并。

