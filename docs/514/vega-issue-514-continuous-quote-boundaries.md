# Markdown 多段引用的连续边界

## 状态与基线

- Issue：[#514](https://github.com/puzige/vega/issues/514)，复用 Draft [PR #529](https://github.com/puzige/vega/pull/529)。方案 v5（2026-10-22）：原引用边界与列表标记实现已存在；v3 针对解析边界补开 TEST-FIRST 并纠正无头字体观察，v4 记录列表内引用缩进边界，v5 记录较宽列表标记后的 dedented 顶层围栏；两项独立审查新发现均先测后修，不追认已完成实现。
- 初始基线：`e542175a726f3ab074d9e9fe63bd5f0383804b90`；分支 `feat/514-continuous-quotes`。本轮生产未改基线为 `7ec15f2c44269d7e068e853ed797f651668dc019`，保留公开 head `ae1888267fd8b05db1c388c01ed80bfc40643a1b`，并 merge 同步主库 `e4f18c656aa69d192d095a4338a2108e6c9a3c2c`，不重写历史。
- 主 Agent 负责规格、协调、审查与集成；专职子 Agent 写测试及实现。遵守[执行总纲](../vega-exec-guide.md)、[测试生命周期](../vega-rust-test-delivery-lifecycle.md)和[设计守则](../vega-design-guidelines.md)。本地不运行 Vega、编译或测试。
- 用户补充截图显示单段长引用自然换行、左侧竖线连续，作为单段回归参考，不作为多段落已通过的证据。

## 目标与验收（先于实现计划）

同一 Markdown 引用块的左侧边界覆盖所有内部段落和段间留白。保留正文、段间距、缩进及自然换行，不修改源 Markdown，不增加界面文案。独立引用与嵌套引用按真实结构分开。

| 用例 | 可观察预期 |
| --- | --- |
| Issue 的三个 `>` 段落，段间空行也带 `>` | 同一层竖线覆盖三段与两个段间留白；段落仍分开 |
| 单段长中文及中英文混排，窄/宽列 | 正文在竖线右侧剩余宽度内自然换行，竖线覆盖全文，无水平溢出 |
| 引用 / 普通正文 / 引用 | 中间正文无引用线，不连接两端 |
| 外层段落 / 内层多段 / 外层段落；相邻内层独立引用 | 外层边界连续，内层按真实范围另画一层；内层结束后恢复外层缩进，不错误拼接不同内层引用 |
| 增量 append、快照更新、finish | 流式结束与同源一次性渲染有相同引用层级及边界；不跨非引用内容连接 |
| 选取跨段正文并复制，引用内链接 | 文本顺序、段落分隔及链接 URL/范围保留；装饰不进入复制，链接仍可触发原有打开路径 |
| 滚动及窗口宽度变化 | 边界随实际布局更新，不因固定估算高度或裁剪截断；重复布局无旧位置残留 |
| 普通正文、代码块 | 不引入引用装饰；代码保留既有不软折行与横滚行为，不纳入 #485 代码拖选修复 |

适用会话可选中与非选中两条 Markdown 路径；共享投影消费者（展开思考正文）不得因模型变化出现断线或层级回归。真实剪贴板、外部链接启动、原生滚动与流式体验最终由用户在合并后手测；进程内测试不得冒充真实系统集成。

## 根因与关联

- `crates/vega_markdown/src/nodes.rs::parse_blocks` 已保留 `RenderNode::BlockQuote { children }` 结构，无需靠源文字符串识别引用。
- `crates/vega_ui/src/conversation_stream/model.rs::flatten_node` 递归展开引用后将所有子行标成 `LineKind::Quote`，没有保存引用归属及嵌套层级。
- `render_rows.rs::markdown_blocks` 将段距放在逐行外层的 `mt`；`render_line_selectable` 与 `render_line` 的 Quote 分支各自在行内部绘制竖线，因此外部段距未覆盖。
- `thinking_body.rs::PreparedBody::new` 也按行高度绘制引用装饰，段距在装饰外，需协调共享边界。
- #485 Issue 虽关闭，[PR #511](https://github.com/puzige/vega/pull/511) 仍为 Draft。其普通引用折行方案是在两条 Quote 正文路径增加 `flex_1().min_w_0()`，尚不能假定已进 master。此卡必须保留剩余宽度约束这一行为，不整支合入 #511，不引入其旧测试集合或代码拖选周期改动。

## 实现计划

1. 在 `model.rs` 的投影中保留每个引用的确定性归属及层级。以解析树边界为依据，不能只比较 `LineKind::Quote` 或只用 mdstream block id 判定所有嵌套归属。保留现有 spans、link occurrence、代码与列表信息，避免把内容改写为一段文本。
2. 在 `render_rows.rs` 基于真实归属构造边界或装饰，让同一引用的装饰包含内部间距，外部间距不带边界。选中/非选中路径共享一致的几何规则。引用正文按剩余宽度布局，竖线不可挤宽正文；嵌套按每层现有竖线宽度 2px 与 `MARKDOWN_CODE_PADDING` 递进。
3. 保留原有块间/段间 token，不以压缩段距解决断线；内部具体间距沿用现有值，本卡不做整体排版重设计。
4. 同步 `thinking_body.rs` 中的装饰、文字宽度及位置计算，保持选择源映射及滚动裁剪正确；不改变其流式生命周期。
5. 原引用边界周期不新增生产依赖、不改 Markdown 解析或源文本、不改持久化/运行时配置、不触碰 #485 横向代码拖选调度。用户现明确授权下述独立周期的 mdstream 0.3 最小边界补丁，不迁移 0.4、不引入新 crate。允许独立审查后的临时 `gpui_kit/test-support` dev-only 接线，GREEN 后一并移除。

只读技术审查已确认：引用身份采用 stream block ID 与确定性解析树遍历身份组合，每行携带独立的引用祖先路径，不复用列表 depth；保留子节点原始 kind，逐项审查现有 Quote 消费者。普通/可选择路径按祖先前缀构造容器，每个容器拥有自己的全高边界；段间距在容器内，独立块外距在容器外。Thinking 按引用容器累计 body 坐标中的边界 span，并在 BodyElement 层独立于逐段可见性剔除进行绘制，应用相同 y_offset 与 content mask。只将装饰挂在第一段并延长高度不可接受，因为首段离屏会使仍应可见的竖线被剔除。测试以实际绘制区间并集断言，不约束必须产生一个 quad。若改变上述验收含义，必须先更新规格并重新开始测试周期。

## v2 冻结前边界澄清

独立方案审查后、生产修改前明确以下验收，测试未冻结且尚无 RED：

- 同一引用容器的普通段落间距保持现有 `MARKDOWN_BLOCK_GAP`，即 16 逻辑像素，不因恢复 Paragraph kind 变为普通正文的 12px。嵌套进入/退出及同级独立引用之间，公共祖先的竖线覆盖间距；退出与新进入的子引用竖线不跨间距相连。
- 引用祖先、真实列表归属、列表 depth 相互独立，不能因同一 stream block 包含列表就给无关段落添加列表缩进。冻结混合引用子节点用例：段落/列表/段落，以及段落/围栏代码/段落；保留其真实内容呈现与既有代码横向布局，不延伸到代码拖选自动滚动。
- 引用身份在单个投影中确定且能区分同深度不同兄弟；parse version 与分配地址不是身份。pending 重解析或转 committed 后只保留一份当前显示，无旧 wrapper/竖线残留；若解析树改变导致结构身份变化，安全失效旧布局即可，不要求数字 ID 永恒不变。不改变无变更 committed block 的冻结缓存。测试检查中间 append、finish、同一 stream block 内不同引用，而非只比最终文本。
- 装饰/换行不贡献复制字符，wrapper 不改变选择注册顺序及原段落偏移。跨段 Unicode 复制保留 `\n\n`；同一代码块物理行用 `\n`，不同代码块保留块分隔；列表保留既有可见标记与行/段分隔。验证混合内容复制，不能只比较内部字段。Thinking 在嵌套缩进、增量更新、resize 后保留源文与渲染文本的选择对应。链接以隔离外部打开边界验证实际请求目的地，不启动真实浏览器。
- Thinking 必测：首段在 culling/overscan 外、后段仍有竖线；content mask 只落在内部空隙、无文字时仍有所属竖线；resize/reprepare 后无旧几何且遵守 y_offset/裁剪；嵌套线结束而外层继续。绘制区间先与 quad 自身 content mask 相交，再换算缩放像素并比较覆盖。

## 测试与交付顺序

1. 在生产未改基线上先写齐行为测试及所需最小 test-only 接线。测试使用行为名称，不用 issue 编号。必须验证生产投影和实际布局/绘制结果，不以源码文本扫描、内部字段相等或只测新 helper 代替可观察行为。
2. 覆盖三段连续、独立引用不误连、嵌套层级、窄/宽布局、单段保护、增量/终态一致及选择/链接边界；可用 GPUI `painted_quads()` 检验段间区域实际有无竖线（区分逻辑与缩放像素）。多条路径用表驱动或共享测试支持避免重复。
3. 非实现写入者独立审查测试，固定测试源码及生产未改基线；建立 draft PR，现有 `pr-check` 运行 fmt、全目标 Clippy、Python 和 Nextest。仅目标行为缺失的断言失败或最小缺失接口构成有效 RED；无关编译失败不算 RED。
4. 主 Agent 确认 RED 后才委派最小实现，冻结测试不追改；同一测试 Actions GREEN 后固定完整 SHA、可访问源码/patch、run/job、发现/执行/通过/失败/跳过计数。
5. 独立删测提交移除本轮 Rust 测试及独占支持；独立审查生产不变并验证最终 head 完整门禁，Rust 明确 `no Rust tests`、0 发现/0 执行。再正式 PR、合并、In review 等用户手测；Issue 保持开放。
6. 本地仅快速静态检查，如 `git diff --check`、格式检查和依赖元数据静态核对，不本地执行 Vega 或测试。云端查询间隔至少五分钟。

## 风险与回滚

风险集中在投影身份跨 pending/committed 转换、嵌套层级、选择映射和双渲染路径一致性。以冻结测试和独立审查控制，用户手测确认真实交互。回滚本卡生产提交，不回滚 #485 独立改动，不修改用户内容或数据。

## 验证记录

- 冻结测试及测试接线最终提交：`bffce7b6c9eac5e83b8efb0241d690dba74b2ed8`，生产未改。前序编译/事件/选择层接线失败均在 PR #529 留存，不算目标 RED。
- 有效 RED：[Actions 39572884256](https://github.com/puzige/vega/actions/runs/39572884256)，Nextest job `119131160632`：16 发现/16 执行、6 通过/10 失败/0 跳过，Clippy/fmt 通过。核心失败为 16px 段间断线、462px 正文溢出 240px 列、引用代码丢失横滚。链接、Unicode 选择与真实 Thinking 生命周期控制已通过。
- 首版生产实现：`1505044ede5f7af7bcae3d32734e308d0c12f052`，冻结测试未改。本地仅格式与差异静态检查通过，GREEN/最终门禁/用户手测尚未取得。

## 独立追加周期：列表非段落首项标记

独立实现审查发现保留底层 kind 后，`flatten_list` 仅将 Paragraph 首项设为 ListItem；标题或代码首项虽保留 marker/checked，但各渲染器只在 ListItem 分支输出标记，会丢失列表标记。原先行测试只有段落首项，不能覆盖这一遗漏边界。当前周期暂停交付，禁止实现后补测追认；以 `1505044ede5f7af7bcae3d32734e308d0c12f052` 为追加周期生产未改基线，单独先测、审查、Actions RED 后再修。

追加验收：普通正文及引用内部，以标题、围栏代码为首子节点的列表项仍显示并按可见顺序复制列表标记/任务状态，同时保留标题字号/字重、代码物理行和横滚；引用边界与缩进不回归。不重做列表排版、不扩展代码拖选行为。

追加用例先验证解析器实际输出，避免把不支持的 Markdown 当作任务列表：`- # heading-first`、列表项内围栏代码；task 变体仅在解析器识别 checkbox 时断言。测试需验证普通/可选择/Thinking 的实际布局及可选择路径复制结果，而非只检查 marker 字段。

列表标记周期已实施：独立列表项开始信息（marker、checked）与底层 kind、引用祖先、列表 depth 分离，渲染/选择与 Thinking 同步消费。先行测试 `b21f635eecb256fdb13c4177df0680eccbea254d` 的 [Actions 39577022103](https://github.com/puzige/vega/actions/runs/39577022103) 执行 19 项，16 通过/3 个新增目标失败/0 跳过，构成该周期 RED；后续实现 `ae1888267fd8b05db1c388c01ed80bfc40643a1b` 的 [Actions 39578674519](https://github.com/puzige/vega/actions/runs/39578674519) 执行 19 项，17 通过/2 失败/0 跳过，Clippy 通过。原 16 项已通过，但全部 19 项尚未 GREEN，不得删测。

## 独立追加周期：列表围栏的流边界（v3）

### 证据与责任层

原始失败为 `nonparagraph_first_list_items_copy_markers_in_visible_order`：Selectable、非引用输入预期 `before\n\n• code-first\nsecond-line\n\nafter`，实际在 `after` 前多一个换行。静态调用链是 `MarkdownStream::append` → mdstream `machine::is_new_block_boundary` → `BoundaryDetector::current_line_can_interrupt`：列表模式遇到缩进的关闭围栏仍按新围栏起点拆块，关闭围栏随后成为另一代码块的开头。该归因为待本轮公开解析边界断言确认的产品依赖缺陷，不能在 UI 复制层 trim 或吞分隔符掩盖。

### 先行验收

- 用公开 `MarkdownStream::append / snapshot / finish` 检查显式期望的 `RenderNode` 树：列表内完整围栏属于该列表项，后续普通正文不被吞进代码或生成伪代码块。普通与引用内列表均覆盖；不能只拿两个同样错误的解析结果互比。
- 对完整输入、逐 Unicode 字符及在列表标记、开/关围栏、换行附近分割的合法 UTF-8 chunk schedule 验证最终相同的明确期望；检查有意义的中间快照，finish 幂等。
- 真实代码内部空行与关闭围栏前的真实空行逐字保留，不允许 trim、删空行或改写源文；无空行的原 UI 复制断言完全保留。包含反引号和波浪围栏、列表结束后的顶层围栏对照，确保最小规则不会吞掉真实的新块。
- 流式缓存边界保持：已 committed 的无关块 ID/version/借用节点身份及内容不随追加而重算；后置引用定义仍只使相关块失效并产出正确链接。用稳定快照边界观察，不扩大生产公共 API。
- 原 19 个测试与业务断言保持；新增用例不是旧周期的先行证据。本轮解析生产实现尚未改变，先独立审查测试并由同一 Draft PR 取得云端有效 RED，再冻结修复。

### 最小实现计划（仅在有效 RED 后）

1. 沿用锁定的 mdstream `=0.3.0` 与现有 committed/pending、adapter、版本失效契约。依据已有 vendoring 预案，从确切 0.3.0 发行源固定来源、checksum、双许可，采用本地 patch 路径；不迁移 0.4，不引入新依赖或上游测试集合。
2. 只修列表延续行的围栏边界误判，具体最小条件由冻结测试和源代码状态机共同确定；不通过每 token 全量 reparse、重写源文本或 UI 复制补偿实现。原有顶层围栏和非列表规则不泛化重写。
3. 独立审查 vendored 来源与语义增量；同一冻结测试全部 GREEN 后固定源码/完整 SHA/run/job，再独立删测，最新 head 全量空集门禁通过后才交付。

## 测试观察修正：无头字体（v3，不改验收）

- 原 `heading_first_list_items_keep_visible_marker_and_heading_style` 在 marker 位置、H1 字号断言后，通过 shaped run 的 `font_id` 反查字体得到 None。实际 GPUI `NoopTextSystem::font_id` 返回 1，但 `layout_line` 忽略输入 font runs 并生成 ID 0；反查该 dummy ID 不是有效字重观察。
- 在真实渲染/布局边界以 cfg(test) 观察实际送入 `StyledText / shape_text` 的有效 `TextRun.font`，与已有可见文本及布局记录绑定；覆盖普通、可选择、Thinking，不能仅调用样式 helper 或从期望 token 伪造记录。生产行为不变。
- 保留 H1 字号与 `Typography::HEADING_PAGE_WEIGHT` 的原断言含义，观察修正不删除、跳过、默认化或弱化字重要求；用同一真实路径中的普通正文作区分度对照，证明观察不会恒为标题字重。该接线已独立审查，`ea64ee4dc3cb682d67c84bf2963cf83c79acd59d` 的 Actions `39741381663` 中该测试通过；本轮 parser 两项与原复制一项目标失败，24 执行/21 通过/3 失败/0 跳过，构成 v3 有效 RED。

## 独立追加周期：引用与列表的祖先顺序（v4，生产未改）

### 新证据与责任层

独立实现审查于 2026-10-22T08:02:37Z 报告新边界：在 `- item\n\n  > quote one\n  >\n  > quote two` 中，引用位于列表项内部。当前模型只记录 quote identity 与行的 list depth，丢失两种容器的祖先顺序。`render_rows::markdown_blocks` 先把列表 inset 加在正文 body，`quote_blocks` 却在 body 外绘制 rail；Thinking 文字 inset 包含 list depth，而 rail x 仅依赖 quote depth。相对文本列，文字仍在 x=38px，rail 从旧路径的 x=24px 移到 x=0px。主 Agent 已读回固定 `ea64ee4dc3cb682d67c84bf2963cf83c79acd59d` 与 master 的相关分支，确认这是不同于 mdstream 围栏的产品 UI 回归；尚无本轮几何执行证据。

该新边界暂停最终交付，不改 v3 冻结的 24 项，不在 mdstream 补丁中夹带 UI 修复。以 UI 生产等同 `ea64ee4dc3cb682d67c84bf2963cf83c79acd59d` 的实现前状态编写新测试，独立审查、云端有效 RED 后才修；v3 的未闭合波浪围栏 pending 断言仍未 GREEN；为避免盲修后再发现同类边界，可在当前 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a` 基线上先编写 v4/v5 新测试，与原24项共同执行并分别确认目标 RED。v4/v5 各自先测后修，不把已存在 v3 失败当作其 RED；全体同一冻结集 GREEN 前不得删测。

### 先行验收与算术

现有 `MARKDOWN_LIST_INDENT=24`，rail 宽 2，`MARKDOWN_CODE_PADDING=12`，quote gutter=2+12=14；各 x 均为相对正文列原点的逻辑像素。禁止一律把 list inset 加给所有 quote rail，否则会移动包在列表外的引用。

| 真实祖先顺序 | 必需 rail x | 正文位置/不变量 |
| --- | --- | --- |
| List(depth 0) → Quote | 24 | quote 文本 24+14=38；同一 rail 覆盖两段及 16px gap；列表外正文不带该 rail |
| Quote → List(depth 0) | 0 | 外层 rail 不随列表增加24；列表内容在 rail 后14，再走既有列表布局；退出列表仍是同一外层 rail |
| List(depth 1) → Quote | (1+1)×24=48 | 文本48+14=62；深层列表退出后不残留该缩进 |
| Quote → List(depth 0) → Quote | 外层0，内层14+24=38 | 内层正文38+14=52；外层仍在0并覆盖其拥有的间距，内层不跨自己的边界 |

测试用公开解析输出先确认真实 List/BlockQuote 祖先；用实际普通/Selectable/Thinking 渲染的已裁剪2px rail 和文本 layout 断言位置、覆盖与窄宽重排，不仅检查内部元数据。可用最少有区分度的表驱动用例覆盖上表，保留既有 list-in-quote 对照与所有24项；源码/复制/链接期望不因几何改变而改写。无需真实系统 E2E 或扩大 #485 拖选范围。

### 最小实施计划（v4 有效 RED 后）

1. 在投影保存每个 quote 容器创建时已有的列表祖先 inset/深度，身份、quote depth 与 list depth 保持独立；不能在渲染时用任意后代行的 list 状态反推 quote owner 的位置。
2. 普通/可选择 quote container 以相对父 quote 的列表祖先增量定位 rail，正文只消费一次列表 inset，避免双重缩进；Thinking 以相同 owner 原点画 rail，宽度/锚点消费者保持一致。不重构所有 Markdown 容器、不全量 reparse。
3. 新冻结几何测试与原24项同一云端 GREEN、独立实现审查后再进入证据固定/删测/最终门禁；新 cycle 源码/RED/实现/GREEN 与 v3 分列，不复用 v3 RED 冒充本边界证据。

## 独立追加周期：较宽列表标记后的顶层围栏（v5，生产未改）

### 新证据、验收与区分度

独立 mdstream 实现审查于 2026-10-22T08:20:45Z 报告 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a` 中的 guard 使用 `is_list_continuation`，该现有 heuristic 只看至少两空格，不依据当前列表 marker 宽度。`10. item\n  ```text\n# literal\n  ```\n` 的 `10. ` marker 占4列，两格围栏已退出列表，必须开始顶层代码块。过宽 guard 却使 List 模式继续，后续 `# literal` 被 heading interrupt 拆成标题。主 Agent 已读回 facts/machine/detector 与 pinned pulldown 的 marker-width 规则，确认是本补丁引入的独立产品边界，尚未执行；原24项的顶层对照只有0缩进，不能排除它。

- 原24项冻结且保留；v5 新用例在生产未进一步修改的 `27b88c2e9934a01aa912ed95fc26b6ec3fd3470a` 上先编写、独立审查、Actions 有效 RED，再修，不修改原测试追认。
- 明确公开树：`10.` 的列表保留 ordered/start=10/item，后接独立 CodeBlock(language=text, code=`# literal\n`)；不能出现额外 Heading/伪代码块或吞掉后续正文。普通源文与所有真实空行逐字保留。
- 完整输入、Unicode字符和语法chunk schedule均检查；反引号/波浪围栏、2格/3格dedent代表、较宽有序marker的真正4格或更多延续close对照，用最少有区分度表覆盖。真正列表close不能回退到v3误拆，dedented顶层代码内部的heading语法不能被当作块边界。
- v4几何和v5解析是不同原因、不同断言，可在同一先行测试提交及现有Actions执行，但记录各自目标失败，不用一个RED替代另一个；任一编译/观测/环境失败单独处理。不得为节省CI删减原24项。
- 冻结前独立方案审查（08:45:50Z）补齐真正列表围栏内部的 literal 保护：`- ~~~text\n  # literal\n  \n  second\n  ~~~\n\nafter` 必须产出 list(code=`# literal\n\nsecond\n`) 与独立 after 段落，不能因 heading/thematic/blank 等普通块中断规则拆开代码。未闭合快照同样保留字面内容，无合成~~；反引号与较宽列表的真实归属对照同理。此澄清在 v4/v5 新测试冻结前，不改原24项；必须纳入新先行补丁及本轮云端验证。

### 最小实施计划（v5 有效 RED 后）

只在本地0.3补丁内传递必要的当前列表项延续缩进/围栏上下文，使新围栏边界依据真实marker宽度与所属项，而不是固定两空格；保持其他生产/adapter/cache/source契约，不引入全量reparse或0.4。独立可行性审查确认现有 BlockMachine/process_line 可保存所属项的内容缩进、quote/list前缀及活动围栏字符/长度，先于普通 heading/thematic/blank中断判断保护字面内容；匹配的所属close只结束内部围栏，不结束外部List。dedent退出所属容器时恢复正常顶层边界；incomplete-tail只能预览，不重复消费半截marker/fence；block/reset/compaction同步上下文。

pending display消费同一活动围栏上下文，在trim/setext/inline repair之前保护代码字节；不靠全buffer三字符计数或只找16KiB tail内opener，不将整个List重标为CodeFence，不禁用所有pending显示，也不额外每append全量pulldown解析。具体最小状态机方案先于实现审查；若发现支持边界无法在最小方案内闭合，先记录证据与必要验收变化，不扩大parser重写或暗改断言。与v4 UI修改分清文件/语义差异，同一冻结完整集GREEN后才进入删测。

### 同一 v3 冻结集的残余 pending 缺陷（非新增验收）

`27b88c2e9934a01aa912ed95fc26b6ec3fd3470a` 的 Actions `39746319972`：24 执行/23 通过/1 失败/0 跳过，Clippy 与 Python 通过；日志确认编译 vendor/mdstream。原完整输入及 UI 复制已通过，同一 `list_fence_snapshots_keep_unrelated_committed_nodes_frozen` 在 tilde/unquoted/首个未闭合快照多 `Paragraph("~~")`，原期望不变。这不是测试观察错误：List 路径进入 pending terminator，`is_inside_incomplete_multiline_code_block` 只认三反引号，`balance_strikethrough` 将 `~~~` 的一对 `~~` 误作未闭合行内标记，在正文后追加合成 `~~`。

该行为已经属于 v3 先行冻结中间快照断言并获得有效 RED，可按原目标修 pending 围栏识别/repair 责任层，不另补测试追认，也不在 UI 删除合成字符或禁用所有 pending 显示。保留真实代码空行、原 strikethrough 配置、committed 缓存；具体最小状态识别需和 v5 围栏归属共同闭合，生产修改仍等 v4/v5 新先行用例独立审查及各自有效 RED 后组织。
