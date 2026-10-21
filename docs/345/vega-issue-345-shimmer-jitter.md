# #345 — Shimmer 文字几何稳定

## 1. 范围与基线

- Issue：[字体抖动](https://github.com/puzige/vega/issues/345)。本文恢复历史评论所引用、但当前仓库与本地全 ref 历史均未找到的同名规格；不声称旧规格已交付。
- 方案 v3，2026-10-21（生产未改、尚无有效测试补丁时，改为复用 GPUI 自带跨帧布局缓存）；最初调查基线 `edc944040b66cfaf196272a6d5577f5c8fde4d70`。测试先行提交前 fast-forward 到 `e658cb4e2db1cff95ffb79e11a85fba444d6bd24`（#521 的 block 兼容性补丁，无 Shimmer 差异），后者为本轮生产未改 RED 基线；任务分支 `fix/345-shimmer-jitter`。
- 目标：同一标题、字体、字号、可用宽度及行高不变时，Shimmer 相位只能改变绘制颜色，不能改变字形、字距、基线、占位宽度或省略位置。保留现有单向柔和扫光与真实运行态。
- 范围：共享 `conversation_stream/shimmer.rs`，包括 Thinking 与阶段标题两个样式入口及 `StageText`。不改运行活动选择、标题内容、交互、权限、模型请求、用户数据、依赖版本或动效周期；不新增界面说明文案。
- 本地仅低 CPU 静态/格式/差异检查；不运行、编译、构建、打包或执行 Vega 测试。自动验证仅交现有 GitHub Actions，视觉与真实系统集成由用户合并后手测。

## 2. 验收用例（实现前冻结）

| 用例 | 前置/操作 | 必须可观察的结果 | 证据层级 |
|---|---|---|---|
| A1 固定文本跨相位 | 固定标题与排版，在一个周期的多个相位生成实际生产绘制数据 | 字形 ID、字体、每个字形的 x/y、基线、文本宽度完全一致；只有颜色随相位变化 | 进程内生产路径测试；用户视觉 |
| A2 复杂文本 | Latin 连字/kerning 组合、CJK 混排、组合音标、emoji/ZWJ、多字节文本 | 保留一次统一样式 shaping 的字形与几何，不按字符打断连字/组合；不出现无效 UTF-8 截断 | 生产布局与绘制路径测试 |
| A3 窄宽度 | 长标题逐步缩窄，含零宽、比省略号窄和正常截断宽度 | 零宽不绘制；宽度小于省略号时也不绘制；其余截断保留能放下的完整字形簇前缀与省略号，同宽度跨相位固定；内容遮罩限制字形 overhang；不产生负尺寸或 panic；保留完整原文的可访问名称 | 生产布局测试；静态链路审查；用户视觉 |
| A4 空文本 | 空标题、零宽内容 | 无字形、有限且非负的几何，不除零或 panic；无无意义动画刷新 | 生产边界测试 |
| A5 停止与恢复 | 运行 → Reduce Motion/隐藏/窗口失焦 → 恢复；运行 → 静态标题 | 停止时使用基础色、不继续请求动画帧；恢复后允许颜色变化但几何不变；静态路径沿用现有内容/交互 | 现有门禁静态审查；进程内适用测试；系统状态由用户手测 |
| A6 合法布局失效 | 相位以外，标题、字体/字号、可用宽度或主题发生变化 | 新输入使用新布局/新颜色，无旧标题或旧宽度残留；相位和颜色不触发重新 shaping | 生产布局测试与失效链路审查 |
| A7 两个入口 | Thinking 标题及当前阶段/工具活动标题，浅/深主题 | 两入口共享修复；保留各自主题色、既有字号/行高、动作/展开/aria；运行结束不残留 Shimmer | 调用链独立审查；用户手测 |

自动断言须在首次生产实现修改前写齐并经独立审查。冻结测试内容与准确边界随后在本文件及 PR 记录；不以源码字符串、仅 mock 调用、镜像算法或内部字段覆盖代替可观察的生产结果。无真实像素证据时不声称视觉验收通过。

## 3. 根因与证据

### 已核实的代码事实

1. `styled_text` 先用统一 `TextRun` 调用 `shape_line` 计算标题宽度。
2. canvas 的每次 paint 再按 `char_indices` 及相位生成逐字符颜色 `TextRun`，对完整/省略后的标题再次调用 `shape_line`，随后绘制。用于分配宽度的布局与用于实际绘制的布局不是同一个几何结果。
3. 当前锁定 GPUI Kit 0.6.1 → GPUI pre 0.3.4 的 `TextSystem::layout_line` 把颜色、下划线和删除线变化纳入 font run 分割条件。相邻同色字符会合并，不同色字符会拆分。
4. macOS `MacTextSystem::layout_line` 在相邻 font run 间交替使用原字号与 `next_up` 字号，以阻断跨 run 连字。因此相位引起的颜色边界变化可改变连字、advance 与 kerning，构成用户所报抖动的具体代码原因。尚未在本地运行或测量像素。
5. `LineLayout::paint(..., decoration_runs, ...)` 允许在已塑形的几何上单独提供颜色装饰，并复用 GPUI 的基线、emoji 和原生 glyph painter。本卡优先使用此 API，不手写另一套 glyph painter。

### 调用链

- `ThinkingBlock::render` → `shimmer::text` → `styled_text`。
- `RunActivityGroup` 的标题视图 → `StageText::render` → `stage_text` → `styled_text`。
- `StageText::set` 只在标题或运行 epoch 变化时 notify；非运行态沿用普通静态标题。外部真实活动及终态语义不在本卡重写。

## 4. 实现约束与计划

1. 在 `shimmer.rs` 分离布局准备与按相位着色/绘制。固定文本与排版先以统一样式 shaping；动画绘制消费同一字形 ID、字体及位置，不再按逐字符颜色重新 shaping。
2. 使用统一样式的 `Arc<LineLayout>`，相位着色只生成 `DecorationRun` 并交给 `LineLayout::paint`；布局准备可每帧调用既有缓存。按 GPUI 原有规则绘制基线、fallback glyph 与 emoji，不为测试新造公开接口或自定义 glyph painter。
3. 省略仅由可用宽度与原始布局决定，必须保持 shaper 已给出的 cluster 边界，不额外承诺完整 Unicode extended grapheme 分割。必要时对显示候选请求统一样式布局并验证宽度；重复候选由 GPUI 缓存命中，动画相位不得影响 shaping 输入或显示候选。候选从原始几何对应的可用宽度附近起步，再测量并退让，不能对长标题从近全长逐字穷举而生成二次规模的候选布局。保留原有超过可用宽度 1px 才省略的容差（绘制仍受标签遮罩限制）；截断后扫光仍按原文全宽归一化。零/极窄宽度按 A3 不绘制。
4. 可用宽度、文本及字体输入变化时重新准备布局；颜色和相位只影响绘制。保留完整原文 aria、原有 canvas 占位/最小宽度约束与两个入口的主题 token。
5. 维持内容遮罩可见、窗口活跃、非 Reduce Motion 的动画门禁；空绘制无无意义请求。不新增后台任务或刷新范围。
6. 仅在开发期间添加必要的本轮 Rust 测试/接线，不恢复历史测试集合，不扩大 production public API，不新增未批准依赖，不添加代码注释。

### 4.1 首轮独立审查决定（生产未改）

- 默认 `TestAppContext` 使用 `NoopTextSystem`，忽略 font run，且不提供可用于验证的 glyph raster bounds；不能据此声称复现 macOS 字体抖动。原生平台构造要求主线程，相关 text-system 替换入口未通过 facade 公开，本卡不搭建新的真实 UI 自动验收框架。
- 自动测试采用私有生产布局/帧边界，输入公开 GPUI `LineLayout`/`ShapedRun`/`ShapedGlyph` 的明确 fixture，验证实际 painter 消费的固定布局与 `DecorationRun`。fixture 包含连字簇、重复 cluster index、非零 y offset、fallback font 及 emoji；它验证已塑形几何不被破坏，不声称在自动测试中运行 CoreText。
- 首轮 RED 可为最小私有接口缺失，必须如实注明行为断言尚未执行。正式实现必须由 canvas 直接消费该生产边界，不保留只为测试存在的镜像绘制结果。
- 至少冻结三个有区分度的纯契约测试组：跨相位几何恒定且颜色确实移动；完整/省略/零宽/极窄宽度及同对象新输入更新；空态/停止/恢复与 emoji 几何保留。复用 GPUI painter，不再另造 glyph 命令或 emoji 路由抽象。若采用 GPUI 集成测试，只验证实际入口的 aria/边界/门禁，不用 Noop 结果代替原生 shaping 证据。
- `closest_index_for_x` 是最近索引而非能放下的索引；截断须验证最终显示宽度。零宽或省略号本身放不下时不绘制、不调度空动画。
- `paint_layer` 只控制绘制层次，不提供裁剪；调用 `LineLayout::paint` 时须使用已有内容遮罩与标签边界交集。允许合法字形 overhang，但不可越过遮罩。
- canvas 回调是 `FnOnce`，不能把 closure 临时值称为跨帧缓存。已核实 GPUI `LineLayoutCache::layout_line` 以文本、字号、font runs 与 force width 为 key，先查 current frame，再从 previous frame 移入相同 `Arc<LineLayout>`（`line_layout.rs:639–688`）。本卡复用该现有 window-local 缓存，不再自建持久缓存对象；统一 shaping 的颜色/装饰固定，动画和调色只进入 `DecorationRun`，因此 font run key 不随相位改变。完整 Font 的解析仍由 GPUI 执行，标题/字体/字号/宽度的新输入使用相应布局。
- v2 API 选择已获独立审查批准：`LineLayout::paint` 与 `ShapedLine::paint` 共用同一 painter，不触发 shaping。`DecorationRun.len` 必须按显示文本（含省略号）的 UTF-8 字节覆盖，不能按 glyph 数量或相邻 glyph index 差值计算；cluster index 可能重复或逆序。颜色之外 background/underline/strikethrough 均为 None，彩色 emoji 沿用原生行为。本卡不宣称新增 bidi 着色正确性。
- 1px 容差只作用于是否进入截断的判断；实际截断候选须放入真实可用宽度，不额外放宽 1px。宽度恰等于省略号时允许绘制省略号。
- v3 缓存简化已获独立审查批准：保留的是平台 shaping 结果，不是整个截断操作；每帧允许扫描 cluster、构造候选并查缓存。不使用期间跨帧淘汰后再次出现可合法重建统一布局，不承诺元素生命周期中永久 Arc 相同或只调用一次 shaper。宽度变化仍须重新选择显示候选，不能误以为宽度自动进入 GPUI cache key。
- v3 缩小最小私有接口为显示布局准备（输入原文、原始固定布局、可用宽度和统一样式 shape 回调）及相位着色帧（输出实际传给 `LineLayout::paint` 的固定布局与 decorations，并给出是否需要后续动画帧）。不引入独立业务缓存或新依赖。使用少量普通 Rust 测试，字体/跨帧缓存/aria/遮罩的接线由源码独立审查，真实字体外观由用户手测；不把该静态证据冒充运行测试。
- 先前两个测试委派均未交付完整测试文件，第一轮已中断，第二轮执行失败，仅留下测试模块接线；没有有效 RED/GREEN，也未修改生产实现。v3 在生产未改时重新确定接口和完整测试，不追认或补测已完成实现。
- 专职子 Agent 在此 v3 契约下先写齐测试，回报源码及接口清单，由独立审查确认后固定。在测试独立审查及云端有效 RED 之前仍不授权生产实现。

### 4.2 保留动效的数值核对

沿用原有 `center = fract(phase) × 1.6 − 0.3`，`strength = max(0, 1 − abs(x / original_width − center) / 0.24)`，`phase = elapsed_ms / 1800`。下表在原文半宽 `x / original_width = 0.5` 代入，使用独立十进制算术核对，非 Vega 代码执行/测试；约束不改动画，不作为本卡 GREEN。

| phase | center | 半宽处 strength |
|---:|---:|---:|
| 0 | -0.3 | 0 |
| 0.125 | -0.1 | 0 |
| 0.25 | 0.1 | 0 |
| 0.375 | 0.3 | 1/6 |
| 0.5 | 0.5 | 1 |
| 0.625 | 0.7 | 1/6 |
| 0.75 | 0.9 | 0 |
| 0.875 | 1.1 | 0 |

基线仍由 GPUI 共用 painter 计算：`top + (height − ascent − descent) / 2 + ascent`，然后保留 shaper 的每个 glyph y offset；不引入坐标取整或逐字符 advance。

## 5. TEST-FIRST、交付与证据

顺序：本规格/验收 → 生产未改测试补丁 → 独立审查 → draft PR 的现有 `pr-check` 有效 RED → 冻结 → 专职子 Agent 实现 → 同一测试 GREEN → PR 固定完整 SHA/原始源码/patch 与云端证据 → 独立删测提交 → 最新 head 空集完整门禁 → 正式 PR/主动 squash merge → In review 等用户手测。

- RED 允许最小目标接口缺失，但须明确行为断言尚未执行；依赖、runner、无关编译失败不算 RED。
- 不实现后补测、不改预期换绿、不 ignore 或重试换绿。测试接线错误须先记录并独立审查；验收变化先重开周期。
- 云端命令沿用 `.github/workflows/pr-check.yml`：fmt、全目标 Clippy、三组 Python 检查、Nextest list/run 与 required check `check (fmt, clippy, test)`。最终 Rust 为 `no Rust tests`（0 发现/0 执行），不称测试通过。
- CI 启动后每 5 分钟查询一次，完成/失败通知或用户即时要求除外。本地不运行上述自动测试。
- 测试补丁：`crates/vega_ui/src/conversation_stream/shimmer/tests.rs`，3 个普通 `#[test]`，197 行，SHA-256 `d903a8ad7daddc6d6f7c792b217afcae5b8e33a95f588b22cf63a49cce84a385`。生产模块只新增 `#[cfg(test)] mod tests;`，无生产行为改动、依赖或 workflow 改动。
- 目标接口（均私有且测试先行时尚不存在）：`display_layout(text, full: Arc<LineLayout>, available, shape_fn) -> Option<(String, Arc<LineLayout>)>`；`frame_decorations(text, layout, original_width, phase, base, highlight, moving) -> Vec<DecorationRun>`；`should_animate(layout: Option<&LineLayout>, visible, active, reduce_motion) -> bool`。
- 冻结测试：`moving_colors_preserve_mixed_text_geometry_and_utf8_spans`；`display_width_boundaries_shape_candidates_and_retreat_without_stale_output`；`empty_and_motion_gates_stop_colors_and_resume_on_the_fixed_layout`。颜色预期为独立数值，截断 shape fixture 是明确候选映射，不替代生产拟合选择。空态、glyphless、所有门禁组合及恢复包含在表中。
- 已执行本地静态检查：`rustfmt --edition 2024 --check crates/vega_ui/src/conversation_stream/shimmer/tests.rs` 与 `git diff --check` 均退出码 0。没有编译/运行测试。测试补丁经独立审查批准；[先行提交 f59131f58fbb6cd675c87e2c8282cb4e951bf105](https://github.com/puzige/vega/commit/f59131f58fbb6cd675c87e2c8282cb4e951bf105) 已进入 [draft PR #524](https://github.com/puzige/vega/pull/524)。
- 有效接口缺失 RED：[Actions 39533789268](https://github.com/puzige/vega/actions/runs/39533789268)，[Nextest job 119005580982](https://github.com/puzige/vega/actions/runs/39533789268/job/119005580982) 与 [Clippy job 119005581207](https://github.com/puzige/vega/actions/runs/39533789268/job/119005581207) 均仅报 `E0432`：测试第 1 行导入的 `display_layout`/`frame_decorations`/`should_animate` 不存在，`vega_ui` lib test 因 1 个前置错误退出 101。fmt、Python 步骤成功。行为断言尚未执行，发现/执行计数未取得；此证据不冒充原生抖动复现。
- 在读取并确认上述 RED 后，才授权专职子 Agent 修改生产模块；测试源码/预期冻结不变。
- 首次带同测试 GREEN：实现 `af28a3abca7a050ec15a11e495a03e20253b2ae7`，[Actions 39535699499](https://github.com/puzige/vega/actions/runs/39535699499) 的 Nextest 发现 3、执行 3、通过 3、跳过 0；fmt、全目标 Clippy、Python 5+15+14 与 required check 均成功。独立审查随后指出从近全长逐个枚举前缀会对长工具标题产生二次规模候选；该 GREEN 不是最终交付依据。
- 性能修正 `27cfe7f2fc847b40b56e0a2455e88148529e1802` 以原始 glyph 的 `available − ellipsis.width` 宽度预算筛选候选，再测量并退让，不改冻结测试。同步 master `d2d07b86b315d5f793ae1e353a820a77dace26ae` 后为带测试 head `2de33192dc24b62d9e5cd979ce881379e0c1cfbe`；Shimmer 与测试文件相对修正版本逐字节一致，标题调用者的 #520 变更单独审查。
- 集成基线带同测试 GREEN：[Actions 39537747298](https://github.com/puzige/vega/actions/runs/39537747298)，head `2de33192dc24b62d9e5cd979ce881379e0c1cfbe`；[Nextest 119018212133](https://github.com/puzige/vega/actions/runs/39537747298/job/119018212133) 原始输出为发现 3、执行 3、通过 3、跳过 0，冻结源码 blob `e6397a2d68fe954bd6938a07dd5238b03376cc56` 与原先行版本相同。Python 分别执行 5、15、14 个通过；[fmt/Clippy 119018212280](https://github.com/puzige/vega/actions/runs/39537747298/job/119018212280) 和 [required check 119019326492](https://github.com/puzige/vega/actions/runs/39537747298/job/119019326492) 成功。该结果仍不代替独立实现审查和删测后的最终空集门禁。
- [PR 固定原始测试 patch](https://github.com/puzige/vega/pull/524#issuecomment-6307155459) 含完整测试源码和接线，已从评论读回并重算 SHA-256，与冻结哈希一致，不依赖任务分支、临时文件或 Actions artifact。原始 RED/各次 GREEN 的完整 SHA 与证据分别留存，不用 rebase 后提交追认 RED 时序。原生字体像素/系统集成尚待用户手测。

### 5.1 第二轮：逻辑前缀与重排字形（实现修正前重新先测）

- 首轮实现/测试已 GREEN，但独立审查在 `2de33192dc24b62d9e5cd979ce881379e0c1cfbe`（生产文件 SHA-256 `f9e602db5c7ecaceb5e6d21ceae2dbf9761edb8f1fad2c5160d44aba1082cd75`）发现新的失败边界：视觉重排的 glyph x 不能作为逻辑前缀宽度。以 1024 个等宽 RTL cluster、可用宽 10、省略号宽 1 为例，按 x 筛选会尝试约 1014–1023 cluster 的近全长前缀并退化为仅省略号，尽管 9 cluster 前缀加省略号可以放下。
- 本轮停止生产修改，冻结新增验收：按逻辑顺序的完整 cluster 前缀准备实际测量候选，不依赖视觉 x 与逻辑 index 同序；上述 fixture 必须保留 9 cluster 与省略号、最终宽 10，并在最多 16 次 shape 请求内完成（包含省略号）。同时检查返回文本/布局及原始布局未变，调用数不单独作为通过证据。该上界为 1024 候选的二分搜索最多 11 次加省略号 1 次留出余量，拒绝逐前缀穷举；不承诺新增 bidi 着色或完整 grapheme 分割。
- 新测试 `shimmer/logical_prefix_tests.rs::rtl_logical_prefix_fits_with_bounded_shaping`，81 行、1 个普通 Rust 测试，SHA-256 `55c36048ca7bec879bf1ffb118997d6832c6f523342f25acd78878d6d38fd734`；原三测试哈希保持不变。新测试实际调用既有 `display_layout`，只用外部 shape fixture 构造重排布局，不替代拟合选择。生产差异仅测试模块接线；本地 rustfmt 与 diff 静态检查退出 0，行为尚未执行。
- 顺序：在上述现有实现基线上先添加独立测试模块，原 3 个冻结测试保持字节不变 → 非实现者审查 → Actions 行为断言 RED → 冻结新测试 → 专职子 Agent 修正逻辑候选搜索（预期使用有界二分、实际候选宽度验证，不再按视觉 x 过滤）→ 原测试及本轮测试一起 GREEN → 固定两轮证据后删测/最终门禁。不得把首轮 GREEN 追认为本边界已验证。
- 第二轮行为 RED 已确认：先行提交 `3e8db76aa5c4eada01baad97bfd6d7d9d7c0a8c1`，[Actions 39540575268](https://github.com/puzige/vega/actions/runs/39540575268) / [Nextest 119027125764](https://github.com/puzige/vega/actions/runs/39540575268/job/119027125764) 执行 4、通过 3、失败 1、跳过 0。新测试在第 65 行返回文本断言失败，实际 `…`、预期 `אאאאאאאאא…`；fmt/Clippy/Python 成功，非接线或环境错误。该轮测试原始 patch [已固定到 PR](https://github.com/puzige/vega/pull/524#issuecomment-6307562585) 并读回核验 SHA-256。
- RED 后实施计划已独立审查批准：不按视觉 x 筛选；逻辑边界合法化、排序、去重；先保留已测量省略号并测量首个完整 cluster 前缀作为候选，即使首候选失败也继续；对剩余边界使用严格缩小区间的有界二分探测，只保存实际测量能放下的最大已测逻辑前缀。上界为省略号 1 + 首前缀 1 + 至多 10 次探测，本 fixture ≤12 次。搜索不假装任意上下文 shaping 的前缀宽严格单调，也不承诺找到所有未探测候选中的全局最长前缀；现有拟合/cluster/无溢出约束与冻结断言不变，不做无关 bidi 着色扩张。

## 6. 用户手测与回滚

合并后在发布构建上使用合成标题，观察 Thinking/当前工具或阶段标题的浅深主题、长短标题、窗口缩窄、连字/中英文/组合字符及 emoji。整个扫光周期文字不左右移动；省略稳定；失焦、隐藏、Reduce Motion 与终态停止，恢复后继续正确状态。展开/收起、权限入口和可访问标题不受影响。

未收到用户明确通过前 Issue 保持开放、In review，不关闭/Done。需要回滚时通过独立 PR revert 本卡生产改动，不改用户运行时配置或恢复已删除历史测试。
