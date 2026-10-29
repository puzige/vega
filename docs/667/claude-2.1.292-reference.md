# Claude Code 2.1.292：压缩行为矩阵

> #667 的版本绑定参考矩阵。仅静态合同，不是 Vega 实现或运行验证；覆盖已定位的客户端压缩路径，不声称知道全部私有服务端实现。Vega 目标、验收和差异见 [任务规格](vega-issue-667-claude-compaction.md)。

## 1. 来源、读法与关键结论

- 来源版本：**2.1.292**；记录的 binary SHA256：`97a01e5bc74a199e67189435d0331ea3a24eac2e07db4b76d9148c5b0386138f`。E59 内含版本、构建时间 `2026-10-06T05:25:12Z` 和 `darwin` 标识。
- 取证读取 E01–E62 全部 62 份有界片段；整段提取不入公开仓库。主 Agent 已重新计算 binary SHA256，并核对有记录的 binary 范围，修正了 strings 变换造成的 8 项整段匹配误标。
- 定位规则：E 编号对应 [脱敏证据索引](claude-2.1.292-evidence.json) 的完整 id。strings 行从 1 起，binary byte offset 从 0 起、右端不含；索引明确区分整段匹配、仅短前缀匹配与补充语义锚点。
- **S＝静态确证**：片段直接支持的控制流、数据变化或算式，不代表运行成功。**F＝feature／平台／配置条件**：只在所列条件下成立，代码中的 fallback 值不等于实际 rollout。**U＝未知／未证**：现有片段没有证明；不为填空继续调查。

关键区别：

1. **手动 `/compact` 走 `rVr` 或 precompute 复用，通常保留尾部；`$Qn` 的 `messagesToKeep: []` 只属于 full 分支。** partial、controller、in-process teammate 也须分开建模。
2. **非 remote 的 `hV()` 无额外 feature gate**；在窗口来源为 `auto`、无显式模型覆盖时，前置 auto 阈值路径让 reactive 接管，不等于关闭压缩。
3. **precompute 是有身份约束的状态机**：普通消费移除，fork 借用不移除；取消前台等待不取消后台；所见 `model_switch` 函数保留 ready。
4. **idle 会话级压缩独立于 micro**：围绕 1h cache 到期调度，配置兜底为 off；不能把它等同于 `$Qn`，也不能把 server micro 的 3900 秒阈值套给它。
5. **UUID／anchor 重接和旧 assistant usage 清零用于重建、估算**；不是删除历史账单。文件恢复超过 token cap 会退为引用，不保证恢复全文。

## 2. Trigger、窗口与预算

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E59 | 来源版本 | 片段声明 VERSION=2.1.292，带构建时间与 darwin 标识。 | **U** 此片段在原索引至少匹配两处，不能当唯一代码位置；也不能据此推断所有平台运行行为。 |
| E19 | 全局 auto gate | `DISABLE_COMPACT` 或 `DISABLE_AUTO_COMPACT` 使 `nm()` 为 false；否则读 `autoCompactEnabled`，fallback true。`hV()` 在非 remote 返回 true。 | **F** remote 才使用 latched `tengu_reactive_compact_remote`，fallback false。`idleCompaction` 是另一设置，fallback true。**U** 不把这些 auto gate 推广为所有手动入口的禁用条件。 |
| E01、E39 | 窗口选择 | 有效 env window → settings（按模型／默认）→ clientdata → experiment → model-default／unknown-model → auto；有效配置受模型 contextWindow 上限约束。 | **F** 部分分支依赖 auto 开关、入口、模型识别、credits／原生大窗口能力；非统一固定窗口。**U** 当前账户的模型表、clientdata、experiment 实值。 |
| E01 | 阈值算式 | 设 `W = window − min(maxOutputTokens, 20000)`，通常压缩阈值 `A = W − 13000`；有效 pct override 时 `A = min(floor(W×pct/100), W−13000)`。判定优先 blocked，再 compact，再 warn；auto 开启时 warn 为 `A−20000`，关闭时基准为 W。通常 blocked 为真实模型可用窗口减 3000，可被 blocking override 覆盖。 | **S** compact 阈值与 blocking 上限不是同一个数，配置窗口不自动等于硬阻塞窗口。**U** 实际选用的模型参数与 override。 |
| E01、E44 | precompute arm 预算 | arm 阈值 `min(W−round(W×f), A)`；f 的 scalar fallback 为 0.2，支持按窗口精确项／default、REPL／SDK 的表；非法表回退 scalar。 | **F** 还须通过 `IN()`；非 `auto` 来源另检查 `window ≥ N5` 并可加入 server-cleared token 补偿。**U** N5 定义未在这些片段展开，不硬填数值。 |
| E39、E56、E08 | token 估算及固定前缀 | `tm` 用最近有效 usage anchor 加后续消息估算；遇 compact boundary 从该处重新估算。`GP` 获取最近合适响应的 server-cleared input tokens，boundary 后不再沿用旧值。auto 判定使用 `tm − snipFreed + GP`。固定前缀估算约为 usage input/cache 总量减消息估算、减 snip。 | **S** E08 的 prefix overflow 分支只记录“压缩不能帮助”的诊断，所见代码没有因此直接 return。**U** 未校准估算误差，也不把估算当实际账单。 |
| E08、E09、E19 | auto 分流 | 排除 `compact`、部分辅助源（prompt_suggestion、away_summary、agent_summary、hook_prompt）及特定大窗口 fork；`hV && source=auto && 无显式模型覆盖` 时不作前置阈值压缩。有指定窗口、classifier overflow 或模型覆盖且 reactive 可用时路由 `TQn`，否则可进入 `$Qn`。 | **F** classifier overflow 有 epoch／permission-mode stale 检查。可先用上一较大窗口模型作一次受限救援；受模型、策略及 `tengu_tidy_axolotl` 等条件约束。**U** 不是每个阈值命中都发出摘要请求。 |

## 3. Micro 与请求级清理

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E02、E20 | 本地 keep-recent micro | `Mrs` 只清候选工具结果，保留最近 5 个候选 tool-use（调用方值；底层至少留 1 个）；估计可省不足 20000 tokens 则不做。可持久化旧结果并替换为引用，失败则占位；含 image/document 的结果用清除占位，不保留其媒体正文。既有清除／persisted 标记避免重复计为候选。 | **S** 这里的遥测虽名为 time-based，实际所见调用 trigger 是 `context_hint`。**U** 未证明另有通用“闲置后本地清理全部结果”的入口；候选工具是代码集合，不等于全部工具。 |
| E20、E45 | context-hint 拒绝／降级 | provider 创建 hint controller；可清量达到 20000 才带 hint body，target fallback 75000。422／424 拒绝及特定无 status 的 stream invalid-request 可触发上述 micro；400 beta 不兼容、409 busy、529 等走去 hint／回退，不都执行清理。controller 内会关闭后续 hint 尝试。 | **F** 要 first-party betas、`repl_main_thread*` 来源及 `tengu_hazel_osprey`（fallback false）。**U** 不推断其他 provider 也支持此合同。 |
| E02、E35、E54 | server idle micro | 发送 `clear_tool_uses_20250919` edit；闲置门槛 3900 秒，keep=5，clear_at_least=20000；trigger 从可清结果数量减 3 得到，并至少为 20。排除不合适的工具名／标记结果，使用时间戳、响应边界、idle 前后回复和共享活动信息。 | **F** `tengu_zany_pike` 有 on／shadow／off／unknown；shadow 不发送，unknown 需已有服务端清理证据；还需模型 beta 能力。首次返回与后续返回判断不同，auxiliary 还看相关清理记录。E54 的 env context-management 表达式被常量 false 屏蔽，不能仅凭 env 名称宣称可开启。 |
| E35、E56、E40 | 清理状态应用 | 服务端报告读取 applied_edits；清除 token 数供估算补偿，不是本地正文都被重写的证明。`hint_clears` 则确实按 ID 改写 live messages，并删除相关 Read 的 readFileState；非法路径不盲删。 | **U** 两种事件不可混为同一种持久化机制；服务器真实清理结果未执行验证。 |
| E39、E21、E19、E57 | thinking／媒体／cold 清理 | context-management helper 可构造 `clear_thinking`、`keep: all`；保留尾部的 thinking strip 由 marker 表达，取决于 env／thinking 类型／flag。媒体 strip 将图片、文档换成占位。cold full 还过滤非必要附件、去 thinking、缩短 tool input/result 字符串（100 chars）。 | **F** cold 入口为 `CLAUDE_CODE_COLD_COMPACT`；thinking strip 与 server micro 不是同一个开关。**U** helper 存在不代表每次请求都发送；这些清理也不等于完成会话摘要。 |

## 4. Full、partial、手动及 controller 入口

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E03、E05、E06、E21 | `$Qn` full | PreCompact → 摘要请求 → 清 read/memory 状态 → 恢复附件／SessionStart → boundary＋summary → PostCompact。返回 `messagesToKeep: []`。PTL 可头截断再试，`y3=3`；共享 API attempts 归零也会阻止继续。空摘要／API 错误不是成功。 | **F** `tengu_compact_cache_prefix` fallback true；cold 禁 cache-prefix 路。**U** E03 原始索引 unmapped，已用旧有界 full 片段补证；不能由此概括 `/compact`。 |
| E05、E06、E36 | full 请求两路 | 优先 cache-safe fork：`maxTurns: 1`、工具调用拒绝、skipTranscript、skipCacheWrite；无可用文本／异常可回退 standalone 摘要请求，后者禁 prompt caching、可剥离媒体，并有模型／拒绝 fallback 处理。 | **S** fork 继承 prefix、工具及上下文参数，并保护工具集合不被无声明地替换。standalone 仍可能携带 Read／MCP 等 schema，不可把工具拒绝概括为“请求没有工具定义”。**U** `maxTurns:1` 不限制整个压缩链只请求一次。 |
| E25、E24 | 手动 `/compact` | 空消息和模型已结束会话先拒绝；执行 function hook 与 PreCompact。无自定义指令、无 hook 新指令时尝试 precompute；命中则 `RQn`／replacement，未命中则 **`rVr`**。正常 fresh 路保留尾部，最终统一清理／应用结果。 | **S** manual 的 `trigger` 标签不决定必须走 full。手动 reuse 对 hook 改写、覆盖边界和 fit 仍重新检查；pending 可等待，取消按取消返回。 |
| E03、E04、E27、E34 | partial selector | `up_to` 摘要选中点之前、保留后缀；`from` 摘要选中点起、保留前缀，但摘要请求可见整段上下文。`vqo` 应用顺序分别为 summary→kept 或 kept→summary。metadata 记录方向、用户反馈、保留 UUID／anchor。 | **S** 不在 active context 的选择被拒绝；空摘要范围报错；带独立 Pre/Post hooks 和 PTL retry。保留 assistant usage 清零。**F** thinking/advisor 标记及 fullscreen 应用分支不同。 |
| E28、E47 | controller／plugin／idle 共用入口 | thin client、外部驱动 turn、active guard 会阻止；取得 guard generation 后调用 `compactManually`，不是直接 `$Qn`。有 origin 时用 plugin，否则 auto trigger；应用前检查是否已开始更新的 turn，保留执行期间新到消息，finally 释放自己的 guard/controller。 | **F** fullscreen 可保留旧显示历史，普通模式替换为新 active transcript。**U** 不能将显示历史仍存在等同于模型仍收到旧上下文。 |
| E52 | in-process teammate full | history 超过 `Pse` 时直接 `$Qn(..., isAutoCompact:true)`；替换 active history。hook block 可继续未压缩，取消退出，其他错误抛出。 | **S** 这是独立可见 full 入口，不能把主 query 的全部 reactive gate／breaker 自动移植到这里。 |

## 5. Reactive、重试与 last resort

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E15、E16、E22 | 分组与摘要请求 | `uWt/gW` 去 progress，按 assistant message.id 等边界分组，不按单条消息盲切；不足 2 组不压缩。round 路初始保留 1 组，可由 initial token gap 提前多留。每次摘要 fork 最多 1 turn，返回区分 PTL、media、API error、abort、空文本。 | **S** 摘要范围缩小与尾部保留是同一分割的两侧；“多留组”用于让摘要请求装得下，不保证最终一定低于阈值。 |
| E16、E21、E57 | round retry／last resort | PTL 有 gap 时按组估算引导步长，无 gap 时逐步多留；media-size 错误可 strip 后重试一次。round 走尽后可 summarize-all，保留尚未处理的尾部用户请求，并从头丢组；无 gap 时每次丢约 20%（至少 1 组、至少留 1 组），有 gap 时按估算丢足。最多 3 次 head truncation，必要时加截断标记。 | **F** last resort 受 `allowFallback` 及 `tengu_hidden_frog`（fallback true）控制；precompute 显式禁此 fallback。有条件先单独摘要 opening round；其无 summary tag／错误结果可被丢弃后再头截断。**U** “3”不是整个 round 链的总请求上限。 |
| E16、E17、E30、E37 | reactive 入口／deeper-pass | `TQn` 要 auto enabled、`hV`、未 abort、未被 prior-attempt gate 阻止；`compact` 源排除，部分辅助源仅在有 precomputed swap 时例外。主 query 可在 withheld PTL／media error 时调用。首次非 precomputed PTL 成功且重建后至少 3 组时，可提供一次 deeper-pass；只在紧邻 `reactive_compact_retry` 再次 PTL 的条件下使用，之后 spent。 | **S** 不是“一次请求”也不是“无限 reactive 重试”。precomputed swap 不等同于已尝试 fresh reactive。**F** media 路禁 last-resort；threshold 路是否允许 last-resort 还看 blocked／classifier overflow。 |
| E17、E18、E21、E38 | 成功重建／失败 | precomputed 先再验覆盖与 hook 改写；fresh 走 `rVr→dhe`。正常重建为 boundary、summary、kept tail、attachments、hook results；保留 metadata、重新估算 postTokens、执行 PostCompact 和公共清理。附件恢复失败时 reactive 有最小 plan／goal 等回退。 | **S** skipped、hookBlocked、failed、aborted 和 compacted 不混为一态；取消成功响应后的收尾也可将它转为 aborted。**U** 最小回退不保证所有附件恢复。 |

## 6. Precompute 与 idle

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E10、E29、E44、E46 | arm 时机 | 可在估算阶段或 API usage 返回后 arm；auto 已运行、已试 reactive、pre-first-compact fork、紧随 precomputed swap 等会抑制。SDK 单 prompt 且 history 未改写会拒绝；subagent 估算入口不 arm，API-response 入口又排除 final-turn。同 agent 有非 failed 记录时不重复启动。 | **F** `IN = nm && hV && tengu_sepia_moth && precomputeCompactionEnabled`；flag fallback false，setting fallback true。当前 rollout 未知。 |
| E10、E12 | pending→ready／failed | 后台独立 AbortController；关前台 progress/retry callbacks。`Ehe` 只有当前条目仍为 pending 且 controller 身份相同才提交结果／删除；旧异步完成不能覆盖新条目。hook skip 可按当前用户 prompt 记 veto，replacement 可直接 ready。 | **S** `dhe` 的 `allowFallback:false`，不执行 last-resort 头截断。连续 3 次 counted failure 限制 re-arm；abort／too-few-groups 不计，成功清计数。**U** 这不等于所有 provider/model fallback 都关闭。 |
| E11、E24 | 普通消费／fork 借用 | 普通 `pVr` 等 pending 后移除条目，并调用 sidecar cleanup helper，再返回 ready／failed；fork `gar` 等待、读取 ready，**不移除**。前台 wait abort 返回 turn_aborted 并保留后台条目；不会因此 abort 后台 controller。 | **S** threshold、withheld PTL、manual 有不同入口；借用失败还可能再尝试本 agent 普通消费。**U** 不承诺所有任意 fork 都可借用。 |
| E12、E17、E24、E44 | 覆盖／fit／hook CAS | 检查 boundary UUID、covered row UUID、kept UUID、旧区域是否有 unseen assistant，以及 hook 后 row-prefix／boundary／新指令；成功将未覆盖 tail 拼回。非 auto 窗口来源还做 fit：估算移除旧覆盖后加 summary／新保留消息；`postSwap≥threshold && madeFrom<window` 才因 would_not_fit 让路。 | **S** 不是无条件“所有 ready 都可用”，也不是纯 model 名称比较。**U** 此处是 UUID／row 结构合同，不是逐字节内容哈希 CAS；不额外假设深层对象不可变性。 |
| E14、E11 | 失效／model switch | 公共失效路径可 abort、remove、清 sidecar，并在 subagent exit 清遥测。所见 `sbn(model_switch)` 对非 ready（含 pending／failed）abort/remove，**ready 直接保留**；普通 post-compact cleanup 则会失效当前条目。 | **U** 只证明该 model-switch 函数的行为，不声称系统其他位置绝不会因模型变化拒绝 ready。 |
| E31、E28 | idle 会话级压缩 | 根据 1h cache 的 lastActivity／expiresAt 约 90% 处定时；检查仍 warm、调度延迟不超过 60s、prefix 未变、无更新请求、token 下限、usage allowance。host 再检查用户在场、busy、其他进程持有、transcript 是否前进；持有者探测失败保守拒绝。调用 controller，API attempts budget=1。 | **F** `tengu_sunny_locket` 有 off／log／compact；缺失／无效的 **配置 fallback 为 off**。默认 min=200000，可配置但不低于 100000；fraction 限在 0.5–0.95。还依赖 auto 与 idle setting。**U** presence 阈值 DQt 未见定义；“fallback off”不是这里已证明所有模型 fallback 禁用。 |

## 7. Sidecar、附件恢复与持久化

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E09、E10、E12、E44 | precompute sidecar | 名为 `precompact.json`，兼容目录内及旧 suffix 路径；schema version=3，读写上限 8000000 bytes，writer 传 mode 0600；sidecar I/O 排队串行。schema 验 main agent key、字符串字段、有限数值、非空 summaryMessages、字符串数组 preserveUuids 等。 | **F** 成功 ready 的 main、sidecar gate、loggable boundary 等条件成立才排队保存；`Che` 要 `tengu_amber_packet`（fallback false）且 persistence 未禁。**U** 这是轻量形状验证，不是完整嵌套内容验证、签名或加密证明；未证明底层写入的原子性／崩溃一致性。 |
| E13、E44 | rehydrate | main、当前 session、每 session 至多一次尝试。拒绝 session mismatch、无效时间、年龄超过 604800000ms（7 天）、boundary 缺失、上下文缩小超过原 preTokens 一半、preserve UUID 缺失、rows 不可读。通过后恢复 ready，重建 coverage；存储分支可 read-ahead，并校验 transcript 路径在允许根内。 | **S** **modelMatch／cliVersionMatch 在此仅用于遥测，不是相等硬 gate**；后续消费仍有覆盖／fit 检查。**F** read-ahead 不用于 fork session。**U** 不从恢复处的宽松版本检查推导跨版本保证。 |
| E07、E50、E57 | 文件／skill 恢复 | 清 readFileState／memory selector 前快照；按最近时间最多取 5 文件，避开部分已有／计划／记忆等文件；每文件 token cap=5000，文件附件合计 cap=50000。permission／路径准入先检查。文本被 token cap 截断时，compact 模式返回 **compact_file_reference**，不是恢复截断全文。skill 单项约 5000、总量 25000 tokens，排除已在 kept 附件的内容。 | **S** 恢复内容来自重新读取，不是旧结果正文永远复制。**U** 文件可变、不可访问、引用后能否重读及真实输出未验证；summary prompt 的“recent files survive”不能替代这些边界。 |
| E07、E41 | 其他状态附件 | 可恢复 plan 引用／plan-mode、未取回的 agent task 状态、同 agent 的后台 shell、invoked skills、peer handback 队列、goal，以及环境／指令相关附件；对 kept 内容做去重。 | **F** isolated agent、plan mode、工具可用性、主链条件影响集合；普通 SessionStart(compact) 受 isolated／settingsHooksOff 约束。**U** 不声称所有运行态都已序列化进摘要。 |
| E07、E42、E48、E60 | tools／MCP 恢复 | boundary 携带 `preCompactDiscoveredTools`，来自历史 tool reference／surfaced announcements 等。恢复用 deferred-tools、agent-listing、MCP-instructions／dropped-tools delta，处理撤销、重加、定义变化及已保留记录。 | **F** latch/pool 分支或 ToolSearch／模型／工具 gates；standard mode 或某些非官方 first-party base URL 默认不开 ToolSearch，允许显式配置改变。**U** discovered 集合不是“所有工具一直可用”的承诺。 |
| E18、E21、E33、E34 | UUID／anchor 重接 | boundary 记录 preserved UUID list 和可选 segment。恢复优先 list，否则按 parent 从 tail 走到 head；缺 UUID／断链则不强接。保留链从 anchor 重设 parent，其后继接回 kept tail，清旧 active-map 条目并修后继。旧 assistant usage 四项归零；live kept-tail 也做同样归零。 | **S** 作用于重建的消息和估算基线，不是历史计费撤销。**U** 不把 active-map 删除推断为原始 transcript 文件立即删除。 |
| E61、E49、E07 | transcript pruning／平台 | pruning planner 保留显式 UUID 或 segment、关联 file-history snapshot/delta、必要跨 boundary parent；缺保留 UUID 或断 walk 则 abort plan。persistence 可因显式禁用、skip prompt history、nested marker 禁用；摘要 transcript 路径还有不可用／平台分支。 | **S** E61 证明 planner，不证明本次或每次都落盘执行。**U** 未验证 retention、完整历史永久可读、跨存储后端一致性；不能把 h3 的 transcript 提示当持久性保证。 |
| E29、E30、E55、E62 | 主 query 应用／公告 | summary 可继承 promptId、turnOrigin、turnPosition、summarizedTurnPromptUuids。query 应用还可把需要的 announcements 放在 boundary 后、summary 前，并避开 kept tail 已有的同类附件。 | **F** `tengu_calm_noodle` fallback true；要求确有 kept tail、非 resumeActive、anchor 相容。**U** 不推广到所有 manual／full 调用方。 |

## 8. Hooks、摘要变体与 continuation

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E26、E51 | function `session.compact` hook | 可调用 core、skip，或替换最终 messages。core 输出 rows 未变才沿用核心结果；改写结果过滤旧 compact boundary，并生成新 UUID；assistant usage 归零。row handle／内容相同可映射回原消息。 | **F** isolated 或无匹配 hooks 时直接 core。**U** hook 自身业务语义、外部副作用未执行；不能把 hook replacement 当正常摘要的等价输出。 |
| E32、E07 | Pre／Post／SessionStart | PreCompact 的成功非 blocked 文本合入指令；blocked 可中止。isolated 情况仅保留 block 信息。PostCompact 接收 summary 并产生 display 结果；普通恢复可能调用 SessionStart(compact)。 | **F** settingsHooksOff 关闭，Post 与 SessionStart 另受 isolated 限制。**S** 背景 precompute 可先运行 Pre／function hook；真正使用／重建时再走相应 Post。不能统一为“消费时重跑所有 hooks”。 |
| E18、E38 | hook replacement 重建 | 无 core 的替换新建 boundary，summaryMessages／messagesToKeep／attachments 为空而放 replacement；Post 的 compactSummary 为空，按条件执行 SessionStart。已有 core boundary 时去掉旧 preserved metadata，避免重复执行同一套恢复。 | **U** 替换消息的内容充分性由 hook 决定，静态片段不证明“摘要成功”。 |
| E22、E23、E43 | 摘要变体／提取 | 可见 control、lean、short、capped；control 要 analysis＋summary，其余 summary；capped 指令为不超过 2000 words。提取优先最后含 summary 标签的 assistant 文本，否则最后文本，不是全链强制格式校验。h3 整理 analysis／summary 标签和空行。 | **S** 2000 words 是 prompt 要求，不是已见硬 token 截断；prompt 对保留内容的叙述也不是系统实际恢复清单。**F** 选择器 `ehe` 先取 `CLAUDE_CODE_CURRIED_TRINKET`，否则 feature `tengu_curried_trinket`，无效值回到 control；reactive `rsr` 使用该选择器。**U** 账户的实际 feature 值及所有入口变体未执行验证。 |
| E15、E23、E03、E04 | continuation 包装 | h3 说明摘要覆盖较早部分，按条件附 transcript 路径；headTruncated 明示最早部分 **“NOT covered by this summary”**；有外部 Artifact 内容时保留不可信内容提示。reactive 传 suppressFollowUpQuestions=true；partial 为 false；full 由调用者参数决定。 | **S** h3 存在 recentMessagesPreserved 文案选项，但所见主要调用点未证明总是传它；“保留尾部”以 metadata／实际 kept messages 为准。**U** continuation 指令不保证模型服从或摘要事实无损。 |

## 9. 取消、计量与 breakers

| 证据 | 路径 | S：静态确证 | F／U：边界 |
|---|---|---|---|
| E01、E08、E09、E29、E37、E58 | auto breakers | 连续失败达到 3 开 failure breaker；成功追踪初始化 failure=0。若上次已 compact 且 turnCounter<3，rapid-refill 累计；连续第 3 次 trip，并向主 query 返回可见失败，不继续压缩。turnCounter 在所见 query-loop 位置递增。 | **S** hookBlocked 不直接等同普通 failed；classifier、prior-attempt、deeper-pass 各有分支。**U** turnCounter 不是已证明的“用户发送次数”；不要把 auto breaker 套给所有独立入口。 |
| E10、E12、E44 | precompute breaker | 每 agent 的 attempts、countedFailures、prompt veto 独立管理；3 次 counted failure 停 re-arm，成功清 countedFailures，subagent exit 可清本 agent 遥测。 | **S** 与 auto consecutiveFailures／rapid-refill 不是同一个计数器；skip、too-few-groups、abort 的处理不同。 |
| E08、E15、E18、E25、E28、E53 | 取消／timeout／budget | 摘要传播 abort，手动映射为取消，controller finally 只清自己的状态。reactive 恢复 timeout 由 token 量计算：`clamp(round(tokens×1.5), 600000, 3600000)` ms，NaN 用 600000；tokens 来源可取估算与 PTL actual 的较大值。API attempts budget 与 fork maxTurns 是不同维度。 | **S** idle／较大模型救援可用 count=1；普通 retry 同时受各路径逻辑限制。**U** provider 重试、hook timeout 默认值、实际总 wall time 未完整确证；不宣称所有入口统一 10 分钟或 1 次请求。 |
| E03、E18、E29、E30、E36、E39 | usage／预算／postTokens | full 分开报告摘要响应的 postCompactTokenCount 与整个重建的 truePostCompactTokenCount；reactive 用重建 QL 估 postTokens。FC 汇总该 fork 的 stream usage；precomputed 的 `RQn` 不再返回 compactionUsage，避免将复用伪作新摘要。boundary 累加正的 pre−post dropped estimate；任务预算另有扣减路径。 | **S** `dhe` 成功记录携带成功 fork 的 usage，不是已证汇总所有失败尝试的账单。**U** 全局计费账本、服务端收费、失败重试收费未验证；usage 清零、skipTranscript／skipCacheWrite 均不等于免费或抹账单。 |

## 10. 证据完整性与交付限制

- 原研究与报告整理执行均为静态操作。62 项片段均已在矩阵引用；主 Agent 当前复核结果为：**52 项整段 byte-match、8 项仅已核对的 160-byte 前缀锚点、2 项原提取未直接映射**。不沿用原索引“59 项唯一 full-match”的错误结论。
- 8 项部分匹配是 E02、E20、E21、E24、E32、E43、E53、E59：strings 提取折叠空行或跳过模块 NUL 分隔符，不能把提取文本长度当原 binary 区间长度。引用这些片段后部时按语义/函数定位，不把前缀核对冒充后部逐字匹配。
- E03（full）与 E23（h3）的关键短锚点另从同哈希 binary 定位；完整提取仍记 unmapped。E59 原记录非唯一匹配，不伪造唯一性。
- 脱敏索引保留每份提取片段的 SHA256、strings 范围、实际核对区间/摘要，以及 E03/E23/variant 的补充短锚点；不依赖临时目录作为跨会话唯一证据。分析者须取得相同 SHA 的合法参考 binary 才能重现这些字节位置。
- 未运行 Claude/Vega、模型或测试；未改用户配置/会话。文件哈希与字节核对是取证检查，不是产品行为验收。未公开二进制或整段专有实现。
- 未证范围：远端 rollout、provider 实际支持、摘要质量/模型服从、私有服务端行为、未展开的常量/入口、底层持久化崩溃一致性、真实端到端竞态与计费。矩阵不能证明“实现完成”或“测试通过”。
