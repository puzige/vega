# #682：上下文用量环对齐 ChatGPT 的单一分子口径

## 契约版本与范围

- v1，2026-10-27；基线 `07a9b355`（`cec629ed` 之后无相关改动）。
- 用户裁决：**对齐 ChatGPT 桌面版的上下文指示器口径**。同一个环不再维护两个可切换的分子。
- 只改 Native 主调用在 composer 环与详情里的展示数据链，以及为冷启动恢复而新增的一条只读查询。保持布局、尺寸、交互、分母语义、预算/压缩决策、provider 协议和费用累计不变。

## 事实与根因（已核对）

1. 环的分子有两个写入者，且在同一 run 内交替生效：
   - 请求前预检 `context_control.rs::apply_context_accounting` 写 `estimate = record.predicted_input`，`usage_source = EstimatedInput`；
   - provider usage `context_control.rs::apply_context_usage` 写 `estimate = usage.input + usage.output`，`usage_source = LastCallTotal`。
   两者既不是同一次调用，也不是同一统计范围，所以同一环在「下一次预计输入」和「上一次实际总量」之间反复换含义。
2. 冷投影 `apply_context_projection` 在 run 开始（`window/context.rs::begin_context_primary_owner`）与重新进入会话时把全量粗估写入同一个字段，因此新一轮/重开会话先显示偏保守的粗估；重开不恢复上次真实值。
3. `estimate_wire_context` 是确定性近似（文本字节/4、JSON 字节/2、非图片部分再乘 4/3，另加协议开销与安全余量），不是模型分词；偏差方向与幅度都不固定。
4. 运行期 `ContextAccountingRecord.input_budget` 与投影 `model_input_limit` 数学恒等（`ContextBudget::new(input + reserve, reserve)` → `input_budget() = input`）。**分母不是跳动来源**，本卡不改分母语义。
5. 对照对象 `/Applications/ChatGPT.app`（Codex 桌面版 `26.1007.71326`，核心 `0.162.0-alpha.17.2`）静态读取：圆环只有 `MF()` 一个计算函数，分子恒为服务端 `token_count` 事件的 `last.totalTokens`，分母恒为模型目录 `modelContextWindow`，`percent == null` 时不渲染圆环；前端无 tokenizer，无第二套估算占位。

## 契约

### U1 — 单一分子来源

环的分子只接受「最近一次 Native 主调用」的 `usage.input + usage.output`（缓存已在 input 内，不重复加）。预检、冷投影、压缩状态记录都不得写入环的分子。

### U2 — 无真实值不占位

没有真实主调用总量时：环只画轨道、不画填充、不显示百分比；详情显示「尚无数据」与容量信息，不显示 0 或估算值。冷投影的估算只进入「预计下次请求」一行。

### U3 — 估算退到详情并区分可信度

「预计下次请求」只在详情出现，并按 `ContextAccountingSource` 区分：`UsageAnchored` 标为「基于真实用量校准」，`Estimated` 标为「估算」。两者不得共用同一句文案。

### U4 — 重载恢复

重新打开会话时，分子从持久化的最近一条主调用 usage 行恢复（`token_usage.message_id IS NOT NULL` 的最新一行）。摘要、自动标题、composer 预测等 `message_id IS NULL` 的行不得作为分子。最新主调用行若 input 为零则不恢复，也不回退到更旧行。持久化投影只补充尚无真实值的控制器；一旦已有恢复值或实时 usage，不得由迟到投影覆盖，后续真实调用仍可降低总量。

### U5 — 分母不变

显示分母仍取配置的模型输入上限；运行期 `record.input_budget` 只延续既有写入路径，不改语义、不改预算判定。

### U6 — 生命周期

- 同一次调用内重复或更小的 usage 幂等，不回退、不累加。
- 同一 message 的下一次调用用新总量替换，允许真实下降。
- 压缩（`PostSummaryTarget`）不再清空或覆盖分子；下一次主调用 usage 到达后按真实值更新。
- run 结束/取消保留最近已知总量；新 run 不因开始而清空分子；模型或会话切换按既有 `reset_context_control` 重建。

### U7 — 无效数据

`input == 0`、`input + output` 溢出、非活跃 message、错误 thread/model、非运行态、响应归属不匹配时显示不变，不产生 0/回绕/饱和假数据。

### U8 — 文案与可访问性

- 分子标签为「最近调用总量 / 配置输入上限」与「最近调用总量：{tokens} tokens（输入＋输出）」，不再把真实总量叫「估算输入」。
- 无真实值时为「尚无数据」；未配置上限仍显示「配置输入上限未设置」，超限仍显示「已超过配置输入上限」。
- 无障碍名、键盘 enter/space/escape、悬停与点击行为保持；必要容量说明不删除。

## 验收矩阵

| ID | 前置 | 操作 | 可观察结果 | 验证 |
|---|---|---|---|---|
| U1-1 | 活跃 Native 主调用，上限 400000 | `UsageUpdated(input=120000, output=30000, cache_read=80000)` | 分子 150000、占比 37.5%；缓存不重复加；详情标「最近调用总量」 | GPUI 回归 |
| U1-2 | 同上已有 150000 | 预检 `predicted_input=160000` | 环保持 150000；160000 只出现在「预计下次请求」 | GPUI 回归 |
| U2-1 | 新 run 仅有冷投影 240000 | 应用投影 | 环无填充、无百分比；详情「尚无数据」＋容量 | GPUI 回归 |
| U2-2 | 无任何真实 usage | 展开详情 | 无百分比、无填充条，不伪造成 0 | GPUI 回归 |
| U3-1 | 已校准锚定 | 下一请求预检 | 「预计下次请求」标为「基于真实用量校准」 | GPUI 回归 |
| U3-2 | 无锚定 | 下一请求预检 | 标为「估算」，与已校准可区分 | GPUI 回归 |
| U4-1 | 线程已有主调用 usage 行 | 重新打开会话 | 环恢复该行 `input+output` | 静态查询 + GPUI |
| U4-2 | 线程只有 `message_id IS NULL` 行 | 重新打开会话 | 按空态，不把非主调用行当分子 | 静态查询 + GPUI |
| U5-1 | 已配置 input_limit | 运行期收到 accounting | 分母等于配置值且与 `record.input_budget` 一致 | GPUI 回归 |
| U5-2 | 未配置上限 | 收到真实 usage | 无百分比；保留「配置输入上限未设置」 | GPUI 回归 |
| U6-1 | 同调用已有 150000 | 重复同量/更小 usage | 幂等不回退 | GPUI 回归 |
| U6-2 | 同 message 下一 response | `input=80000, output=20000` | 替换为 100000，不累加 | GPUI 回归 |
| U6-3 | 压缩 `PostSummaryTarget=40000` | 应用 accounting | 环不变；下一主调用 50000 后更新 | GPUI 回归 |
| U6-4 | run 结束/取消 | 迟到投影到达 | 保留最近值；旧 run 事件不再接受 | GPUI 回归 |
| U7-1 | 活跃调用 | `input=0`、溢出、其他 message/thread/model | 显示不变 | GPUI 回归 |
| U8-1 | 空态与有值两态 | 读无障碍名、键盘操作 | 单一口径表述；enter/space/escape 不变 | 静态 + GPUI |

算术核对：120000 + 30000 = 150000；150000 / 400000 = 37.5%；缓存不另加。下一调用 80000 + 20000 = 100000，替换而非累加。`u64::MAX + 1` 不可表示，保留既有值。

## 最小实现计划

1. `crates/vega_ui/src/conversation_stream/context_usage.rs`：显示类型收敛为「最近调用总量 + 容量 + 预计下次请求」；百分比、填充、紧凑值只由真实总量产出；新增预计下次请求与来源文案；删除被取代的 `ContextUsageSource` 与未使用的 `percent_used`。
2. `crates/vega_ui/src/conversation_stream/context_control.rs`：`last_call_total` 与 `next_request_estimate` / `next_request_source` 分离；`apply_context_accounting` 不再写分子；`apply_context_projection` 恢复真实值且不再用冷估算占位；`apply_context_usage` 只写真实总量；压缩不再清空分子；run 结束只清 live 状态。
3. `crates/vega_ui/src/conversation_stream/render.rs`：无真实总量时不画填充；详情增加「预计下次请求」一行；其余布局与交互不变。
4. `crates/vega_store/src/token_usage.rs`：新增只读 `latest_primary_call_total`（`message_id IS NOT NULL` 的最新一行，checked 加法）。无迁移。
5. `crates/vega_conversation/src/types/context.rs` + `agent/compaction.rs`：`ContextProjection` 增加 `last_call_total`，在同一读快照内查出。
6. `crates/vega/src/window/context.rs`：投影应用时透传恢复值，保持既有 generation/thread/stream/model 栅栏。
7. `crates/vega_i18n/locales/ui.en.toml` / `ui.zh-CN.toml`：新增预计下次请求与来源键，删除被取代的估算键。

## 非目标

- 不改估算算法、不引入 tokenizer 依赖。
- 不改压缩阈值、触发/目标比例、预算判定、`ContextBudget` 语义与 runtime 预检链路。
- 不改 meter、费用累计、`token_usage` 写入路径与 pricing。
- 不改 provider 协议与事件 schema；不新增数据库迁移。
- 不改环尺寸、位置、动画；不做平滑插值掩盖口径切换。
- 不实现 Codex 的 `get_context_remaining` 工具或 context window 重置机制。
- 不声称与 ChatGPT 全机制一致；仅对齐指示器的单一来源、空态与恢复语义。

## 验证与交付边界

- 本地只做改动文件 `rustfmt`、`git diff --check`、`python3 scripts/check_i18n.py`；不运行 Vega、不编译、不跑 Clippy/test/nextest。
- 行为回归按矩阵在开发分支/draft PR 交现有 `pr-check`；开发 Rust 测试留痕（原始源码/patch + 完整 SHA + 实际结果）后在合并前清理，最终无 Rust 测试的 head 经完整 required CI 确认 `no Rust tests`（0 发现/0 执行）。
- 独立审查由非该实现写入者承担；真实模型/服务与 UI 集成由用户在合并后的 master 手测。
- 回滚：撤销本卡生产差异即可，无数据迁移。
