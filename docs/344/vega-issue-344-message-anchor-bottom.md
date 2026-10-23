# #344 消息锚点尾部高亮与鼠标预览关闭

## 基线与范围

- Issue：<https://github.com/puzige/vega/issues/344>。
- 方案 v2（测试冻结前独立审查修订），生产基线 `871f91f246e90753743a256cee1eec315e74effc`；任务分支 `feat/344-message-anchor-bottom`。
- 用户授权接手修复两个症状：最后一个锚点无法成为当前位置高亮；点击后预览在鼠标移出后仍保留。
- 仅修改消息锚点状态投影。保留稳定消息 ID 的请求、已加载定位、历史分页、路由隔离、tail-follow、主题 token、正文布局与持久数据。
- 本地不运行 Vega、不编译或测试。测试只交现有 PR Actions，真实集成由用户在合并后手测。历史 #72 / #387 的慢速完整根窗口组合回归不恢复。

## 原始证据与根因

1. `conversation_stream/message_anchors.rs::render_message_anchor_rail` 以 `logical_scroll_top().item_ix` 选择最后一个 `entry_index <= top_index` 的锚点。末条短消息的置顶请求会被 GPUI 列表回填前面的行，达到底部时 top_index 仍早于最后一个锚点。因此最后一屏正文已可见而当前位置无法落到最后一条。
2. 点击 marker 的 mouse-down 聚焦轨道，mouse-up 写入 `message_anchor_keyboard_id` 并请求稳定 ID 定位。移出只清除 hover，而 render 仅凭 focus 再次选择 keyboard ID，导致鼠标预览粘住。
3. `content.rs::reveal_loaded_message` 已使用稳定 ID 查找、暂停 tail 和 `ListState::scroll_to`。本轮未发现定位链断裂，不重写它。
4. 锁定依赖为 gpui-kit 0.6.1 / gpui-pre 0.3.4。`ListState::is_scrolled_to_end()` 在任意未测量条目存在时返回 None；不能单独用它解决长会话末尾高亮。`bounds_for_item`、`viewport_bounds` 和 `Window::last_input_was_keyboard` 都是真实可用 API。末尾实际几何必须包含 foreground bottom padding 的影响，且拒绝未布局、零宽或无有效阅读高度的几何。
5. 独立方案审查确认：`bounds_for_item(last)` 不保证 top 到 tail 中间连续已测量；未知高度会以零贡献进入前缀差。生产先 render rail、后 list prepaint/layout，因而 render 中直接读几何可能滞后一帧，resize/foreground 改变尤其明显。`is_following_tail()` 也不能替代实际已完成布局的证据。
6. 以上为源码证据，尚无本轮运行时或云端 RED 证据。

## 验收矩阵（先于实现冻结）

测试以合成持久消息 ID、真实 GPUI ListState、生产 rail/preview 事件与绘制为边界。允许轻量测试宿主隔离无关 Composer/外部服务，不能重写被测的选中、预览或定位决策。观测选中 marker 的实际 token 宽度/绘制、预览是否存在及其对象、MessageLocationRequested 的 ID；不以源码字符串、仅内部 bool 或镜像算法充数。

| ID | 前置与操作 | 必需可观察结果 |
|---|---|---|
| A1 | 溢出的变高消息列表，最后消息短于阅读视口；点击末条并完成正常布局，鼠标移出轨道/预览 | 请求仍为末条稳定 ID，末条正文进入有效阅读视口；top_index 可早于末条，但末锚点成为当前位置高亮；无粘住预览 |
| A2 | 长列表保留上方未测量条目，关闭 tail-follow 后手动滚到底，`is_scrolled_to_end()` 为 None | 不依赖全部行测量，末锚点仍高亮；用真实布局/绘制验证而非注入底部布尔值 |
| A3 | 从底部上滚至普通位置；以及尾行仅在 overdraw 中、仍在有效视口下方，或最后锚点之后还有较高的非锚点条目；缓存尾行但 top 到 tail 中间存在未测量行 | 不因“末条已测量”或“末锚点可见”提前判底；非底部继续按视口顶部对应消息高亮，不保留过期的鼠标选择；实际到尾部时选择最后有效消息锚点 |
| A4 | hover marker → click → 移出轨道与预览；以及 hover preview → 移出 | hover 可显示正确预览；移出后预览消失，不要求再次点击或强制 blur；再次 hover 能重新显示 |
| A5 | 键盘聚焦轨道，上/下选择，Enter/Space 定位，Esc 关闭；鼠标点击后再用键盘 | 键盘模式保留预览和稳定 ID 定位，Esc 关闭；鼠标/键盘切换无粘住或失去键盘可达性；保留 focus，不用删除焦点修复鼠标问题 |
| A6 | 空、单锚点或不溢出的短会话；未布局/无有效视口的状态 | 不显示空轨道或预览，不越界；正常有溢出、至少两锚点的阳性对照仍显示 |
| A7 | 底部末条变高、阅读区尺寸变化/foreground 占位改变后再次正常布局 | 当前位置依据当前几何重算；未到底不误选末条，到底仍能选末条；不修改原有跟随和跳转语义 |

A1–A3 的当前位置断言必须在鼠标离开且无键盘预览资格时执行，且检查旧位置不误亮；负例中顶部规则对应 ID 必须不同于末锚点 ID。测试宿主保留生产的 rail-render → list-layout 顺序，事件后只允许正常生产通知/帧任务收敛，不能额外注入 notify 或重复强制 draw 来掩盖 F2；预置 fixture 布局与实际验证动作明确分开。A6 的“未布局/无有效视口隐藏”保留为本轮明确目标，布局有效后必须自动恢复阳性显示。

A1–A7 可由少量行为测试组合覆盖，不以测试数量为目标。新测试必须在生产实现未改前写齐；测试缺陷修正先留证独立审查，不改变验收含义。若出现遗漏验收先修订规格并重新开 TEST-FIRST。

### 几何验算示例

无顶部内距时，列表视口高 600、foreground 下内距 180，实际阅读高为 `600 - 180 = 420`。已测量尾段 300 + 80，末条 80 的置顶请求不够填满 420，必须向前回填至少 340；顶部不可能留在末条。到底时最后内容边缘位于 `viewport.bottom - 180`。上滚 40 后该边缘位于有效底边下 40，不得仍判底。示例只解释夹紧，不引入生产固定高度或估算阈值。

## 最小实施计划（RED 后执行）

1. 在 `message_anchors.rs` 为当前位置选择增加局部真实末尾判定，只影响 rail，不改共享分页/跟随判定。几何应来自本轮列表布局完成后：在生产列表之后安排不参与布局的观测节点（canvas/等价 element），记录当前 logical top、有效 viewport、尾部结论等最小投影，只在投影变化时 defer notify，避免每帧无限刷新。`core.rs` 可保存该小型投影，`render.rs` 接入布局后观测；测试宿主必须复用同一生产观测入口及顺序。
   - 只用真实最后列表条目，而非最后锚点。top 到 tail 的相关尾段须连续具有有效已测量 bounds；遇到未知或已超出阅读底边即可停止并判非底，不挂载/扫描全部历史。不允许用 `is_following_tail()` 单独认定底部。
   - 上方未测量行可以保留，不要求全列表已测量；cached tail、中间未知、零宽/零视口、foreground 不一致的旧几何不得作为底部证据。读取完成布局后的有效 bottom padding，底部取最后有效锚点，其余保留既有顶部规则。
   - rail/preview 在首次尚无有效视口或实际有效阅读高度为零时隐藏，不用窗口 fallback 假装有效列表视口；布局观测变为有效后由同一受控通知恢复。
2. 同文件把“键盘预览资格”与单纯 focus 分开，以 `last_input_was_keyboard()` 约束 focused fallback 和对应窗口跟随选择，保留 hover、focus、按键及稳定 ID 请求。
3. 必要时用最小结构化 helper 收敛逻辑，但不为测试扩大公开 API、不加依赖或产品说明文字；不添加代码注释。
4. 不修改持久化、根窗口定位服务、provider、CI 工作流或视觉尺寸。测试专用 selector/接线可以临时加入，但不得改变生产决策，GREEN 后清理。

## 测试与交付流程

1. 专职子 Agent 写本轮测试及最少 test-support 接线，产出完整 diff 与静态检查证据并停写。
2. 非实现者独立审查测试/观测前提，主 Agent 提交 draft PR，现有 `pr-check` 执行完整门禁。有效 RED 必须是上述缺失行为导致，不接受无关编译/环境错误。
3. 固定测试提交、源码/patch、原始 RED 与准确计数，再委派最小实现；相同测试云端 GREEN 并独立审查。
4. PR 固定可恢复的测试源码、完整 SHA、RED/GREEN run/job 与计数后，独立提交删除 Rust 测试及其独占支持；验证生产差异没有新增行为。
5. 最终 head 的完整门禁通过且 Rust 0 发现/0 执行（no Rust tests）后转正式 PR、squash merge。CI 状态每 5 分钟查询，通知或用户即时请求除外。
6. Issue 保持开放，合并后设 In review 等用户手测；不使用自动关闭关键字。用户确认前不宣称验收完成。

## 用户手测

长会话到底后确认最后锚点高亮；点击末锚点再移开鼠标确认浮窗关闭；上滚确认高亮随当前位置变化；Tab/上下/Enter/Space/Esc 确认键盘路径仍可用；缩小窗口与改变 Composer 高度后复验。浅深主题和可访问标签沿用 token/既有结构，真实集成与像素外观待用户验证。
