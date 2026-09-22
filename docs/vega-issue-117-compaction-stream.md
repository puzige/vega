# Issue #117 — 对话流中的上下文压缩状态

来源：<https://github.com/puzige/vega/issues/117>，2026-09-22。

## 冻结范围

现状：`context_control` 保留最多六条状态转换，在 transcript 与 Composer 之间的固定 band 展示。用户要求参照其提供的正常运行截图，将压缩作为类似工具调用的简洁状态行放进对话流。

1. 真实压缩开始时，在当前对话流的发生位置插入状态行，参与同一个滚动列表；完成、失败、取消更新同一次操作，不重复显示开始和结束两行。不再渲染 Composer 上方的固定状态 band。
2. 使用现有共享图标、语义颜色、metadata 字号与工具行间距；无气泡、无新操作按钮、无虚构进度。进行中显示“正在压缩上下文…”，成功显示“上下文压缩完成”；失败/取消保留现有有意义的诊断文案。Ready/Unknown 不生成压缩操作行。
3. 后续消息不得把已有状态行挪到新消息之后。多次压缩各保留独立行。流式自动跟随与用户手动离开尾部的行为保持原契约。
4. 保留 thread/model/run/generation 的所有权与过期事件防护、计费不确定性、取消和失败保护。不把状态行变成 assistant 内容或发给模型，不展示摘要正文。
5. 重新打开会话沿用已有 `last_status` 投影恢复最近一次持久化结果。现有 HistoryEntry 没有消息时间，状态的 created_at/source_version 不能证明其在消息内的位置，因此恢复行放在已加载历史尾部，文案明确“上次上下文压缩完成”（或对应失败/取消），不冒充精确历史位置。后续新消息保持在该行之后；异步恢复不得覆盖新实时操作。旧数据不补齐未投影的全部压缩历史。不扩大为压缩算法、阈值、provider 配置或数据库架构重做。

6. 压缩行使用单一共享图标，不随状态换图形。用户提供的参考截图中，进行中与完成态是同一枚括号内三条渐短横线的轮廓图标，即 Lucide 公开图标 `text-select`；状态差异只由既有文案表达，失败态额外使用 `danger` 颜色。该图标来自 Lucide 公开图标库（ISC），与 Vega 现有 24×24、stroke-2、round cap/join 的 inline SVG 惯例一致；`gpui-kit-assets 0.6.0` 未收录该符号，因此按 [`icons.rs`](../../crates/vega_ui/src/icons.rs) 既有缺符号内联约定补入，不新增依赖、不改动其它图标。图标语义不得再从状态派生：旧实现按状态切换 Refresh/Check/Warning/Close，与参考形态不符。

本规格取代 #76 correction 中“固定 band”的展示约定，其余安全与行为契约保持。

## 实现前验收矩阵

| ID | 前置与操作 | 预期 | 层级 |
|---|---|---|---|
| C1 | 对话中真实开始压缩，再完成 | 流内一行从进行中更新为完成；无固定 band | production UI/controller + 原生截图 |
| C2 | 压缩失败或取消 | 同一行显示实际终态，原始消息与计费保护不变 | controller 回归 |
| C3 | 先压缩、继续输出与工具调用、再次压缩 | 行序稳定，每次操作一行，不重复开始/完成 | production stream 回归 |
| C4 | 切换任务/模型，送达旧事件 | 不污染当前会话，不回退已完成状态 | 既有与定向回归 |
| C5 | 重新打开已有压缩记录的会话 | 可恢复记录显示在流内，明确旧数据顺序限制 | 持久化/controller 回归 |
| C6 | 无压缩、Ready/Unknown、用户离开尾部 | 无多余行；不强制抢滚动 | 定向回归 |
| C7 | Light/Dark 与窄窗口 | 状态可读、不溢出、与正文列一致 | 真实原生截图 |
| C8 | 同一会话依次出现进行中、成功、失败/取消 | 四种状态共用同一枚 `text-select` 图标；仅失败态用 `danger`，进行中/成功/取消保持中性；文案仍随状态变化 | production stream 回归 + 原生截图 |

## 计划与分工

主控维护规格、review、集成与真实验收。专用实现代理负责 `vega_ui::conversation_stream` 的列表投影和渲染，必要的 `vega` context controller / `vega_conversation` 历史投影及对应定向测试。无新依赖。先建立可检出旧行为的测试，再实现；交付报告为 `docs/vega-issue-117-compaction-stream-delivery.md`。

验证：`cargo fmt --all -- --check`，受影响 crate 的 context/stream 定向测试与 clippy；云端 PR check。真实模型/native 截图未完成时不得声称 E2E 已通过或关闭 Issue。回滚使用本卡独立提交的 revert。
