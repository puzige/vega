# ✦ Vega — S4 任务卡（Sprint 4 · Runtime 核心 · W7-8）

**版本** v0.1 · 2026-08-29 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt
**S4 目标**（phase1-plan §2）：provider 抽象（OpenAI 兼容 + SSE 流式）；agentic 循环；read/glob/grep 只读工具。
**Sprint DoD**（phase1-plan §2）：headless 单测——给任务「找出 repo 里所有 TODO」，agent 自主调用工具完成并输出。
> 本 Sprint 全程 headless（vega_runtime/vega_tools 不依赖任何 UI crate，tech-spec §1 红线）。

---

## 卡依赖图

```
T19 Provider trait + OpenAI 兼容流式（vega_runtime 起步）──┐
T21 只读工具 read/glob/grep（vega_tools 起步）────────────┤（T19 ∥ T21 并行）
                                                            ▼
                              T20 Agentic 循环 + 上下文组装 + 事件/落库
                                                            ▼
                              T22 端到端验收「找出 repo 里所有 TODO」（DoD 收口）
```

---

## T19 · Provider trait + OpenAI 兼容流式实现（A3-01/A3-02 · vega_runtime 起步）

- **前置**：S1-S3 · **参考**：tech-spec §4.1 §7（VegaError）§8（mock provider 回放）
- **产出**：
  - vega_runtime crate 起步（headless）：`Provider` trait（tech-spec §4.1 签名：chat_stream(req, cancel) → EventStream）、`ChatRequest`（model/messages/tools/max_tokens）、`ProviderEvent`（TextDelta/ThinkingDelta/ToolUse{id,name,input_json}/Usage/Done{stop_reason}）、`StopReason`（end/tool_use/length 最小集）
  - **async 处理（架构师预裁，可否决）**：trait 方法返回手工装箱的 `BoxFuture`（即 async-trait 宏的产物形态）——**零新依赖**、dyn 兼容（mock/真实 provider 可替换）、与 §4.1 trait 边界完全一致；不引入 async-trait crate
  - OpenAI 兼容实现（A3-02）：reqwest(rustls) POST `{base_url}/chat/completions` `stream:true` + `stream_options.include_usage=true`；SSE 解析（eventsource-stream）；tool_calls 增量按 index 聚合完整后发 ToolUse；usage 从最终 chunk 回收
  - 重试：网络错误/5xx 指数退避 1s/2s/4s 最多 3 次；429 读 Retry-After；**重试只重建请求上下文**
  - `VegaError`（tech-spec §7 全枚举落 vega_runtime，跨线程 Send+Sync）
  - MockProvider：按脚本回放 ProviderEvent 序列（§8 测试策略；S4-S8 全部循环测试的公共基建）
  - workspace 依赖首次激活（均既有白名单，精确锁定）：tokio、tokio-util（CancellationToken）、futures、reqwest(rustls)、eventsource-stream
- **验收**：`cargo test -p vega_runtime` 全绿——mock 回放（纯文本/工具调用/usage/done 序列）、SSE 跨 chunk 切分解析（token 中间断开）、重试路径（5xx 退避后成功、429 尊重 Retry-After、3 次耗尽报错）、cancel 中断流即断
- **禁区**：不做 agentic 循环（T20）；不碰 UI；真实网络调用仅集成冒烟（单测全走 mock）

## T20 · Agentic 循环 + 上下文组装 + 事件/落库（A3-03）

- **前置**：T19 T21 · **参考**：tech-spec §4.2（时序定稿）§3（ConversationEvent——S3 后已解锁）§8
- **产出**：
  - agentic 循环（tech-spec §4.2 时序）：上下文组装（system prompt + 对话历史窗口；记忆/@文件 S5+ 后置，注明）→ chat_stream → TextDelta/ThinkingDelta 转发 → ToolUse → **工具执行（单轮串行）** → 输出截断（头 2k+尾 2k 行）→ tool_result 追加 → Usage 到达 → 计价钩子（**S4 成本占位 0，计价引擎 S7 接入**，注明）→ token_usage 落库 → 无 ToolUse 且 stop_reason=end 收敛
  - 循环上限 100 轮工具调用，超限收敛并提示；中断 = CancellationToken <1s 停手（KPI），当前工具等待完成后收敛
  - 事件双类型（**架构师预裁，解 §1 依赖方向歧义，可否决**）：vega_runtime 定义并发射 `RuntimeEvent`（字段与 ConversationEvent 对齐），vega_conversation 驱动循环并**转换**为 `ConversationEvent`（types 红线不破：UI 唯一事件流仍是 ConversationEvent；落库编排也在 conversation 层——messages/threads 行 + tool_calls 行（status 生命周期 approved→running→success）+ token_usage 行（cost_microcents=0 占位））
  - 权限钩子占位：S4 只读工具 + auto/readonly 语义（写类工具 S5 才存在），§4.3 完整矩阵 S5 落地
- **验收**：mock provider 脚本驱动完整循环（任务 → tool_use → 真实 vega_tools 工具 → 收敛输出）；中断测试（cancel 后 <1s 停）；100 轮上限测试；落库断言（messages/tool_calls/token_usage 行正确）；六表外零 DDL
- **禁区**：不做写工具（S5）；不做权限矩阵（S5）；不接真实 provider 密钥路径（冒烟除外）

## T21 · 只读工具：read / glob / grep（A3-05~07 部分 · vega_tools 起步）

- **前置**：S1-S3（与 T19 无依赖，**可并行**）· **参考**：tech-spec §4.4（I/O 契约）§3（路径围栏红线，risks #4）
- **产出**：vega_tools crate 起步（headless，仅 thiserror + ignore + regex——均白名单）
  - **路径围栏（红线，最先实现）**：所有工具入参 path 相对项目根解析，**canonicalize 后必须仍位于项目根内**，逃逸（`../`、绝对路径、symlink 跳出）一律拒绝并报 Tool 错误
  - read：path/offset/limit → 带行号输出；单行 >2k 字符截断；二进制检测（NUL 探测）拒绝；文件不存在/超 limit 语义按 §4.4
  - glob：pattern → ignore crate 遍历（**尊重 .gitignore**）+ glob 匹配；结果上限 500 条
  - grep：pattern(regex)/path → ignore 遍历 + 逐行 regex 匹配（文件:行号:行内容）；上限 500 条；二进制文件跳过
  - 统一工具输出结构（成功文本 / Tool 错误），供 T20 循环以 tool_result 追加
- **验收**：tempdir 全覆盖——三工具正例、gitignore 尊重（忽略文件不被 glob/grep 命中）、**围栏逃逸三种形态全部拒绝**、read 二进制拒绝、单行/总条数截断、上限 500
- **禁区**：不做写工具 write/edit/bash（S5）；不做 web_fetch（后置）；不引白名单外 crate（ignore/regex 已有）

## T22 · 端到端验收：「找出 repo 里所有 TODO」（Sprint DoD 收口）

- **前置**：T19 T20 T21 · **参考**：phase1-plan §2 S4 验收原文；tech-spec §8
- **产出**：vega_runtime 集成测试——临时 repo（含散布 TODO 的多语言文件）+ **mock provider 按真实 agent 行为编排脚本**（收到任务 → 调 grep 工具 → 调 read 工具 → 汇总输出 TODO 清单 → end），走完整 T20 循环 + 真实 vega_tools + 落库；断言：最终输出含全部预埋 TODO、循环轮数合理、落库行完整、全程无 unwrap 路径
- **验收**：该集成测试 + T19-T21 全测 + 四门禁在 master 绿 = S4 DoD 达成
- **禁区**：不用真实 LLM（mock provider 编排即验收口径——真实调用属 S4 后 dogfood）

---

## S4 完成定义（DoD，Sprint 验收）

- [ ] T19-T22 全绿；hooks 门禁 master 绿
- [ ] **「找出 repo 里所有 TODO」headless 端到端**（mock provider + 真实工具 + 落库断言）
- [ ] 中断 <1s 停手实测；100 轮上限生效；围栏逃逸全拒
- [ ] 红线全过（runtime/tools 无 UI 依赖；types 唯一事件流；六表外零 DDL）；bench 不回退

> 架构师预裁（可否决）：① async 用手工 BoxFuture 不引 async-trait；② RuntimeEvent/ConversationEvent 双类型 + conversation 层转换（解 §1 依赖方向歧义）；③ S4 成本计价占位 0（S7 接引擎）；④ S4 循环含权限钩子占位（§4.3 矩阵 S5）。
