# Chat usage 缓存字段优先级：直接修复 v4.1

## 阶段、基线与范围

继续 [#494](https://github.com/puzige/vega/issues/494)，处理 [PR #504](https://github.com/puzige/vega/pull/504) 合并后的用户手测失败。2026-10-20 用户明确要求“需要快速修改 定位到问题 直接修复”，本轮由 Codex 直接组织专职子 agent 修复，替代默认 Ready 交 Vega 的等待点。根因与验收已冻结；尚未实现或手测通过。

生产基线为 `0d21c4ce2947c4f3e3602680d8763e94f5cccf7e`，发布版本 0.1.183。已先 fetch；保护用户主检出的脏改动，在原卡 worktree 切出基于最新 `origin/master` 的 `feat/494-cache-usage-handoff`，原 `feat/494-pi-compat-baseline` 保留。复用 `/Users/puzige/Workspace/worktrees/vega-494-chat-auto-adapt`；专职子 agent 是唯一代码写入者，主会话负责方案、审查、提交、PR/CI/合并。

只修改公共 Chat Completions usage 规范化及本轮开发测试/证据。能力目录、模型识别、思考等级/图片设置、Responses codec、用户配置与数据库均不属于本轮生产修改范围。不添加模型名称或域名特判，不恢复模板、旧配置读取或真实网络 E2E。依赖白名单：沿用现有生产依赖；开发测试可按实际需要恢复原有 `tempfile`，不升级或新增其他包。

约束入口：[AGENTS](../../AGENTS.md)、[执行指南](../vega-exec-guide.md)、[Issue 工作流](../vega-issue-workflow.md)、[Rust 测试生命周期](../vega-rust-test-delivery-lifecycle.md)。云端命令只供现有 Actions 执行，本地不运行 Vega、不编译或测试。

## 根因与证据

用户在 0.1.183 中以 `deepseek-v4.1-flash` 手测：可见回答已输出，约 8 秒后显示 `The provider returned invalid data`。只读确认实际进程来自 `/Applications/Vega.app`，已安装版本为 0.1.183；不是将旧版本失败推定为新版本失败。

对应主请求在 2026-10-20 06:07:51–06:07:59 UTC 记录为 provider/protocol/response，最终 HTTP 200、87 字节可见输出、重试数 2。两次先行 HTTP 502 已重试，不能用它们解释最终 HTTP 200 流的解析错误。对应流有 566 个 JSON 帧和一个 `[DONE]`；565 个中间 `finish_reason: ""`，最后 `finish_reason: "tool_calls"`；两个 index 的调用各有稳定非空 ID、函数名和完整对象参数。没有工具执行诊断阶段。

最后一帧的内容无关 usage 摘要为：

```json
{
  "prompt_tokens": 6721,
  "completion_tokens": 627,
  "prompt_tokens_details": { "cached_tokens": 6528 },
  "prompt_cache_hit_tokens": 6528,
  "prompt_cache_miss_tokens": 193,
  "cached_tokens": 0
}
```

`crates/vega_runtime/src/openai/sse.rs::SseAssembler::absorb` 先调用 `usage_counts`，再解析同帧 delta 和 finish_reason。`usage_counts::compatible` 收集三个缓存读取位置并要求所有非空数字相等。`6528 != 0` 因而在合法工具终态处理前触发 `chat_compat::protocol_error`。工具已积累，但没有向 runtime 释放并执行。最后一帧不能完成，也没有最终 usage/Done。

这是源码与只读真实数据结构共同确认的原因；本地没有执行 Vega 或回放测试。原始日志仅留在原服务，不向公开仓库复制请求、正文、思考、参数、身份、请求头或凭据。源日志 SHA256 为 `07da77b154c4fcbf5b4e7b6ab13810bb0031dd5aabac6c1954b891ae553fa475`，只用于对应本次只读分析，不将哈希当行为测试证据。

固定 pi `b79e4cc834970cca69daebffab7df1da7d1e52c4` 的 [parseChunkUsage](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/src/api/openai-completions.ts#L1499) 使用 nested cached_tokens → prompt_cache_hit_tokens → top-level cached_tokens 的 nullish 优先级，没有要求不同位置相等。Vega 的额外相等约束拒绝了实际可兼容的代理数据。

上一轮 [133 项 GREEN](https://github.com/puzige/vega/pull/504#issuecomment-6283717935) 中 `usage_tracks_total_input_caches_and_progressive_snapshots_without_double_output` 覆盖正常 nested 计数、choice fallback、渐进快照和同帧两个独立 usage 对象冲突；没有本次一个 usage 对象内不同别名不一致的输入。该遗漏如实保留，旧 GREEN 不追认为本次真实服务通过。

## 验收契约

下列验收先于实现冻结。本轮是新 Bug 周期；不得更改旧周期的测试/结果追认成功。

### 公共字段规范化

1. 在一个 usage 对象内，缓存读计数按 `prompt_tokens_details.cached_tokens`、`prompt_cache_hit_tokens`、`cached_tokens` 顺序取第一个非 null 的已验证数字。缺失与 null 跳过；显式 0 是有效选择，不继续查找非零值。不同位置的有效数字不相等不再是协议错误，不求和、不取最大值。
2. 缓存写计数按 `prompt_tokens_details.cache_write_tokens` 优先，保留现有顶层 `cache_write_tokens` 作为次级兼容位置；同样区分 0 与缺失/null。无可用读/写计数时保留 `None`，不凭空制造已知 0。不为本轮加入其他提供方字段。
3. 继续校验所有现有已识别、非 null 计数字段是 u64；负数、分数、字符串、布尔值、对象/数组、超出 u64 的值仍为 protocol 错误，不做类型转换。`prompt_tokens` 和 `completion_tokens` 仍是必需字段；不将 malformed usage 降级为缺失。
4. 继续校验选中的读/写计数不超过 prompt 总量，checked 相加不溢出且不超过 prompt 总量；独立 miss 计数若存在，保留其 u64 和不超过 prompt 的检查。去掉将所选缓存位置与另一个来源的 `prompt_cache_miss_tokens` 强制凑成 prompt 总量的约束；miss 不参与选择，也不覆盖总输入。
5. Vega `ProviderEvent::Usage.input` 保持含缓存的 prompt 总量，output 保持 completion 总量；reasoning_tokens 不再次加入 output。不照抄 pi 扣缓存后的 Usage.input，避免二次扣减。计价、容量与诊断继续消费既有 Vega 语义。
6. 只对同一个 usage 对象内的字段按优先级规范化；同一帧同时含 chunk.usage 和 choice.usage 时，仍按现有规范化后的完整计数一致性要求处理。两个独立对象的实际规范化结果冲突仍拒绝。渐进快照仍由最后一个有效快照覆盖，最终只向 runtime 发出一次 Usage。
7. 保留合法终态要求、工具身份/参数完整性、终态后变更、取消、权限、凭据与失败前零工具效果边界。修复不得靠忽略整个 usage、跳过错误、提前执行工具、伪造 Done 或静默重试已执行工具实现。

### 本轮先行测试矩阵

所有输入都用合成文本、合成 ID、合成参数与自建临时文件；只借用真实失败的字段形状。验证实际 codec、流装配和 runtime，不在替身里预先生成 ProviderEvent 或复制规范化算法。外部 HTTP/模型服务可以用现有进程内替身隔离；文件 Read 使用真实工具与自建文件，禁止执行 Git/Shell/真实模型或恢复真实 E2E。

| 目标 | 前置、操作 | 必须观察到的结果 | 层级 / 预期基线状态 |
| --- | --- | --- | --- |
| U1 真实失败等价流可续工具 | 空 finish 中间帧、思考和可见文本、两个分片 `read` 调用；同一最后帧带 `tool_calls`、合法 delta 及 prompt=100/output=9/nested-hit=70/DS-hit=70/top-hit=0/miss=30，随后 DONE | 原文本和思考各自完整；两个稳定调用的原参数和顺序不变；Usage 为 input=100/output=9/read=70，合法终态恰一次；相同流经真实 runtime 读取两个自建文件各一次，续轮拿到真实工具结果并完成，再续问不重复执行 | SSE + runtime；基线应因别名不等失败，构成本轮行为 RED |
| U2 读取位置与显式零 | 表行覆盖 nested=70/DS=60/top=0；nested=0/DS=70/top=60；nested=null/DS=70/top=0；nested 缺失/DS=null/top=70；全部缺失/null；包含非选中但合法的 miss 数字 | 分别选 70、0、70、70、None；与低优先级数字及 miss 不一致不会改所选值或拒绝流 | SSE；冲突正向行基线 RED |
| U3 写入位置与读写会计 | nested-write=30/top-write=0，nested-write=0/top-write=30，nested=null/top-write=30，以及都缺失/null；读=50/prompt=100/output=5/reasoning=3 | 分别选 30、0、30、None；input 始终 100，output 始终 5；读写单列不相互扣减或重复增加 | SSE；冲突正向行基线 RED，其余保护已有行为 |
| U4 全部支持计数仍验证 | 对已选和被低优先级遮蔽的位置分别放负数、分数、字符串、布尔值、对象/数组或 u64 溢出；必需总量缺失/null；selected-cache > prompt、selected-read+write > prompt/溢出、miss > prompt | typed provider/protocol 错误，无 Done；即使之前已有合法工具碎片，真实 runtime 仍零工具执行，不吞错误或修补参数 | SSE + runtime；保护合法数据与错误数据的边界 |
| U5 位置与快照组合 | 相同 payload 分别在 chunk.usage 与 choice.usage；两个位置同时出现且规范化值相同；中间快照再到最终快照 | 同等计数和终态；最终 snapshot 优先、Usage 仅一次，不双重计费；低优先级镜像差异不制造两个不同结果 | SSE；混合别名行应 RED |
| U6 独立对象冲突仍拒绝 | 同帧两个 usage 对象规范化后 prompt/output/选中缓存实际不同 | 原有 protocol 失败与零工具执行不变；不能把 pi 的位置优先级借机用于吞掉本轮未授权的对象冲突 | SSE + runtime；应保持已有拒绝 |
| U7 正常流及终态/工具边界 | 普通无 usage 文本，单一合法缓存字段，合法非零缓存写入，null/空 finish 中间帧；缺真实 finish、未知非空 finish、无效最终 JSON、跨 index 身份冲突、真实函数名变更、终态后变更、取消 | 正向保持文本/真实调用/终态；负向失败或取消且零工具效果；不靠扩大 parser 宽容范围解决 usage 缺口 | 按现有公开边界选择最少必要回归；保护范围，不将基线已通过的负例冒充新 RED |

每个表行在冻结清单列出具体 Rust 测试名、fixture、输入和结果，不以此表 7 行冒称 7 个执行数。U1 至少包含真实 runtime 续轮；不能只断言私有 helper 返回 Some(70) 或只检查 mock 被调用。U4/U6 的错误断言应核对 typed failure、无成功终态与执行效果，不能只 `is_err`。

## 具体实现计划与交接动作

1. **专职子 agent 先写测试，生产不改。** 在当前生产基线新建 `crates/vega_runtime/src/openai/chat_usage_tests.rs` 或同等任务局部模块，并仅添加必要 `#[cfg(test)]` 接线。从固定来源逐项选取必要 fixture/helper，不整目录恢复旧 133 项集合。来源参照以下固定文件，保留原含义并记录本轮新增输入；不把真实失败输入称为 pi 原有用例：
   - [Vega v4 stream 回归](https://github.com/puzige/vega/blob/7d1066901e9f488cc81e825bb035df6dea7936ee/crates/vega_runtime/src/openai/compatibility_tests/stream.rs)。
   - [Vega v4 codec/runtime 续轮支持](https://github.com/puzige/vega/blob/7d1066901e9f488cc81e825bb035df6dea7936ee/crates/vega_runtime/src/openai/compatibility_tests/lifecycle.rs)及该提交的 `mod.rs`/HTTP 支持，只提取本轮需要的真实调用边界。
   - [pi 缓存读写用例](https://github.com/earendil-works/pi/blob/b79e4cc834970cca69daebffab7df1da7d1e52c4/packages/ai/test/openai-completions-tool-choice.test.ts#L1567)：Vega 总输入语义按上文换算，保留原 output、读/写分开及不双计思考的断言。
2. 写齐 U1–U7 所需用例和冻结映射后，子 agent 交回 Codex并停止写入。主会话审查、提交测试/必要支持，记录生产未改的完整基线、完整测试提交 SHA 与各文件 SHA256，建立新的 draft PR；PR #504 已合并，不能复用成执行 PR。
3. **等待有效 RED。** 仅由现有 `.github/workflows/pr-check.yml` 在新 draft PR 执行。Actions 中先 `cargo nextest list --workspace --message-format json`，再按真实非空集合 `cargo nextest run --workspace`；同时维持 fmt/全目标 Clippy 与原三组 Python 检查。不增加 dispatch、脚本或本地测试入口。预期 U1/U2/U3/U5 正向差异在 `usage_counts` 相等约束失败；无关编译、网络、runner 失败不算 RED。Codex 记录原始 job/失败与准确发现/执行数、冻结测试后，明确授权子 agent 才可生产实现。
4. **最小生产实现只改 `openai/sse.rs::usage_counts`。** 将 `compatible` 的全等合并换为按上文顺序选择第一个非 null 的已验证 u64。继续使用实际 number 校验，保留必需总量和 selected-read/write/独立 miss 的范围检查；移除跨来源 alias 全等和所选 read+miss 必须分割 prompt 的假设。保持 `UsageCounts`/事件类型、总输入语义、`SseAssembler::absorb` 的两个独立 usage 对象冲突检查、快照与 finalize 发出顺序不变。不要改模型 resolver、方言、网络重试或 Agent 执行判断。若实际需要修改其他生产符号，先在 Issue 交回 Codex修订，不自行扩大实现。
5. 子 agent 完成后停写，由 Codex审查并提交，以同一冻结测试取得云端 GREEN，附实现 SHA、原始 job/计数与差异。本地只做 fmt/diff/静态检查，不执行编译或测试，不改断言或补测追认。Codex 固定可访问的源码/原始补丁、完整 SHA 与 RED/GREEN，再明确交独立删测；删除本轮 Rust 测试、独占 helper/fixture/dev 接线，生产行为不得改变。
6. Codex 审查独立删测及最终最新 head 的完整 required check；确认 0 发现/0 执行并明确 `no Rust tests` 后才转正式 PR、squash merge。Issue 再回 In review 等用户纯手动 DeepSeek/Step 及普通 Chat 工具续轮，不提前关闭或 Done。

CI 每 5 分钟查询，通知/用户即时要求除外；失败看原始日志，不自动重试换绿。继承 v4 来源/许可与[来源映射](vega-issue-494-pi-source-cases.json)，本轮不能声称整套 pi 或真实 758 个模型通过。

## 用户手测与回滚

在包含后续修复 PR 的发布版本中，原代理和模型选择保持不变，以 DeepSeek 连续调用文件工具并续问；确认任务完成、结果可被下一轮使用且无重复执行。再用 Step 与普通 Chat 模型验证同类续轮。输入框等级仍只影响真实请求选择；本轮不改设置或要求用户重新配置模板。

未取得新修复的 Actions RED/GREEN、未发生生产修改、没有新发布或真实服务验收时，报告均明确为待办。回滚走修复 PR 的独立 revert；不修改运行时用户数据、不恢复旧配置、不回退 PR #504 的模型能力编辑。

## 执行回报约定

Issue 494 为唯一回报入口。用户本次直接修复授权下当前执行者为 Codex，状态 In progress，不转 Ready 等待 Vega。专职子 agent 测试写齐、实现完成或遇阻均交回主会话并停止写入；由主会话审查、集成和发布证据。新的验收缺口或 Bug 停止本轮，由 Codex另立完整测试先行周期，不能给实现后的代码追加通过证明。
