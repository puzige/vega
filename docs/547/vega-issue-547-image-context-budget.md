# 图片预算与首轮压缩决策

- Issue: #547；阶段：验收与先行测试准备，尚未实现。
- 规格版本：v1.1，2026-10-22；生产未改/测试尚未冻结，边界审查补充历史图片与旧 checkpoint 兼容性、精确初始形态及 preflight 取消。
- 事件版本：v0.1.199，`83fd51d79940516029cdc1eaf8e81566870c877b`。
- 开发基线：`871f91f246e90753743a256cee1eec315e74effc`，分支 `feat/547-image-context-budget`；已 fetch/快进，无本卡生产修改。
- 工作流：[执行指南](../vega-exec-guide.md)、[Rust 测试生命周期](../vega-rust-test-delivery-lifecycle.md)。本地只做静态/格式检查；测试、编译、Clippy、完整门禁仅在现有 Actions。

## 1. 问题与边界

新会话已经保存首条用户消息和两张图片，但尚未发送主模型请求，发送前估算达到 378232 tokens，超过 300000 的输入预算。自动摘要找不到旧历史，返回 `NoCompactablePrefix`。这是本地预算/决策问题，不是 provider 拒绝或图片解码失败。

用户图片估算目前同时计入编码字节、像素和固定余量。两张 PNG 为 654126 / 636742 bytes，尺寸 2808×1720 / 2936×1878；图片估算合计 365172。仅第二张图的对照请求成功，provider 报整次输入 13021 tokens。这不是单图真实视觉 token 的测量，不作为 provider 通用公式。

只修复图片估算和明确无历史的首轮发送决策。不涉及 #543、用户配置、上限提高、图片重编码/缩放、UI 重设计或新 provider 特性。公开材料不包含原截图、原对话、数据库、服务端记录或敏感本地路径。

## 2. 验收矩阵（先于实现）

| ID | 输入/边界 | 必须观察到的行为 | 验证层 |
|---|---|---|---|
| A1 | 相同像素、不同合法编码体积的图片 | chat/wire 两条估算路径的图片增量相同；估算为正且随几何尺寸增长；附件字节不变 | 公开估算 API + 合成图片 |
| A2 | 上述两种尺寸的合成普通截图，首条用户消息，输入预算 300000 | 返回正常 provider 文本；原附件完整到达 provider；没有摘要开始/失败事件、没有摘要调用 | 真实 runtime loop，替换网络边界 |
| A3 | 只有当前用户输入，无旧历史；自动压缩启用，估算在触发阈值至硬上限之间（含等于硬上限） | 正常发送；不要求存在 compaction hook；不产生无意义摘要状态 | runtime loop |
| A4 | 同一无历史形态，真实估算高于硬上限；自动压缩开/关 | 返回 `ContextRuntimeError::OverLimit`，包含实际估算与预算；provider/摘要均未调用 | runtime loop |
| A5 | 有完整旧历史，达到自动压缩阈值 | 仍经过摘要边界，使用压缩后的投影发送，保留当前用户输入 | runtime loop + 外部 hook 替身 |
| A6 | 首轮已产生完成的 assistant/tool 组（含多组），没有第二条用户消息 | 不因最新 User 位于开头而禁用原有压缩路径；完整工具组和保留后缀仍进入 hook/后续 provider | runtime loop，工具结果预构造，不执行外部命令 |
| A7 | 初始用户请求收到 provider 明确 context overflow，即使本地估算未超限 | 保留失败，不再次发送相同输入，不尝试空历史摘要；有可压缩历史时原有单次恢复仍可用 | provider 故障注入 + 可见错误/最终结果 |
| A8 | 有历史的摘要失败/取消/保护错误 | 真实超限不绕过；阈值内可恢复失败遵守现有策略；SourceChanged 等保护错误不被吞；取消不继续调用 provider | runtime loop + hook 故障注入 |
| A9 | 图片字节/数量/格式/角色限制 | 原有 8MiB 单图、16MiB 每轮、32MiB 历史、4 张/轮，以及 tool 图片与角色限制不放宽；至少覆盖代表性超限与合法对照 | ImageAttachment/公开 runtime 入口 |
| A10 | 超限错误映射到用户提示 | 明确可减少当前输入或移除图片再重试；不再只建议新建会话或增大上限；保存内容的事实保持准确 | conversation 错误映射 |
| A11 | v2 checkpoint 重载，带不同大小历史图片组 | 旧摘要仍可读且持久化字段不改写；历史图片按当前估算受 64k 预算约束，保留完整组、连续最新组与时间顺序，历史不可信标记不丢失 | conversation 真实历史投影/内存 store 测试，不扩大 public API |

测试在生产未改时一次写齐，独立审查后才提交 draft PR 取得有效 RED。mock 仅隔离网络/摘要服务，不替代 runtime 预算决策；断言绑定结果、投影、事件与错误，不能只有调用次数。测试目标、数据、断言在 RED 后冻结；遗漏边界先更新规格并重开周期。所有图片为代码构造的合成 fixture，经真实 `ImageAttachment::from_bytes` 校验，不增加生产测试 API，不恢复历史测试集合。

## 3. 估算契约

用户图片采用现有几何项加固定余量：`1024 + ceil(width × height / 256)`。移除 `ceil(encoded_bytes / 4)`，不替换为 base64 字符数。tool 观察图片保持现有 `1280 + ceil(width × height / 256)`，不顺手变更其既有语义。

这是保守的 provider-independent 本地近似，而非精确视觉计费。字节/格式/解码限制由 `images.rs` 继续负责；估算与附件验证职责分开。保持 checked arithmetic、图片数、非图片估算、安全余量、后续真实 usage 校准和请求输出 reserve。

算术核对：

| width×height | pixels | ceil(pixels/256) | 用户图片估算 |
|---|---:|---:|---:|
| 2808×1720 | 4829760 | 18867 | 19891 |
| 2936×1878 | 5513808 | 21539 | 22563 |
| 1×1 | 1 | 1 | 1025 |
| 4000×4000 | 16000000 | 62500 | 63524 |

事件两张图片合计为 42454，而不是 365172。其他输入保持相同的条件下总估算为 `378232 - 365172 + 42454 = 55514`；该值仅为事件数据的静态算术，不冒充修复后运行结果。300000 是输入预算；128000 reserve 不再次从它扣除。

估算器版本变为 `vega-context-estimator-v3`，未来 checkpoint 记录新版本。既有持久化摘要仍是有效历史，不删除、不重写、不因公式变化丢弃；读投影时使用当前估算。当前版本字段参与 checkpoint/replay 指纹，不是旧摘要不可读的版本门禁；不增加迁移或跨版本恢复策略。

## 4. 无历史决策与安全边界

在 runtime 组装的消息向量中，首项为系统消息。仅对能证明是初始输入的形态提前排除自动摘要：恰好 `[System, User]`，没有 tool calls/result metadata，没有额外历史/摘要/assistant/tool。无 User 或其他非典型投影不在本次判定内。不得仅凭 latest User 索引为 0/1、用户消息数量为 1 或没有完成的 assistant 最终文本判断不可压缩。

`PrimaryPreflight` 事件送达会 await sink；若 sink 期间发生取消，必须先返回 Interrupted，不能让新分支将取消变成 OverLimit 或继续 provider。此观察点需在测试冻结前覆盖。

- 预算内/达到触发阈值但未超过硬上限：继续普通请求，不发摘要 Started/Failed，不要求摘要 hook。
- 高于硬上限：直接返回既有 typed OverLimit，不调用 provider 或摘要。自动压缩关闭也保持硬限制。
- provider 明确拒绝初始输入：保留原错误/失败结果，在两个 overflow recovery 入口避免设置 force flag，不进入无法改变请求的空历史压缩重试；覆盖建立流失败及流内错误路径。不得把本地估算低于预算解释为可以无条件重发。若已进入其他形态的 forced-compaction 分支，不在该分支应用普通自动压缩跳过逻辑；保留强制摘要必须减少投影、满足预算和禁止不变重发的保护。
- 存在旧消息、assistant/tool 进展或其他复杂投影：保持原有 hook、source fence、completed tool boundary、attempt cap、取消和不可恢复失败保护。hook 是持久化边界的权威；本次不复制 conversation 的完整切分算法到 runtime。
- 有历史的 provider overflow 恢复仍只允许原有的一次受保护压缩；不增加通用重试。

用户提示复用既有 `ContextOverLimit` 类型，在失败位置提供减少当前消息/移除图片后重试的动作，同时保留长任务可缩小工具查询或开始新会话的选择。不加常驻解释、成功提示或配置自动调整。

## 5. 最小实现计划（须先取得有效 RED）

1. `crates/vega_runtime/src/context.rs`：修改用户图片贡献函数，保留 tool 分支；移除过时且解释旧公式的局部注释；更新版本常量。冷热估算共享同一图片路径，不复制公式。
2. `crates/vega_runtime/src/agent/loop_.rs`：私有、明确命名的初始输入判定；在 primary preflight 和两个 provider-overflow recovery 入口应用。保持既有取消、保护校验和正常压缩分支次序。
3. `crates/vega_conversation/src/types/events.rs`：只修改超限恢复提示，不新增错误分类/数据库状态。
4. 开发测试仅使用现有依赖；必要 crate 内测试接线允许错误提示映射和 A11 的真实历史投影。A11 使用内存 SQLite 经现有 checkpoint 接口存取/读回 v2 记录，再调用现有 projection 边界；合成 ContextSource 可用于选择顺序/完整组断言，但仅借用不可变 checkpoint 对象不冒充数据库重载验证。生产无新 public API、无新依赖、无新增代码注释。

## 6. 交付证据与手测

主 Agent 统一 Git/index/PR 操作；专职子 Agent 是唯一代码写入者；非写入者独立审查。测试完成停止写入后绑定精确内容版本审查；有效 RED 后最小实现，同一测试 GREEN。PR 保存原始测试源码/patch、完整 SHA、RED/GREEN run/job 与准确计数，随后独立删测提交，最终现有完整门禁必须明确 `no Rust tests`，不能用空集冒充带测试 GREEN。

现有 CI：fmt、workspace/all-targets Clippy、三组 Python 检查、nextest list/run，required check `check (fmt, clippy, test)`；不改 workflow。每五分钟查询，失败先归因，禁止自动重试换绿。

合并后 #547 保持 OPEN，状态 In review，等待用户手测：新会话首条消息附两张普通截图，应直接开始模型响应；适当缩小输入预算可看到准确超限提示且消息/附件保留；有长历史的正常自动压缩不退化。代码合并不等于本机应用已更新，不打断用户正在运行的任务。回滚采用回退本卡合并变更，不迁移/删除用户数据，不提高配置上限。
