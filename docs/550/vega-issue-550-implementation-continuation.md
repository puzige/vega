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

## 20/28 后的责任层收敛与尚未授权的冲突

head `76fd200952154dc5be8c6cda1f6cd513badff106` 的 [Actions 39927968952](https://github.com/puzige/vega/actions/runs/39927968952)：Nextest `120328923841` 实际 `28 tests run: 20 passed, 8 failed, 0 skipped`；Clippy `120328924012` success，required `120331512074` failure；failed-log SHA-256 `16dc786ac7f373cd2d128d91032de81a9d715e24c158cf1edb794afdd9b423f8`。此前 16 项 metadata、10 项 root reborrow、1 项泄漏不再作为本轮全部失败的归因；独立静态增量审查确认这轮修复责任层正确。父另 fetch/正常 merge master `a439483cf2c377fe59f1dfb9b487bfb2200d5c64`（#577 / coordinated storage），当前任务 HEAD `1f706e4b7053be5da6625196d8d661bfa8896163`；保留该存储协调，不借 SQLite writer 序列化冒充 ordinary launch owner 锁。

父读取全部八个根断言并复核实际源码；只读诊断执行虽异常结束，但具体源码结论已由父定点确认，不能称完整审查通过：

| 失败/缺口 | 已核责任层 | 当前处理边界 |
|---|---|---|
| independent owner `ProductionHeader already updated` | fixture 在 typed second_window.update 内再 second.read，同 root 重借用 | 改为非 typed query 或 App read，保留两个 owner、角度和真值断言 |
| long Thinking 原 card 文本没追加 | fixture.begin 已发 TextDelta Commentary，真实 content 会 finish_live_thinking 并清 active_thinking；后续 ThinkingDelta 创建新 card，旧 card 不该继续增长 | 真实纯 Thinking 活跃前置，维持同一个 card 的交错输入/旋转；最后再按真实顺序送 Commentary→Final。保留每步完整文本、草稿和几何断言，不按旧 card 输出改期望 |
| shimmer `progress_finished` 未结束 | fixture 用 Completed group；set_thinking 因 terminal 被拒，finish_thinking 没有 current card | 用 Running group 真实挂载同 card、登记 current_thinking 后执行原 finish；不得手动设 finished 伪造通过 |
| no inactive draw 12 vs 11 | 新真实 counter 已证自动 inactive root render，公开 API 限制 | 严格前提/原角度续帧断言保留；BLOCKED，不再重复相同云端重跑 |
| Final 子 `thinking.expanded` 仍 true | #416 契约要求外层过程关闭、隐藏选区/焦点处理；当前 settle_final 只改 outer expanded/body_in_details/ancestor_visible，内部展开位不清 | 属于验收假设与既有产品范围冲突；本轮不删除该断言或改 Final 算法。需先明确 spec/可观察隐藏与重开语义后再处理 |
| disabled Tab skip | GPUI track_focus 使用句柄自身 tab-stop=true，row.tab_stop(false) 不改显式 handle；既有 master 同样存在 | 可能是既存无内容焦点缺陷，与本卡“焦点顺序不改”范围冲突；本轮不改生产 focus 或降低 skip 断言 |
| fold invalidates 3 vs 1 | group observe 会失效 RunActivity、segment 和 owned process-copy 多条；不能从总数直接推断 decoration 重复 | 需要真实 owner/index/事件来源和操作边界；本轮保留 one-fold 严格断言，不直接改成 3 或 <=3 |
| drag selection empty | fixture 用 glyph bounds 猜正文坐标，未证明实际可选 body 已完成准备/绘制 | 需真实正文 bounds/准备阳性对照，不能直接设置 selection 缓存替代真实拖选 |

另有独审 P2 的合法 failed-tool 路径假阳性：Failed 的 exit_code/duration_ms/truncated 必须全 None；旧 fixture 给 Some 值进入 Corrupt fallback，而 activity_state 仍是 Failed。只授权纠正该 fixture metadata 并补真实 failure 输出仍在 card/render 的观察，保留原状态/正文/选择断言，不改变生产严格 projection。

本轮下一 writer 仅修上述三项确定 fixture 错误及合法 failed-tool 数据证据；余下四类验收/观察冲突与 D05 限制保持明确未决，不扩大产品范围或静默放宽原断言。失败未解前不清理测试/不合并。

## 2026-10-23 恢复核对与执行环境阻塞

2026-10-23T10:04:07Z 核对：本地 HEAD `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2` 已包含当前 fetch 得到的 master `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`（#542 产物聚合）；PR #579 远端仍为 `76fd200952154dc5be8c6cda1f6cd513badff106`。测试文件在本地 HEAD 与该远端版本的 Git blob 均为 `73474821b5493d6ff8f7b9f9b8fae5a744e6634a`，本次恢复没有新增源码修复。

原修复执行已确认为 interrupted / recovered_abandoned，无新源码交付。重新安排专职 writer 时，执行工具返回 `capacity_exceeded`；对旧执行请求取消后，查询仍返回 running / release_state unknown，不能声称已释放容量或新 writer 已接手。完整目录查询另返回 receipt too large，现有接口没有可传入的筛选参数；不反复调用同一个失败查询。此为执行环境/委派阻塞，不是产品或测试的新结果，也不绕过仓库的专职实现与独立审查要求。

本次重读原始日志及严格计数边界：#550 在重新激活之前的断言已得到 12 vs 11，不能将该失败解释为只统计了恢复后的绘制。#552 的 14 vs 12 检查在恢复后，不能仅凭该总数细分两次绘制的时点；两卡均未取得符合原验收的无 inactive draw 证据。原断言不变，不修改 framework / registry / CI，不盲重跑。

下一步仍是已列明的三项 fixture 修复与合法 failed-tool 输出证据；恢复条件为专职 writer 能获实际执行名额并确认单一写入权。停写后固定精确内容，非写入者增量审查，再交现有 Actions 验证新 SHA。其余验收/观察冲突与 D05 均保持未决；当前无新 CI、无测试清理、无合并。

## 2026-10-23 有限测试修复接手

- 本轮重新核对任务 HEAD `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2`、测试 blob `73474821b5493d6ff8f7b9f9b8fae5a744e6634a` 和上轮 31 行未提交检查点；接手时完整 binary diff SHA-256 仍为 `6ba98cc001b34663acbf9514ebe387c4cf2f6b443c81fddb6711a874575e5777`。原检查点完整保留，没有后来源码改动。fetch 后远端 master 仍为 `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`，安全更新返回 Already up to date；#542、#535、#344、#577 集成保持不变，不触碰主工作区脏文件。
- 本会话执行目录为空，公开交接无后续占用记录；已成功派发唯一代码 writer，本轮恢复了实际执行名额，不将旧异常执行当作新工作。父负责 Git/index、文档、提交、推送和 PR/Actions；writer 只修改指定测试，必要时在 `tool_card.rs` 增加极小 `cfg(test)` 只读观察，不扩大生产 public API。停写后由非写入者审查固定版本。
- 验收仅为四项已定位的测试责任层修复：非 typed root 查询避免重借用；纯活跃 Thinking 前置并在全部增量后依序 Commentary → Final；仅 shimmer 用例建立 Running group 并登记同一 card，由真实 finish 完成；Failed metadata 三项改为 None，并证明原失败输出保留在真实 card/detail/render 路径。
- `ToolCard` 的 `output_rows`、`detail` 私有且无现有输出 getter。优先真实 header 展开/渲染与已有观察；如需 test-only 接点，只观察真实数据，不另造 projection/selection 缓存或改业务决定。28 项原测试和断言全部保留，其他 begin/fixture 语义不变；本轮不修 no-inactive-draw、Final 子 expanded、disabled Tab、measurement 3 vs 1 或真实拖选空的问题。
- 原始失败日志哈希再次一致，远端 Actions `39927968952` 的 head/job 结果也已读回；这些仅验证旧 `76fd200952154dc5be8c6cda1f6cd513badff106`，不验证当前集成或本轮修复。新结果待停写、提交后交原 PR #579 的现有 Actions，按 5 分钟间隔查询；不改 workflow、不盲重跑换绿。
- 本轮不清理测试、不合并 #550、#552 或相关 PR，保持 Issue OPEN / In progress。禁止本地运行 Vega、编译/build/package、check/Clippy/test/nextest/bench/profile、CUA/OS 自动化；仅快速静态和格式检查。完成实施、独审和新 head 云端结果分别记录，不能以全部 28 项全绿为由扩大授权。

### 限定修复停写产物

- writer 已 READY 并释放写入权，父读回确认：相对 `5da24db0ba4ce9a9f369dffa2807cbc7fc3128c2` 仅两源码共 133 行新增、11 行删除，源码 binary diff SHA-256 `48306ba9399384a3595db3e1f7c59f4a15929352e9ee1e19b7de5b5edb1f38bb`。测试源码 SHA-256 `be049269cbde74cababc2fc79631626da8fff6aa9a694fb096832c4a19c09936`；`tool_card.rs` 为 `2d820e0f3255c95118f8af7c0f4c7cb2dc010fcf63fd9006de3ded0b619e8e07`。
- independent-owner 末尾改 App update 查询，不再在 typed root update 内重借；long Thinking 提取原阶段展开 helper，其余 begin 流不变，本用例保持同一 active card 接收所有增量，再发送 Commentary/Final，额外严格检查活跃身份与未结束状态；shimmer 仅本例新建 Running group，挂载并登记同一卡片，终态仍由原 finish_thinking 完成。
- Failed metadata 的三项改为 None；经真实工具 header paint、focus/Enter 展开、draw、真实 header snapshot 验证可见/展开/焦点，随后精确断言失败正文。唯一新增接点为 `#[cfg(test)] pub(crate) test_detail_output()`，直接返回既有 `detail().output`，不新增投影决定；Corrupt 清空 input/output 后无法通过该正文断言。证据为真实展开/渲染与生产 detail 数据观察，不宣称独立像素文本回读。普通构建的 ToolCard 业务代码逐字节不变。
- writer 报告单文件 rustfmt、全仓 fmt check、diff check 均退出 0；父独立执行 `cargo fmt --all -- --check`、`git diff --check` 与静态范围检查均退出 0，确认 28 个测试名称保留、仅四个授权测试体变化、其余 24 个逐字节不变，以及 ToolCard 唯一差异为上述 test-only accessor。原断言未放宽或删除；writer 静态清点为原 204 个断言全部保留、新增 14 个，不作为执行计数。
- 首次 writer 格式检查仅两处换行，修正后通过；其静态检查脚本首次误把 helper 计入相邻测试，修正函数边界后通过，未改变测试断言。未运行本地编译、程序或测试。固定提交后的独立审查与现有 Actions 尚待取得；五项范围外失败仍未决，不据此清理测试或合并。


## 2026-10-24 artifact 兼容修复接手与实施契约

- 本轮用户只授权修复 `visible_artifact_preview_and_real_control_focus_survive_first_final_collapse` 的 #542 API/fixture/观察兼容，独立审查后取得新 head 的现有 Actions 结果。不合并 #550/#552 或相关 PR，不修改 #552，不清理 Rust 测试，不扩修五类已知未决。当前规则不重新要求 TDD。
- 接手重核原本地/远端/PR head 为 `ed14a21a1c6db5b09a8a4631df5d7e3eeb49c252`，index/worktree 干净，公开交接已释放前轮 writer/reviewer。本会话只读契约核对者没有源码写入权。fetch 后正常 merge master `73977c89eba718da4b2573227c44beb8bcff6911`，本轮基线 `725d63281f891189e81ed0b66665130001e196e6`；上游仅 #307 权限面板，四项旧修复以及 #542/#535/#344/#577 保留，主仓库文件未动。
- 上轮 [Actions 39993796156](https://github.com/puzige/vega/actions/runs/39993796156) 对应 `ed14a21a1c6db5b09a8a4631df5d7e3eeb49c252`：Nextest `120551969424` 与 Clippy `120551969746` 均止于六处 E0599，required `120553551657` failure；Format/Python step success。Rust 发现未完成、行为未开始，执行/通过/失败/忽略/跳过计数未取得；不是 no Rust tests。原始日志仍在且 SHA-256 读回为 `01f9e2e2276e85fb4bc568e57bab1390460deeb76c410b9f604d5f6fb4b83ae5`。公开完整前轮结论见 [Issue](https://github.com/puzige/vega/issues/550#issuecomment-6370815322) / [PR](https://github.com/puzige/vega/pull/579#issuecomment-6370814655)。

### 原验收与当前合法前置

[#542](../542/vega-issue-542-artifact-card-row.md) 明确运行中不向对话流插入产物，产物改为结束回合后的独立聚合；workspace 仍使用 `ArtifactCard::render_row` 和原 `row_count()`。因此旧同段 `RunActivityChild::Artifact` 挂载与 `insert_after_tool/contains_artifact` 结构已不存在，不能恢复。先结束再补当前 Final 也不作为合法事件顺序。

本轮采用同一实时会话、同一路由已完成前轮的合法产物：真实 `register_artifact_call` / `apply_artifact_card` 登记 → `MessageFinished` → 唯一匹配 `StreamEntry::ArtifactGroup` → 已打开同窗 workspace 预览 → 当前轮首次可信 Final。不是历史 hydration 回溯，不宣称继续验证已删除的同阶段内嵌结构。旧结构断言改为精确 card Entity、call owner/message_id、唯一匹配 ArtifactGroup 与聚合 `row_count()==1`；当前轮过程仍须在首次 Final 收起。

| 必保目标 | 修复后的严格观察 |
|---|---|
| 合法产物归属与可见性 | 精确实体/call/回合归属、完成前不入流、完成后唯一聚合；Final 后身份/归属不变 |
| 完整预览 | 当前生产 `render_row` 四行均实际 paint、可见、在 viewport 内，原逐行几何尺寸前后相等；精确比较两行预览正文 |
| 真实控件焦点 | 真实 Preview 控件已有 FocusHandle，paint 后实际设焦，并通过 Enter/ArtifactActivate 产生字段匹配的 `ArtifactPreviewRequested` 阳性证据；Final 后继续保持同一焦点/控件可用 |
| 首次 Final 隔离 | 当前轮真实 Final 路径收起当前过程，以上可见产物的预览/几何/焦点均保持 |

最小 test-only fixture 在同一窗口并列挂载真实 ConversationStream 与 workspace 使用的相同 `uniform_list` / `ArtifactCard::render_row`，默认不启用预览，不改变其余用例前置。已有 snapshot 只提供几何/可见性与 AX，不能读预览 child 正文；允许在 `artifact_card.rs` 增加唯一 `#[cfg(test)] pub(crate) test_preview_lines() -> &[String]`，直接返回既有 `preview_lines`，不另算/复制业务投影，不扩大普通构建 API。该证据是实际行渲染加真实渲染数据观察，不是独立像素文本回读。

### 唯一 writer 与验证边界

- 唯一专职 writer 只改本 artifact 测试、必要的同文件 test-only surface/fixture，以及上述五行以内只读方法；不改其他 27 个测试体、四项旧修复、生产行为/算法、依赖、registry、framework 或通用 CI。writer 不操作 Git/index/commit/push；主 Agent 负责本记录、版本、提交和云端。
- writer READY 必须附精确源码/diff 内容标识、实际静态检查及停写声明；随后主 Agent 核对 28 个名称、其余 27 个测试体与范围外断言保全，固定提交，交非写入者独立审查完整 SHA，继续原 Draft PR #579。
- 本地仅快速静态/格式检查，禁止运行 Vega、编译/build/package、cargo check/Clippy/test/nextest/bench/profile、CUA/OS 自动化。自动测试只交原 Actions，不改 workflow；启动后每五分钟查询，完成/失败通知或用户即时请求除外。新结果分类后仅可继续修本轮授权内 fixture/API/观察错误，范围外失败只记录。

### artifact 兼容修复停写产物

- 唯一 writer 已 READY 并停写，父逐项复核实际 diff：只改 artifact 测试体及默认 `None` 的同文件预览 surface/fixture，`artifact_card.rs` 唯一差异为四行 `cfg(test)` crate-private 只读 `test_preview_lines` 方法；移除此接点后普通构建源文与基线逐字节相同。默认无预览 surface 仍生成原渲染树，不改其余用例前置。
- 真实登记/apply 后运行中无 ArtifactGroup；前轮合法 Final 在 MessageFinished 之前，完成后唯一匹配聚合。当前轮开始后，精确 card/call/owner/group 及聚合行数前后不变；四行实际 paint、全 viewport bounds、原逐行几何比较与两行精确全文保留。真实 workspace Preview focus[0] 在首次 Final 前后均实际持焦，Enter 经既有 ArtifactActivate 产生精确 emitter EntityId 与 thread/project/card/file 请求字段，两次请求严格相等。当前轮真实 process_in_details 从 false 到 true，过程关闭，精确 Final 原文在外。
- 测试源码 SHA-256 `b5d4ca9a458b600d34ba6cac3dfc2bd19da085572f32493f6e440415cec39157`；ArtifactCard 源码 `0877d19ebd5db4149dc6a3a8a1f05de5add202f839a41179ebae43f3718d150e`；两源码 binary diff `19db48afcea186637557382f0139b520ca821d06a0558e499b5c1f563537ce45`。父读回全部匹配 writer 停写内容。
- 父实际 `cargo fmt --all -- --check`、`git diff --check`、词法边界静态范围检查以及唯一 accessor/字节保全检查均退出 0。28 个名称/顺序保留，其他 27 个测试体逐字节不变；四项旧修复、五类已知未决原断言、tool_card、workflow、Nextest 配置与依赖保持。静态 assert 宏计数 218→237，不是发现或运行计数；旧结构断言依据上述契约作当前身份/聚合观察等价替换，不放宽业务目标。
- 未本地运行 Vega、编译/构建/check/Clippy/test/nextest/bench/profile/CUA；新 head 编译、发现与行为结果及非写入者审查仍待安排，不宣称通过。规格偏离无；证据仍是实际渲染加渲染数据观察，不是像素正文或真实文件/权限/外部应用验收。

## 2026-10-24 artifact 修复独立审查与实际云端结果

本轮授权目标已完成：artifact 兼容修复、非写入者独立审查、固定新版本的实际云端结果均已取得。整体 #550 仍有五类范围外未决，保持 OPEN / In progress；PR #579 保持 Draft、不自动合并。没有合并 #550/#552 或相关 PR、修改 #552、清理测试/分支/worktree，未宣称用户手测通过。

### 固定实现与独立审查

- 本轮实现及 Actions 关联 PR head SHA：**`4f633e469113fe25c50ffc006dadc4ed0cd1d05c`**；修复 delta base `725d63281f891189e81ed0b66665130001e196e6`，包含实施前正常集成的 master `73977c89eba718da4b2573227c44beb8bcff6911`。原 `ed14a21a1c6db5b09a8a4631df5d7e3eeb49c252` 四项修复保留。上述实现已正常推送，PR/远端 head 与源码 blob `bf5ce272fd3456886e10cd53bf9f73dcbc9052f5` 已读回一致。
- 实际执行环境：Nextest 与 Clippy 的 Checkout 原始日志均打印 **`c07c27dcc4a62bedad5750851527254abcf3efa8`**（`refs/pull/579/merge`），父节点依次为 base **`b2248996cc68497ef050539fd69f05cc723e4c0a`** 与 PR head **`4f633e469113fe25c50ffc006dadc4ed0cd1d05c`**，tree **`6fb13cad9af9349796e6e1e5c536ce888d71f09e`**。这是 Actions 的临时合成 merge，不是合并 PR；#550 仍未合并。下列行为结果来自这个合成 tree，不能称纯 PR head tree 已直接执行。
- 非写入者 `artifact_review` 对完整固定 SHA 的独立静态审查完成：未发现可证的新增 P0–P3 问题或新增阻断；确认合法聚合前置、四行实际渲染/完整数据/几何、真实 Preview 焦点及两次精确请求保持验收强度，没有恢复旧同阶段结构或扩大普通生产 API。28 名称/顺序、其他 27 个测试体、四项旧修复和五类原断言均保留；订阅/borrow/drop/API 和普通构建源文核对无新增问题。该结论是静态审查，与下文实际行为结果分开记录。
- 本地实际执行的 `cargo fmt --all -- --check`、`git diff --check`、词法范围/测试体保全、唯一 accessor 与受保护路径字节检查均 exit 0；独审另执行 scope、fixed delta diff-check 与独立 blob/字节/哈希核对，均 exit 0。未本地运行 Vega、编译/build/package、cargo check/Clippy/test/nextest/bench/profile/CUA/OS 自动化。
- 可恢复[固定源码](https://github.com/puzige/vega/blob/4f633e469113fe25c50ffc006dadc4ed0cd1d05c/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs) / [本轮原始 patch](https://github.com/puzige/vega/commit/4f633e469113fe25c50ffc006dadc4ed0cd1d05c.patch)。源码 SHA-256 **`b5d4ca9a458b600d34ba6cac3dfc2bd19da085572f32493f6e440415cec39157`**；ArtifactCard **`0877d19ebd5db4149dc6a3a8a1f05de5add202f839a41179ebae43f3718d150e`**；源码 binary delta **`19db48afcea186637557382f0139b520ca821d06a0558e499b5c1f563537ce45`**。

### 新版本实际 Actions

[Actions 39999223526](https://github.com/puzige/vega/actions/runs/39999223526) 关联的 PR head 为 **`4f633e469113fe25c50ffc006dadc4ed0cd1d05c`**，两个执行 job 实际 checkout 上述 **`c07c27dcc4a62bedad5750851527254abcf3efa8`**，使用原 `pr-check`，没有改 workflow/配置或重跑换绿。2026-10-24 01:56:41（Asia/Shanghai）启动，02:01:46 完成；首次读取在 01:56:54，下一次读取在 02:02:15，遵守五分钟间隔。

| Job / 步骤 | 实际结果 |
|---|---|
| [Nextest 120570652816](https://github.com/puzige/vega/actions/runs/39999223526/job/120570652816) | failure；编译与发现流程完成，行为实际执行，workspace step exit 100 |
| [Clippy 120570653262](https://github.com/puzige/vega/actions/runs/39999223526/job/120570653262) | success；Format 与全目标 `cargo clippy --workspace --all-targets -- -D warnings` success |
| [required 120573011042](https://github.com/puzige/vega/actions/runs/39999223526/job/120573011042) | failure；Nextest 的五个真实失败使汇总失败 |
| Python 三组 | 5 + 15 + 40 = 60 项实际执行，三组均 OK；不是旧报告的 Python 计数 |

云端先成功执行 `cargo nextest list --workspace --message-format json`，JSON 被原 workflow 重定向至 runner 临时文件，未在失败日志中打印独立 `test-count`，因此该 JSON 总数**未取得**。Nextest 原始 banner 是 **`Starting 28 tests across 15 binaries`**；实际 footer 是 **`28 tests run: 23 passed, 5 failed, 0 skipped`**。分别记录为：发现/启动 banner 28，执行 28，通过 23，失败 5，跳过 0；ignored 的独立计数**未取得**，不由静态无 ignore 或跳过数推算。不是 no Rust tests，原日志中出现的该字符串只是 workflow 脚本 echo，不是本轮空集结果。

### artifact 与四项旧修复的实际执行

所有下列用例的完整模块前缀均为 `conversation_stream::thinking::disclosure_motion_tests::`，原始 Nextest PASS 行均已读取：

| 用例完整函数名 | 本轮实际结果 |
|---|---|
| `visible_artifact_preview_and_real_control_focus_survive_first_final_collapse` | PASS（0.425s），本轮合法 artifact 兼容路线已执行 |
| `independent_disclosure_owners_never_share_the_transition_channel` | PASS（0.078s） |
| `long_thinking_stream_interleaves_reversals_and_real_composer_character_input` | PASS（6.052s） |
| `thinking_shimmer_keeps_its_own_active_visible_reduced_and_finished_frame_gates` | PASS（0.064s） |
| `invalid_or_missing_final_and_failed_or_cancelled_turns_keep_unknown_body_visible` | PASS（1.320s），含合法 Failed metadata/真实输出观察 |

这些结果验证本轮实现及其四项旧修复在上述合成 merge 环境中的行为，不倒填为旧 head 或单独 PR head tree 的行为结果；四行渲染和数组正文观察也不冒充独立像素回读、真实文件/Git/权限/外部应用或用户观感验收。

### 五项失败的首个根断言与范围

下面五个完整函数名也均位于上述模块；本轮全部与交接列明的五类已知未决匹配，没有新增失败类型。源行号均指本次实际验证的固定 SHA。

| 完整函数名 | 首个失败与固定源位置 | 分类 / 本轮处理 |
|---|---|---|
| `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion` | `:2363`：`fixture.root_renders() == before_deactivation`，12 vs 11；automatic inactive root render，发生在重新激活之前 | 已知 no-inactive-draw 前提失败；保留严格断言，不改框架/registry/CI |
| `first_final_overrides_independent_manual_focus_and_real_scrolled_reading_states_once` | `:2742`：`!thinking.read(cx).expanded` | 已知子 Thinking 展开位与 Final 展示范围未决；不改 Final 算法或删除断言 |
| `tab_and_shift_tab_visit_only_enabled_real_headers_and_not_the_decorative_glyph` | `:2268`：disabled 分支 `after.is_focused(window)`；`no-content header must be skipped by Tab` | 已知 disabled Tab / 显式 FocusHandle tab-stop 问题；不改生产 focus 或降低 skip 断言 |
| `decorative_frames_do_no_more_message_measurement_than_identical_settled_redraws` | `:2099`：`fold.invalidates.len() == 1`，3 vs 1；`one real fold, not decorative invalidation` | 已知单次 fold 观察/归因问题；不把期望改为 3、不去重或删记录 |
| `first_trusted_final_collapses_reading_selection_and_focus_then_enter_reopens` | `:1064`：`!fixture.selected_text().is_empty()`；真实 source 拖选前置未建立 | 已知真实选择接线前提失败；不伪造 selection 缓存、不删非空断言 |

原始全文日志以 `gh run view 39999223526 --repo puzige/vega --log` 保存，SHA-256 **`52ce7546e181f8f49e823df4c8be1be3d4fdc22add97a6a234e3464948cdb93d`**；原文件完整保留，ANSI 清理仅产生单独分析副本。首次日志提取脚本有分组索引错误，修正观察脚本后严格核对 28 条执行结果、五条目标 PASS、五个根失败及 footer 均通过，原始日志与源码未更改。前轮原始 E0599 日志及其哈希保持不变，没有用最终成功项覆盖首次失败证据。

### 结果回写与后续条件

本节作为独立文档记录提交/推送时会形成新的 PR head，**上述 run 与 PR head `4f633e469113fe25c50ffc006dadc4ed0cd1d05c` 关联，行为实际绑定合成 merge `c07c27dcc4a62bedad5750851527254abcf3efa8`，不绑定单独 PR head tree 或后续文档 head**。收尾 fetch 已安全确认任务远端无后来写入；当时 origin/master 已推进到 `b2248996cc68497ef050539fd69f05cc723e4c0a`（#583），本次结果回写仅修改文档，不再扩集成已停写、已验证的实现范围。文档新 head 与其 own Actions 状态由 Issue/PR 最终回执记录，不冒充最新 head 已通过；本轮不合并。

artifact 兼容范围没有剩余修复或执行环境阻碍，唯一 writer 与独立 reviewer 均已停止。若要处理五类范围外未决，需要另行明确修复范围；涉及 Final 子状态验收需先明确产品/可观察契约，无 inactive draw 需合法 harness 观察路线。后续测试清理、合并及用户手测均是独立阶段，本轮没有这类授权，不因 23/28 或静态审查擅自继续。

### 独审补充：云端 checkout 版本归属纠正

非写入者审查文档 head `09c007fe0cf7d5715852ad70552069e9a28dabed` 时，确认其相对 `4f633e469113fe25c50ffc006dadc4ed0cd1d05c` 只有本记录 +58 行、源码相同、数字/五项 PASS/五根失败与日志一致，同时指出 P2 记录问题：先前“实际行为验证 SHA”没有区分 PR head 与 Actions 合成 merge。父重新读取两个 job 的原始 Checkout 与 `git log -1 %H`，并通过 GitHub commit API 核对实际执行 commit、父节点和 tree，已在本节纠正。原始日志、测试结果及源码均未改变。

[实际合成 merge](https://github.com/puzige/vega/commit/c07c27dcc4a62bedad5750851527254abcf3efa8) / [其 base](https://github.com/puzige/vega/commit/b2248996cc68497ef050539fd69f05cc723e4c0a) 与前述固定 PR head 共同提供恢复条件。合成 tree 中测试源码 blob `bf5ce272fd3456886e10cd53bf9f73dcbc9052f5`、ArtifactCard blob `9458f5cd04056110799e82d5ad3b4a37ed99305c` 已分别远端读回，与固定 PR head 两文件完全一致；五个根失败的源行号也相同。#583 实际已在 CI 的合成 base 中，本地任务实现 commit 并未另行合入它。独立 list JSON 总数和 ignored 独立计数仍未取得。

本次仅纠正文档版本归属，源码未变；修正后文档 head 及其自身 Actions 状态由最终 Issue/PR 回执明确记录，不能把先前合成 tree 的结果充作新 head 的完整 required check。没有新增代码修复、测试重跑或合并授权。

## 2026-10-24 继续推进至 master 的授权与基线

用户最新明确要求“继续推进 直到合并master”，取代上一阶段仅 artifact、不合并/不清理的停止边界。本阶段在 #550 范围内解决五类未决，先固定可观察契约和最小责任层修复，保留回归强度；经专职 writer、非写入者独审与云端开发验证后，留痕并清理开发 Rust 测试及独占支持，对最终 head 执行完整 required CI，成功后 squash merge。#552、无关 PR、用户手测/关闭 Issue 不在此授权内。

- 接手时任务 HEAD/远端/PR 一致为 `08d2f9f403d9888bb2bcd65e35bc43817a71c8a1`，工作树干净，原 writer/reviewer 已停止。fetch 后正常 merge 最新 master `5bc062b245d5f1c7cee3b333df896ef31810fc11`，当前集成基线 `55813aa37fd3f0d02cd230b62ae93073ae11fbe7`；保留 #583、#585 横向代码滚动和 #586 两阶段权限准备，不回退上游。目标树没有 nested AGENTS。
- 上一文档 head 的 [Actions 40001818592](https://github.com/puzige/vega/actions/runs/40001818592) 已完成：Nextest `120579776420` failure、Clippy `120579776698` success、required `120581778233` failure；原始日志再次是实际 28 执行/23 通过/5 失败/0 跳过，五个首断言与前轮相同。实际 checkout 合成 merge `27ffe4bc8d8f5a4ec85db27a1f26af1c226ecfca`，base 为 `b2248996cc68497ef050539fd69f05cc723e4c0a`；该 run 不验证新集成基线。原始日志 SHA-256 `dc10e2007befc0d96422219e70d63a4c4a769d7f0636df76962f780bc01c96c2`。
- 主 Agent 先只读分析并登记具体实施契约；三个非写入诊断切片分别核对生命周期/Tab、测量/真实选择、Final 可观察契约。代码仍只委派一个 writer，停写后独审。所有本地执行限制和五分钟 CI 查询规则不变；不通过删测、改为当前输出或盲重试掩盖未解失败。

### 合并阶段已核责任层与第一实现切片

只读诊断已定位四项：

1. Final 测试错误地要求子 `ThinkingBlock.expanded` 清零，#416 实际要求外层 run 入口关闭；通过既有 `body.painted`、实际 stage/glyph 绘制与焦点/Enter 前后阳性/阴性观察替换该内部位断言，保留 manual/focus/scrolled 全部模式、锚点、原文、重复 Final 和一次性语义。不修改 Final 生产算法。
2. fold 的全量 invalidate 日志包含 run/segment/owned commentary 三个不同条目，原 `measured_work(owner)` 也未给 Invalidate/Remeasure 加 owner；应保留全量原始记录并用动态 run entry index 对被切换条目严格断一次，装饰帧仍检查全量零额外失效及 settled 对照。最小只读记录可补 owner，不能去重或将期望改为 3。
3. 真实拖选的固定 x=36 位于 source participant x>=40 之外，MouseDown 未激活真实 source scope。允许在现有 SelectableMessage 真 prepaint/paint 路径加入仅 cfg(test) 的窄只读观察：copy/source handle 身份、实际已准备的 text run/layout bounds、content mask、实际 paint 到达。fixture 从同一源的真实可见 glyph 内部点 drag；不增加 production public API、不复制布局/选择决策。此临时观察随测试清理。
4. Thinking 真实 FocusHandle 构造时 tab_stop=true，div.tab_stop(false) 不会覆盖显式 track_focus handle。最小生产修复仅 Thinking render 同步 `self.focus = self.focus.clone().tab_stop(can_expand)`，identity 不变；工具已有正确门控，不改其他 focus 算法。

第一专职 writer 只实施上述四项、其必要同文件 fixture/只读接线和 Thinking 单点门控。其余测试体、artifact 及四项前轮修复保持。D05 no-inactive-draw 仍由非写入诊断核查，路线明确后再交同一个 writer；父仅维护 docs/Git/CI，不并行写源码。后续固定停写版本必须经非写入者独立审查和现有 Actions，全部有效行为验证完成后才清理开发测试，不以空集消解真实未决。

补充静态 API 核对：`RunActivityGroup::render_segment` 的 stage handle 同样由显式 `track_focus` 接管，构造时的 tab-stop 也须随 `can_expand` 同步；与 Thinking 使用同一最小门控，保持 handle identity。工具卡和过程总入口已有正确门控，不修改。Thinking 的私有 body 字段不跨兄弟模块公开；允许增加唯一 `#[cfg(test)] pub(super) source_painted() -> bool`，直接返回既有 `body.painted.get()`，与实际 source SelectionPaint/glyph 的首次 Final 前阳性、后阴性配对，随测试删除。上述为同一四项修复的窄接点，不改 Final 或正文布局决策。

D05 只读诊断已确认：测试平台的每次失活/激活回调都经 `handle.update` 强制 `window.refresh`，test-support 的 effect flush 自动绘制 dirty window，因此原断言在重新激活前已经因额外一次 root render 失败。跳过 inactive paint 会丢失需逐帧保留的 keyed channel/sampler state，无法证明同一 channel 的无重播；持有 App borrow 阻止回调也不是合法测试路线。当前未找到合法公开的暂停自动绘制 API，未改框架/registry/CI，未删改严格测试。主 Agent 已向用户提交验证处置选择，尚待答复；其他四项及完整生产独审继续。静态代码审查不能冒充这一动态前置已证，测试清理不能消解此未决。

### 固定基线的完整生产独审与 D05 诊断证据

非写入者已对固定 `55813aa37fd3f0d02cd230b62ae93073ae11fbe7` 相对集成 master `5bc062b245d5f1c7cee3b333df896ef31810fc11` 完成生产增量审查，未发现可证的新增 P0–P3 产品缺陷：同一 sampled angle 只进入真实 Svg 绘制变换，layout/prepaint/AX 委托保留；五类 owner/key 稳定，锁定 Kit 的首次挂载/反转/零时长逻辑与共享 activation observer 对齐；Final、选择、焦点、失效和产物生产算法未改。结论不覆盖尚未停写的四项修复，不代表动态验证完成或可合并。固定 crates diff-check exit 0；全量历史 delta 的唯一 EOF 空白已经本阶段文档批次修复，当前全量 diff-check exit 0。

D05 依赖事实固定为 gpui-kit/gpui-base `0.6.1`、gpui-pre `0.3.4`：`platform/test/platform.rs::set_active_window:284–303` 依次异步发送 false/true，`window.rs:1862–1880` 每个通知均单独 update、通知 observers 并 refresh，`app.rs::finish_update:1078–1095` / `flush_effects:1712–1723` 在 test-support 中自动 draw。原严格断言的 `12 != 11` 出现在 blocker 已激活、目标窗重新激活之前，恢复端点/续帧断言未执行。

`window.rs::use_keyed_state:3930–3948` / `WindowFrame::finish:1124–1133` 只保留连续访问的 element states；`skip_drawing` 仍 finish/交换/清理 frame。跳过 glyph paint 或 draw_roots 会使 paint-only lifecycle/sampler 首次重建为 target，不能充作同一 channel 的无重播证明；内部 replay/PaintIndex 非公开。`HeadlessAppContext::update:149–152` 只持有 RefCell borrow，异步 callback 的 `try_borrow_mut` 会失败，不是合法批处理路线。生产 observer 收到 inactive 后保留 `interrupted=true`，下一采样 `mem::take` 后传零时长；Kit `motion.rs:286–302` 静态上直达 target 且不请求帧。这解释未见对应生产根因，但仍未动态证明无 inactive draw 场景。

### 四项修复停写与父静态核对

专职 writer 已 READY 并停止七文件写入：实际 source SelectionPaint 在真实 body.paint 返回后记录，同源可见 run/layout 供真实 drag 使用；owner/index 原始测量日志保留，真实 run 条目严格一次失效及配对重测；Final 前后实际正文/控件 paint、隐藏焦点、原文与真实 Enter 重开/重关配对；Thinking 和 Stage 同一 FocusId 的 tab-stop 随真实 can_expand 同步。原 Stage enabled 回归之后，还用真实 summary 令相同 stage 禁用，确认真实 title/header 仍渲染、raw source 保全、Tab/Shift-Tab 跳过且同一绑定持焦时 Enter/Space 不展开。

父读回七文件 SHA-256 均与 writer manifest 精确相同；writer 的原始 unified patch SHA-256 `ed24040286b3fa067a6647066a0292b8c9f7673922f0562adfeb5a22511c7256`，父独立生成的 git binary diff（含 git headers/函数上下文，格式不同）SHA-256 `aa79b827c07a6b8f90b0015e1668d82486d2890924027764fc37c51df6e91570`，未将两种 patch 格式误记为逐字节相同。测试源码 SHA-256 `51046452ffafb38327b550cfe7e696262f5a91631a87cb4caa800002c6d5993a`。父实际词法范围检查 exit 0：28 名称/顺序保持，仅四授权测试体改变，其余24体逐字节一致；静态 assert 宏计数237→292，不是发现或执行计数。artifact、前轮四项修复、D05原体、motion算法、tool_card、依赖与CI配置保全。

父实际 `cargo fmt --all -- --check`、相对集成 master 的完整 `git diff --check` 及静态范围/受保护路径检查均 exit 0；fetch 后 master 仍为 `5bc062b245d5f1c7cee3b333df896ef31810fc11`，远端任务分支无并发写入。未本地运行/编译/构建/check/Clippy/test/nextest/bench/profile/CUA；普通构建生产改动仅上述两处门控，临时观察均随开发测试清理。当前尚待固定提交的非写入者增量审查和 Actions 编译/行为结果，不能称四项已动态通过；D05选择仍待用户答复。

## 2026-10-24 四项修复的独审与首轮云端结果

固定完整提交 **`f54f803927ccaa27720d74c740c879b2e780fc02`** 已正常推送；[测试源码](https://github.com/puzige/vega/blob/f54f803927ccaa27720d74c740c879b2e780fc02/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)与[完整提交 patch](https://github.com/puzige/vega/commit/f54f803927ccaa27720d74c740c879b2e780fc02.patch)均已远端读取，抓取的提交 patch SHA-256 `84e500e3f894c2c791a7c147022de4bf531ca1251477ad9a60d50099c9bdf0d0`（包含 docs，区别于前述七源码 delta）。非写入者已完成此完整 SHA 的增量审查：未发现可证新增生产问题或验收弱化，真实选择/全量测量/Final配对/同Id门控与所定契约相符；28名称/24体保全、七文件哈希、delta/full-base diff-check 均核对通过。该静态结论不代替行为结果，D05仍未决。

[Actions 40069105826](https://github.com/puzige/vega/actions/runs/40069105826) 关联上述 PR head，2026-10-24 14:09:56（Asia/Shanghai）启动、14:14:35 完成；父14:10:09首次读取、14:15:55再次读取，未短轮询。Nextest 与 Clippy 原始 Checkout 均为合成 merge **`72de7af04237364556cec1b5d5e75125cebd0292`**，已通过 commit API 核对父为 base **`5bc062b245d5f1c7cee3b333df896ef31810fc11`** 与 head **`f54f803927ccaa27720d74c740c879b2e780fc02`**；tree **`0ef881104ebc352f44f970331620e34175732f15`** 与本地固定 head tree 精确相同。实际执行仍归属该合成 checkout，不称 PR 已合并。

| Job / 实际命令 | 结果 |
|---|---|
| Nextest `120798494308`，原 `cargo nextest list --workspace --message-format json` / `cargo nextest run --workspace` | failure，workspace step exit100；banner `Starting 28 tests across 15 binaries`，实际 **28执行/25通过/3失败/0跳过** |
| Clippy `120798494171`，Format 与 `cargo clippy --workspace --all-targets -- -D warnings` | success |
| required `120799412273` | failure |
| 原 Python 三组 | 5+15+40=60项，均OK |

独立 list JSON 总数仍未在日志打印、未取得；aggregate ignored 计数未取得，不由个别二进制的过滤/忽略统计推算。真实拖选 `first_trusted_final_collapses_reading_selection_and_focus_then_enter_reopens` **PASS（0.459s）**；键盘 `tab_and_shift_tab_visit_only_enabled_real_headers_and_not_the_decorative_glyph` **PASS（0.238s）**，含真实 disabled Stage。artifact及前轮四项修复仍均PASS，未回退。

以下函数均带完整模块前缀 `conversation_stream::thinking::disclosure_motion_tests::`，源码位置绑定本轮固定 SHA：

| 函数 / 首个失败 | 已知与下一责任层 |
|---|---|
| `decorative_frames_do_no_more_message_measurement_than_identical_settled_redraws:2333`：moving 每个真实 owner/index Layout 为2，settled为1 | 新增的动态 run 一次失效/重测及原始归属/配对均已通过；装饰失效/显式重测均零。需核对 simulate_next_frame 的自动 draw 与显式 draw 是否造成输入边界不等，不将期望改2、去重或删除记录 |
| `first_final_overrides_independent_manual_focus_and_real_scrolled_reading_states_once:2993`：`first Final stops painting the previously visible source` | 外层关闭、process_in_details、当前 source_painted=false、原文/Final断言已过；需核对累积 SelectionPaint 是否混入 append_final 的合法前置阶段，尚不判定隐藏正文生产失败 |
| `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion:2595`：root render12 vs11 | 原无inactive draw前提仍失败，恢复断言未执行；用户处置仍待答复 |

本轮原始全文日志 SHA-256 **`3797bccea25cbb5b7a1aee802eb8658c4f3e22d69f83bc563017e020ed05168e`**，保留 raw 与单独清理副本，逐条解析核对28项与footer。首轮结果不是27/28，也不能据静态独审称四项全部通过。主 Agent 已发起上述两项后续责任层诊断；继续相同唯一 writer，不改生产算法/通用CI，不清理Rust测试以掩盖失败。

### 用户已明确接受 D05 动态验证边界

2026-10-24 14:21（Asia/Shanghai）前，用户在本会话的验证处置问题中明确选择：**“保留生产要求，明确记录 D05 未获动态证明，经其余修复/独审/CI 后合并（推荐）”。** 该答复解除等待用户选择的阻碍，取代本阶段前文的“尚待答复”；D05 产品要求不变，严格无 inactive draw 的同 channel 动态证明仍未取得，不改写为通过，也不据此修改框架、忽略或放宽原测试。

下一轮行为验证仍执行全部28项并保留D05原体/实际结果，其余27项的未解失败继续处理。取得其余必要行为证据、独审后，按仓库政策统一留痕并删除整套开发Rust测试/独占支持；最终无Rust测试head的独审和完整required CI成功后可合并。最终空集和D05获用户接受的动态验证缺口必须分别披露，不能将清理/空集CI冒充D05或整体行为全通过。Issue仍等master用户手测，不关闭/Done。

### 后续 Final 观察窗口诊断

`final_event` 经 `append_final` 先独立 update 发送 Started/TextDelta，再另一次 update 发送可信 `AssistantResponseCompleted(Final)`；Headless 外层 update只借用App，每次 entity update都可能经finish_update/flush自动draw。当前在整组事件之前清SelectionPaint、之后检查“从未画source”，错误地包含了合法可信Final前的source绘制。`source_painted=false`与累积旧记录并不矛盾，但当前日志不足以证明所有可信Final后的自动帧正确。

最小修正限定该测试：先真实stage Final text，随后在仅Completed事件之前划清pre-trust边界；发送Completed后，先原样检查全部自动SelectionPaint/glyph阴性，再显式绘制并独立检查当前最终帧与真实run header阳性。不得在Completed之后清空自动记录求绿、只取末帧或等待直到消失。Enter重关也在操作之前清边界，分别检查其后自动和显式帧。manual/focus/scrolled、source阳性/全文、scope/copy/handle、锚点/Composer/真实焦点/Enter重开重关和重复Final一次性全部保留，不改Final生产算法。

### 后续 D06 真实绘制边界诊断与第二实现切片

锁定 `simulate_next_frame` 执行既有animation callback，`cx.notify`使窗口dirty，update退出时test-support自动draw；后续 `fixture.draw` 再refresh并draw一次。settled无运行callback时前一步不dirty，只画后一次。该调用链解释本轮2vs1，但日志未带逐draw标签，不能声称已取得独立逐draw trace。`HeadlessAppContext::advance_clock` 还经TestScheduler::advance_clock运行任务，不能将它误认纯时间加法。

同一公开TestClock路线已核对：BackgroundExecutor.dispatcher → as_test → scheduler → clock → advance仅更新真实now/utc，不执行任务；Kit sampler的BackgroundExecutor.now经相同scheduler读取同一clock。D06专用同文件helper采用纯clock前进15ms且断期间无root/glyph/layout工作，再统一同一次update simulate_next_frame+refresh，退出交原flush自动draw；moving/settled复用相同helper，不额外显式draw、不按观测次数补齐。增加同文件Rc<Cell<usize>>只读root render计数，配真实glyph和全量item观察，严格一个受控刷新样本，保留全量记录精确比较、八帧角度/几何与零额外失效。

此fixture仅MessageStarted等、未发RunStarted，group.started_at=None且schedule_clock直接返回，静态上未排group clock task。原shimmer关闭与异步停泊保留，比较前增加同源真实SelectionPaint准备/paint阳性；不取消任务或修改生产时钟/正文。第二writer切片只改这两个测试体及必要同文件fixture/helper，其他26体/28名称顺序、D05原体与七文件中其他源码保全。主Agent仅维护docs/Git/CI；待固定停写版本再独审和原Actions。

第二切片执行中会话发生中断。恢复时已核对旧子Agent均不再运行、没有第二轮READY报告；当前只有测试文件部分D06 helper/counter改动与父两份文档未提交，Final第二轮修改尚未完成。主Agent保留当前diff检查点，不reset或覆盖；fetch读回head/远端仍为`f54f803927ccaa27720d74c740c879b2e780fc02`、master仍`5bc062b245d5f1c7cee3b333df896ef31810fc11`，无并发写入。新专职writer接续同一部分产物、原诊断与授权，不重做已完成修复；仍需完整静态核对、READY停写、固定版本独审和Actions。

### 第二切片 READY 与父静态核对

新唯一writer已完成并停写。源码SHA-256 **`963847a732cef649d1e776c4db40c2ba8e82d92ecca878182a8baefd0ec40f7d`**，仅该源码相对f54f8039…的原始git binary patch SHA-256 **`89ac6e4a7002a847c59663b8fe1f598e67f017b63addc3b825a62ee1ab48bb16`**；父独立读取文件与patch逐字节核对匹配。D06每步真实纯clock/统一flush/root+1/glyph一次/全量item阳性及完整原始精确对照保留；Final在仅Completed之后检查自动和显式两批，Enter重关也同样检查全部两批，pre-trust仍须保持正文和过程可见。

父实际静态范围检查exit0：28名称/顺序相同，仅D06与Final两体改变，其他26体逐字节一致；静态assert宏292→305，非执行数。其他源码/依赖/通用CI完全不变，严格D05原体保留。父实际cargo fmt与完整diff-check均exit0；fetch后master/远端无新写入。尚未本地运行/编译/测试Vega；新固定版本的非写入者独审与Actions编译/行为结果待取得，不能称其余27项已过。

## 2026-10-24 第二切片云端结果与 Final 前置修正

固定完整head **`2527b421ff0fe928ab8e071637c16b02f7de30fe`** 已推送；[源码](https://github.com/puzige/vega/blob/2527b421ff0fe928ab8e071637c16b02f7de30fe/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)已远端完整读回，blob `6ac178ea8eeb60469eb4d094917a5baa84188a49`、SHA-256 `963847a732cef649d1e776c4db40c2ba8e82d92ecca878182a8baefd0ec40f7d` 与本地一致；[完整提交patch](https://github.com/puzige/vega/commit/2527b421ff0fe928ab8e071637c16b02f7de30fe.patch)抓取SHA-256 `76bc073abae52cd22fd7bb6f22d614316da45c4f9f2f4627429f331f3ad65843`。非写入者已独立核对28名称/顺序、26体与严格D05保全、同源clock/正常flush及Completed后全部批次，静态检查均0；其静态未发现问题不代替下列真实失败，新增前置问题已反馈独审者复核。

[Actions40075910590](https://github.com/puzige/vega/actions/runs/40075910590) 2026-10-24 15:53:29（Asia/Shanghai）启动、15:58:05完成；父15:53:53首次读取，15:58:54之后再次读取，遵守五分钟间隔。Nextest与Clippy实际Checkout **`58d6c5aad65a3aa5857a4c84b6d3b3e89c90f5da`**，API核对父为base **`5bc062b245d5f1c7cee3b333df896ef31810fc11`** 和head **`2527b421ff0fe928ab8e071637c16b02f7de30fe`**；tree **`797f1e0a5a631554bbbea5a2e46650ff29435642`** 与固定head完全相同。

- Nextest `120819081902` failure/exit100：banner28 across15 binaries，实际 **28执行/26通过/2失败/0跳过**；独立list JSON与aggregate ignored仍未取得。
- Clippy `120819082020` success，Format success；Python5+15+40=60项均OK；required `120819923114` failure。
- D06 `decorative_frames_do_no_more_message_measurement_than_identical_settled_redraws` **PASS（0.578s）**，统一边界后原始全量精确比较、八样本角度/几何、零额外失效与真实run一次失效全部执行。
- Final `first_final_overrides_independent_manual_focus_and_real_scrolled_reading_states_once:3085` 在新增pre-trust `group.expanded()` 阳性失败，reading=manual；日志同批实际stage expanded=true/enabled=true、外层run expanded=false/enabled=false、source_paints=6。尚未执行Completed后覆盖，不称Final三模式通过。
- D05 `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion:2699` 仍原前提12vs11；保持用户已接受的动态边界，不改产品要求或结果。其余26项，包括拖选/Tab/artifact/前轮修复，均PASS。

原始全文日志SHA-256 **`b27544fb172c606364b6ba56c94e82d0d6db741e187f3c6af4492b8e90fff540`**，保留raw/单独清理副本，逐条核对28项及footer。不是27/28，也未因D05已获处置而清理另一项未解失败。

本次新增pre-trust前置再次误用了外层局部偏好位：可信Final之前正文按live阶段独立渲染，外层run尚未process_in_details，其不可展开入口的expanded并不表示live正文是否可见。第三窄切片仅替换这一新增错误前置：要求!process_in_details，并用同source copy/handle、真实可见run/mask及同stage enabled/expanded glyph加source_painted做阳性；不要求外层偏好true、不强制reveal、不改生产算法。Completed后全部自动/显式阴性、三mode/focus/anchor/Enter/once/全文不动；D06及其余27体逐字节保全，再交独审和原Actions。

第三切片writer已READY停写，非写入者另结合原日志独立确认该P2前置错误并纠正先前纯静态遗漏。父核对最终源码SHA-256 **`d5fe9c4b0799e19480041fdaba394e489d71fe6e3ddb5d89a81d3c81606f100e`**、仅源码git binary patch SHA-256 **`5ec3164c1bf1644fc4ca02157e9d254c599562aa0c421febe8fe3ca33228b09c`** 与停写产物精确一致。真实source同copy/handle/scope/content、可见prepared run/shaped glyph、同stage真实paint阳性取代错误内部位，source全文和source_painted保留。

父实际词法检查28名称/顺序、其他27体逐字节保全、仅pre-trust block与两份docs变化，exit0；cargo fmt与full-base diff-check均0。Completed/Enter后全部检查、已PASS的D06、D05严格原体及生产代码均不变，未本地运行/编译/测试Vega。新固定版本的窄增量独审与Actions仍待取得，不先放行删测。

## 2026-10-24 pre-trust 修复独审与实际滚动锚点失败

固定完整head **`431ec02bbc840354bda15b83a4331915271fd17e`** 已推送。远端[源码](https://github.com/puzige/vega/blob/431ec02bbc840354bda15b83a4331915271fd17e/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)完整读取与本地SHA-256 `d5fe9c4b0799e19480041fdaba394e489d71fe6e3ddb5d89a81d3c81606f100e`一致，blob `df75fd2e7c06bba0918bc48357c99736ebd1e27a`；[完整提交patch](https://github.com/puzige/vega/commit/431ec02bbc840354bda15b83a4331915271fd17e.patch)抓取SHA-256 `09935be54a2acdc4c75f355208c9e6e7b5312b127031f57aab6acc26b0f4c114`。非写入者完成窄增量独审，确认pre-trust P2已修复、实际API/真实记录与28名称/27体及块前后保全，无新增静态发现；该结论不替代后续行为和滚动诊断。

[Actions40076729448](https://github.com/puzige/vega/actions/runs/40076729448) 2026-10-24 16:06:16（Asia/Shanghai）启动、16:10:08完成；父16:07:02首次读取、16:12:36再次读取，遵守五分钟间隔。Nextest/Clippy实际checkout **`ef30eca9d82ea49cdd86be50b615f7cfc2a51477`**，API核对父为base **`5bc062b245d5f1c7cee3b333df896ef31810fc11`** 与head **`431ec02bbc840354bda15b83a4331915271fd17e`**；tree **`658098c947dc4500cb0323cd2fd4b5b66cd2b3ab`** 与固定head完全相同。

- Nextest `120821558133` failure/exit100：banner28 across15 binaries，实际 **28执行/26通过/2失败/0跳过**；独立list JSON与aggregate ignored计数仍未取得。
- Clippy `120821558370`、Format success；Python5+15+40=60项均OK；required `120822308670` failure。
- D06全项继续PASS（0.393s）；source真实选择、Tab、artifact及前轮四修复仍PASS。
- Final用例已通过manual/focus两个完整分支，scrolled分支的pre-trust阳性和Completed后隐藏检查也走过；首次失败推进到固定`:3210`，`reading=scrolled first Final preserves reading anchor`。Final后为history-6/run-activity、offset20.5、following_tail=true；Final前为history-8/run-activity、offset14.5、following_tail=false。该函数整体仍FAIL，不把分支执行拆成独立PASS计数。
- D05仍在`:2699`原严格无inactive draw前提12vs11失败，恢复断言未执行；继续按用户接受的动态验证边界留痕。

原始全文日志SHA-256 **`d23f7dab313ae4306e2ccbb1702f25f83924ed75ad915d4629a5aa581278bcfe`**，保留raw/单独清理副本、逐条28项与footer。不是27/28；没有因D05获处置而跳过滚动失败。

主Agent将新锚点失败交非写入者只读诊断，核对#416/#344/#545与锁定List回填/paused Tail恢复路径。现日志没有viewport/total/offset等几何，尚不认定是合法clamp或生产恢复缺陷。保留精确identity/offset/following_tail与真实wheel/source阳性断言；不任意放大72、不改expected、不清理测试，不动生产List/anchor算法。只读清理计划虽已列齐4删除/13恢复/6生产保留，仍等待滚动失败解决及其余27项必要行为证据。

### 锚点诊断观察切片（不改验收或输入）

非写入者确认锁定List在旧top到tail不足视口时回填，并在距离底部1px内恢复paused Tail；当前数字与之相容，但未取得几何不能认定因果。下一切片仅在原Final测试及同文件helper增加四阶段只读快照：A真实wheel/source准备后、stage text前；B stage text后且Completed前；C Completed自动批次已收集后、显式draw前；D显式draw后原anchor断言前。记录当前/旧anchor index与identity、offset/follow、viewport、foreground padding、逐项identity及Option<真实bounds>、辅助scrollbar offset/max（明确max不含padding，不独作判底），以及同source真实run/glyph/mask位置。不得draw/notify/park/改scroll或丢弃任何paint批次。

原16历史、640×1200窗口、wheel72、短Final、原精确anchor/follow断言及全部Completed/Enter后覆盖保持，不改变expected或添加放宽/ignore；其他27体/D05原文保全。这是新增责任层观测，不是盲重试或行为修复。若旧identity唯一且offset合法、suffix全部连续已测，按 `sum(h_i) - old_offset + foreground - viewport_height` 判断旧top的真实余量；None不当0，0..1px不能证明非尾随可保留。取得数据后再确定仅fixture修正或真实生产责任层；长Final候选也须先证明实际可达与有效阅读区source阳性，不能直接按行数换绿。

观察writer已READY停写，父读回源码SHA-256 **`e0d4a7aeb974e5b75e1ef3e993b7d4118786708ef8df30bad9ea21088a041b33`**、仅源码git binary patch SHA-256 **`428919c96d145351a507e671b3575efe97d8f743fec0679b5521ca993b914c5e`** 与停写产物精确一致。父重跑静态范围脚本exit0：28名称/27体保全，移除只读helper/5个let与还原唯一错误消息后，整个源码与431基线逐字节相同，静态assert宏308不变；证明原输入/断言表达式/批次时序未改。父cargo fmt、完整diff-check均0，未本地运行/编译/测试Vega。新head的独审和原Actions待取得，不把本切片称行为修复完成。

观察切片固定为`13379172d763399153f0a63d0e52a690f9273315`。停写后fetch发现master新增`a100f7307c73e51fefcc56012c117b7b832e292e`（#587，CUA面板chrome/拖动跟随）；规范、依赖、#550测试/生产文件均无交叉。主Agent正常merge该新基线，不回退上游；测试源码字节与观察切片相同。后续实际Actions按新完整head及其synthetic checkout记录，不能把旧head结果冒充新集成验证。清理计划与最终静态审查基线同步改为a100f730…，执行前重新核对13个拟恢复blob与保护路径，保留上游CUA源文。

## 2026-10-24 四阶段实测：原滚动前置不可达

固定集成head `f9dd2aa8edb30ac2b9e0fbd51fa2e084496ad6f5` 的[Actions 40078719933](https://github.com/puzige/vega/actions/runs/40078719933) 已完成；Nextest `120827645358` failure、Clippy `120827645530` success、required `120828276959` failure。两执行job实际checkout `daf06648e537a7e7a4274c70dfce523e81695a15`，API核对父为a100基线与f9dd完整head，tree `4493b12e2bd1c9c91d949efb8e0625df3c18df1d` 与head相同。实际 **28执行/26通过/2失败/0跳过**，独立list JSON与aggregate ignored仍未取得。Final原锚点断言在`:3318`失败；D05仍在原`:2699`前提12vs11失败，其余26项PASS。原始全文1,025,872 bytes，SHA-256 `382511e3414e60f355716469fcca6871b8b7c017c31930692d987331fcc5eb22`；保留完整raw与单独ANSI清理副本。

唯一writer停写后，非写入者确认观察增量只读，移除helper/五个let及还原错误消息后与431源码逐字节相同；28名称、其余27体、原输入/断言表达式/时序均保全。主Agent和独审者再各自从原日志重算四阶段，均无未知高度：suffix连续、全部Some、隐藏零高保留，宽576、viewport高1200、foreground padding170均不变，有效阅读底边1030。

| 阶段 | 原history-8/index32至tail的实测高度和 | 减原offset14.5、加padding170后的剩余高度 | 超出viewport的余量 |
|---|---:|---:|---:|
| A wheel/source后、Final文字前 | 1116.5 | 1272 | 72 |
| B Final文字后、Completed前 | 1155.5 | 1311 | 111 |
| C Completed自动帧后 | 859.5 | 1015 | -185 |
| D 显式draw后 | 859.5 | 1015 | -185 |

A/B原history-8 identity唯一、offset14.5、following=false不变；C开始才移动。stage285与owned commentary39收为0，独立Final39增至67，净减少296；111−296=−185。真实scrollbar offset由−778.5变为−593.5，回填恰好185；tail bottom1030恰好有效阅读底边。锁定List原生不足高度回填与1px内恢复Tail有完整物理依据，本轮没有证据表明#550新增锚点缺陷。A/B同source copy73/handle93v3的真实首glyph line位于y823–846，有效阅读区内，原source可读前提确实成立。

下一窄切片只修正scrolled合成输入：使用合法较长Final，在Completed前测量真实保留Final和连续suffix，以仅实际将隐藏的stage/owned commentary作为收缩预算，证明原历史锚点仍可达且距native 1px门限有余量；同时要求同source真实glyph在有效阅读区内、原锚点/非尾随未变。manual/focus继续原短Final，原Completed后的自动/显式隐藏、完整Final/source字节、锚点identity/offset/following、焦点、Enter往返与once断言保留；不改List/Final/锚点生产算法、D05或其余27测试。先修spec再由唯一writer实施，实际行为结果仍待新的完整Actions，不提前删测。

### 可达前置切片 READY（行为待 CI）

同一唯一writer已停写，只改目标测试体，所有helper/其余27体与28名称顺序保全。scrolled仅使用16段合法Final输入；首次与重复Completed end、两次完整Final期望均精确绑定实际输入，manual/focus仍原短Final helper/时序。原wheel72、16历史、四阶段、精确anchor相等和全部Completed/Enter自动/显式批次保留。新增实测前置要求唯一合法历史anchor、连续suffix全Some、仅同group stage与owned commentary两项隐藏预算、保留内容lower_slack>3px；同source真实字符内点落入有效阅读clip，真实pending Final同copy/scope且prepared glyph已paint。Completed后再核同Final identity/copy/scope/精确全文及实测高度不减，验证原body仅追加footer的下界。

最终测试源码SHA-256 `a46118ae29918d78eafe7085ccf43921ec1a0074ad943f59db26e1a969f6f147`；相对f9dd的源码git binary patch SHA-256 `01a72d29f1b6af60a3a198a8bf18c76b6da35c50a7f1e6808e40edf12f2d3845`。父实际词法核对28名称/27体、仅目标测试+两份docs变化，assert宏308→336；fmt、head/master diff-check全部exit0。原35个目标assert调用顺序与表达式保留，两处全文期望仅随合法输入变更；没有新增draw/park、接点、生产、依赖或CI改动。上一run的Python三组实际5+15+40=60项成功；本新版本仍待非写入者固定head独审与现有Actions，不称27项已通过，也未开始清理。

可达前置切片固定为 `5af009694d097a2645db4655e807ebcdf0120ef1`。提交前fetch发现master新增 `1ced1f10985a3f197cd91e72393e42cc001041e1`（#590，仅docs/588与docs/589两份规格），与本卡源码/规则/依赖均无交叉；父正常merge并完整保留两份上游文档。后续独审/Actions绑定新的完整集成head，源码内容hash不变。清理与最终静态审查基线同步改为1ced1f10…，不得借恢复旧测试支持回退新上游。

非写入者对完整head `4aea153242897c855bf5e54300a4104270929913` 的静态审查发现新增pre-trust预算P2：`process_copies`只有已记录可信Final时经`confirm_final`赋值，当前Started/TextDelta阶段仍为空，因此用`owns_process_copy`筛commentary会只预算到stage，len2前置无法成立。父核对phase/run_activity生产调用链后确认；这不是生产缺陷，不把必要预算改成1。下一最小修正仅改目标测试：可信前按同current稳定identity、真实已完成Commentary输入范围与精确正文唯一定位copy，Completed后再正验同copy的真实group归属、隐藏与0高；不新增hook或提前confirmFinal。其余27体和原严格行为断言保留，修spec后交同writer，未清理测试。

[Actions40081100194](https://github.com/puzige/vega/actions/runs/40081100194) 对完整4aea的实际结果与该静态P2一致：Nextest `120834659216` failure，Final`:3314`预算len1vs2；D05`:2699`仍12vs11。实际 **28执行/26通过/2失败/0跳过**，Clippy `120834659424`与Format、Python5+15+40=60项成功，required `120835375755`失败；独立list JSON/aggregate ignored未取得。两job实际checkout `199bff6e91be8e189dd4a9112f2d2ec861ab5f32`，API读回父为1ced基线+4aea、tree `2daabe237610fad0d344cb099999674bab7830bc`与该head相同。原始全文195,211 bytes，SHA-256 `419da466233e9eacb0f934975205504dbaca452b912bfa372c7d30b8db4c3848`。父首次09:13:53 UTC、再次09:18:58 UTC读取，间隔超过5分钟，失败后读取原日志；不是盲重跑，也未将尚未到达的新增几何/Completed后验记为通过。

预算时序修复writer已READY停写：只在原Final体中通过current两条真实Assistant顺序、精确原文/累计字节范围、实际active response边界定位commentary，保存其稳定identity/copy；Completed后核真实owns、隐藏/0高，stage同group/details与0高，Final不owned。原两项预算、lower_slack>3px、精确anchor、全部批次及其他27体保全。父实际fmt、head/master diff-check和词法范围检查exit0；源码SHA-256 `bfbed9c3e3ae284ba349497a5c06a9ae767ad927ae0768a931da16ea870417f5`，相对4aea源码binary patch SHA-256 `59a6562d4a7c6c36bb84e57d9faadc8cf78c5956bf7d2accd900349804f1cee8`。assert宏336→357，原63调用顺序保留；这是静态证据，新固定head独审与实际云端行为待取得，尚未清理。

## 2026-10-24 必要行为验证收敛与统一清理授权

固定开发head **`6be3e57e3461dd768003c2b5072e375a5e1249d6`** 已由非写入者完成增量独审，预算时序P2关闭、无新增发现，其他未改生产结论复用。父/独审分别核对28名称、其他27体与helper字节、实际输入/metadata范围、全部paint批次和原精确anchor保全；本地仅fmt/diff/静态检查，均0。

[Actions40081720206](https://github.com/puzige/vega/actions/runs/40081720206) 对该head实际 **28执行/27通过/1失败/0跳过**；父逐条核全部28名与footer，唯一FAIL为D05 `rapid_reactivation_without_an_inactive_draw_does_not_replay_missed_motion:2699` 原WIRING前提12vs11。Final三模式、真实滚动可达/锚点、Completed/Enter全过程、D06、Tab、拖选、artifact与其余26项均实际PASS。Nextest `120836476023` failure、Clippy `120836476261`及Format success、Python5+15+40=60项成功，required `120837183892`仍failure；绝不称28项全通过。独立list JSON/aggregate ignored仍未取得。父09:23:10 UTC首次、09:28:30 UTC再次读取，遵守5分钟间隔。

两job实际checkout **`97b7bad4cc3221d0957fc8125d253bc521dc50b3`**，API核父为1ced基线+6be开发head，tree **`44264648e428b45a993ede84ae144c62b3589234`** 与head相同。原始全文193,447 bytes，SHA-256 **`2bcce18df40e67becbc747b028fb45f158c35b665512367cb364e2910fce96e8`**，保留raw/独立清理副本。D05按用户既有裁决继续保留生产目标并明确未获动态证明；本轮无其他未解决行为失败。

清理前已从远端实际读取并逐字节核对[完整4001行开发测试源码](https://github.com/puzige/vega/blob/6be3e57e3461dd768003c2b5072e375a5e1249d6/crates/vega_ui/src/conversation_stream/disclosure_motion_tests.rs)，blob `0cd333f9991ce7752e683c48056e6c50f35ce8a8`、SHA-256 `bfbed9c3e3ae284ba349497a5c06a9ae767ad927ae0768a931da16ea870417f5`；[本次修复完整commit patch](https://github.com/puzige/vega/commit/6be3e57e3461dd768003c2b5072e375a5e1249d6.patch) SHA-256 `99a4c2f24bca0e3068d5769e6344c5efba4215ad50b0e79bdcb30710483d5086`；[相对当时master的完整开发diff](https://github.com/puzige/vega/compare/1ced1f10985a3f197cd91e72393e42cc001041e1...6be3e57e3461dd768003c2b5072e375a5e1249d6.diff) 经API原始diff实际读回343,988 bytes，SHA-256 `c77c59dc09c8878ccbcbeb0093e54476fe9a1a71c2b4b89514627dc7759b9487`。历史源码/补丁保留D05严格失败与全部原始断言，统一删测不销毁验证留痕。

父清理前fetch发现master仅新增 **`8861240f863927a695fa401db98760a883b9cb14`**（#592，一份docs/591规格），无源码、依赖或规则变化，正常merge并完整保留。清理固定基线改为8861240f…；6be的27项行为证据只归属6be与上述synthetic，不冒充新head门禁。最新集成head源文件字节仍与6be相同。

现向同一唯一writer授权统一删除整套28项开发Rust测试及独占支持：删除4个新增测试/支持文件，13个仅测试接线文件恢复最新master精确blob，3个混合文件仅去测试部分，保留共享decorative生命周期/120ms motion与两处tab-stop门控。不得挑删D05、保留helper、忽略失败或改生产求绿；Python/fixtures、benchmark、依赖生产版本、所有workflow/Nextest均保全。预计最终相对master只剩6个生产源码delta和5份既有本卡文档；实际清点、非写入者完整最终head独审及完整required CI尚待完成，必须明确 **no Rust tests（0发现/0执行）** 后才能合并。Issue保持开放，合并后In review等用户master手测。
