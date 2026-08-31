# ✦ Vega — S7 任务卡（Sprint 7 · Token 经济 v1 · W13-14）

**版本** v0.4 · 2026-08-31 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt

**S7 目标**（phase1-plan §2）：API usage 精确回收；流式期间实时估算；每次调用与每个任务的成本可见；数据目录 `pricing.json` 内置主流模型并支持用户自定义。

**Sprint DoD**：确定性 mock 任务完整证明“流式近似 → API usage 校准 → `cost_microcents` 实算 → 六表内落库 → Composer/任务汇总可见”；真实任务与 API 账单误差 `<5%` 的最终证据由人类 dogfood 提供，executor 不索取 key、不发真实请求、不产生费用。

> **实现基线**：T37 开工基线为 `master` `90e5e35`，S6 T35 已合并；T37-T41 只适配该基线上的真实 Settings/Composer/route seam，不反向修改已冻结的 S6 契约。
>
> **白名单降级裁决**：phase1-plan §2/§3.3 与 features A10-02 写有 `tiktoken-rs`，但它不在 exec-guide §5 白名单。S7 不引入它或任何替代 tokenizer；v1 固定为“API usage 最终权威 + 流式期间按 Unicode scalar 字符数作有界近似”，并以 `≈` 明示估算。估算不写 `token_usage`，usage 到达后原位校准。未来引入 tokenizer 必须另行获批。
>
> **币种裁决**：`Microcents` 依 tech-spec §3 表示 `1/1_000_000 USD`；S7 定价与聚合只用 USD，不做实时汇率、CNY 换算或混币种求和。ui-spec §4.4 的 `¥0.17` 视为展示样例，S7 实装显示 `US$`；该字面偏离进入 S7 报告。

---

## S7 冻结契约

### C1 · 定价目录与安全持久化（A1-12/A10-03）

- `vega_token` 是纯 headless 定价/计量 crate；不得依赖 GPUI、SQLite、Keychain、网络或 `vega_runtime`。调用方只向它传显式文件路径、模型 id 与 token usage。
- `pricing.json` 固定在 tech-spec §6 的数据根；首次缺失时由内置目录原子创建，父目录沿用 `vega_store::paths::data_dir()`。测试只注入 `tempfile` 路径，不触碰真实用户数据目录。
- v1 JSON 顶层 exact 为 `schema_version="pricing_v1"`、`currency="USD"`、`models`；未知/缺失/重复字段或重复 exact model id 均拒绝，不静默覆盖。每个模型含 exact UTF-8 `model` 与四个十进制定价字符串：`input_usd_per_million`、`output_usd_per_million`、`cache_read_usd_per_million`、`cache_write_usd_per_million`。禁止 JSON float 进入计价路径。
- 限额 inclusive：文件 `<=1 MiB`、模型 `<=1,000`、model id `1..=200 bytes`、价格为非负十进制且小数位 `<=6`；解析成每百万 token 的整数 microcents，所有转换 checked。model 匹配大小写敏感、exact only；不做 glob/prefix/alias 猜测。
- 内置目录至少 5 个 exact model id，覆盖 DeepSeek、GPT、Claude 三个系列。实现卡须在报告记录每条价格的官方来源与核验日期；价格属于时点数据，不在 SDD 中伪造数值。Anthropic 模型价格可用于 OpenAI-compatible 渠道的自定义 model id；Anthropic 原生 provider 仍属 Phase 2。
- 用户自定义通过设置页对 exact model id 新增/编辑/删除；保存采用同目录临时文件 → flush/sync → rename 的原子替换，先完整校验再写。内置条目可覆盖价格但不能产生重复 key；损坏文件保留原样并显示 typed inline error，禁止删除、截断或静默回退覆盖。
- configured model 在真实 provider 调用前必须有 exact 有效价格；缺失/损坏定价时拒绝启动并内联提示“先配置价格”，保证 `0` 不被冒充“免费”。该 preflight 不读取或暴露 API key。

T36 动态价格补充契约（v0.2）：

- model 顶层四价是 base/off-peak profile；可选 `max_standard_input_tokens` 为正 JSON 整数，可选 `schedule` exact 为 `{"kind":"utc_weekly_v1","windows":[...],"peak":{...}}`。`peak` exact 含与 model 顶层同名的四个十进制价格字段；`windows` 为 1..=32 项，每项 exact 为 `{"weekdays":[...],"start_minute":N,"end_minute":N}`。weekday 使用 ISO `1=Mon..7=Sun`，每窗 weekday 1..=7 项且不重复，minute 为 UTC 当日 `0..=1440` 的半开区间，必须 `start < end`；同一 weekday 的窗口不得重叠。所有层继续 `deny_unknown_fields`。
- quote 必须显式接收 Unix UTC 秒，以无依赖整数 `div_euclid` 计算 weekday/minute；窗口命中使用 peak，否则使用 base，并返回 exact `pricing_v1` 与 `base|peak_utc_weekly` profile。静态 custom entry 省略 `schedule`。
- OpenAI `gpt-5.6-terra` / `gpt-5.6-luna` 的 `max_standard_input_tokens=272000`；usage `input` 在 exact 上限可计价，`+1` 返回 typed unsupported，禁止按 standard 低价估算长上下文。DeepSeek `deepseek-v4-flash` / `deepseek-v4-pro` 的 peak 为 Mon-Fri UTC `[01:00,04:00)` 与 `[06:00,10:00)`；Anthropic `claude-sonnet-5` 的 cache-write 只代表 5m standard profile，1h/geo/fast 不自动猜价。
- atomic save 以同目录 `rename` 成功为 commit point。rename 前任一步失败必须保持旧 target bytes 精确不变，并 best-effort 清理 `create_new`、有界唯一尝试创建的 temp；rename 后 directory fsync 失败返回 typed `CommittedDurabilityUnknown`，明确新 bytes 可能已可见，调用方不得把它当作普通未提交失败盲重试，且不承诺恢复旧 bytes。preflight 保守拒绝既有 target symlink、non-regular 或 `nlink>1`。
- safe model id v1 grammar 固定为首字节 ASCII alphanumeric，后续仅 ASCII alphanumeric / `.` / `_` / `:` / `/` / `-`，并拒绝 `..`、`//` 与尾随 `/`；长度仍按 UTF-8 bytes 计 1..=200。十进制允许 leading zero，save/reload 保留调用方字符串字面；不做 trim 或数值 canonical rewrite。`max_standard_input_tokens` 与 `schedule` 不得同时出现。
- `models` / `windows` / `weekdays` 的 logical cap 在 serde sequence 边界执行：retain 到 exact limit 后只用零 `T` 构造的 ignored probe 判断 `+1`，不得先 deserialize 第 `limit+1` 个业务对象。ordinary save 对既有 target snapshot 的 exact bytes 必须在创建 temp 前走同一 strict decoder；损坏文件返回 typed codec/schema error、bytes 不变、零 temp/rename。missing target 的首次 seed 不受此限制。
- optional 只表示 field absent：`max_standard_input_tokens` / `schedule` 缺失映射为 `None`，但字段显式出现为 JSON `null` 必须拒绝；所有 required field 的 `null` 同样拒绝，不做缺失或默认值归一。

T37 Settings/authority 补充契约（v0.3）：

- `pricing_v1` 不增加 `origin` 字段。built-in membership 每次从 `PricingCatalog::built_in()` 重建；持久化目录必须保留全部五个 built-in exact model id。built-in 不可删除，Reset 恢复完整 exact built-in spec；未来 built-in 变化不得静默合并进既有 v1 文件。
- GPT built-in 只允许编辑四项 base rate，并锁定 exact `max_standard_input_tokens` 与无 schedule；Claude built-in 只允许编辑四项 base rate，并锁定 static metadata；DeepSeek built-in 显示并编辑四项 base rate + 四项 peak rate 共 **八项** string input，`schedule.kind` 与 windows 锁定为 built-in exact metadata；custom 只允许四项 static rate，`max_standard_input_tokens=None`、`schedule=None`。UI 不得提交 generic `ModelPricingSpec`；conversation policy 必须按 typed mutation 重建并复验完整 spec。
- malformed/unsafe 既有 `pricing.json` 必须 byte-preserve；不得删除、截断、备份后覆盖、静默 seed 或强制保存。authority 进入 `Invalid(error_code)`，编辑/保存/任务启动全部阻断，仅允许显式 Reload。用户外部修复或删除后显式 Reload；只有 target 确实 missing 时才可重新 seed built-ins。
- `vega_token` 提供 private-bytes `CatalogSnapshot` 给 headless service 做 byte-exact + semantic reconciliation；raw bytes、绝对路径与底层 codec error 不得越过 token/conversation 边界。ordinary save 只有安全 reload 与 desired canonical bytes/semantic 同时 exact 才发布新 authority。
- app/controller 是唯一 pricing authority，状态 exact 为 `Loading | Ready(authority) | Saving | Reloading | Invalid(error_code)`。只有 `Ready` 可启动任务或开始 pricing mutation；Save/Reload single-flight、无队列、无自动 retry。Settings entity close 只 fence UI delivery，不取消 reconciliation；reopen 从 controller 重新投影。
- locked-profile policy 必须在每个 authority ingress 执行：initial load、explicit Reload、ordinary save 后 reload、`SaveTargetChanged` winner 与 `CommittedDurabilityUnknown` reconciliation 都先复验五个 built-in exact membership/metadata 及 custom static-only；codec-valid 不等于 policy-valid，任一失败都不得发布 `Ready`。
- `Saving` 开始后 desired catalog、operation generation、旧 Ready authority 与 dirty/conflict draft 均由 controller 持有，绝不寄存在 Settings entity。ordinary success、target race、durability unknown、pre-commit failure、worker spawn/channel failure都必须 exact first-wins 收到一个终态；Settings close during save 不丢 desired、不取消 worker、不提前回 Ready，也不得永久卡在 `Saving`。
- `Ready` 若持有 pre-commit failure 或 external-winner conflict draft，普通 Add/Edit/Reset/Delete 一律 typed `Busy` 拒绝且不得替换或丢失 draft；UI 只提供 **Retry original plan**、**Discard and adopt current authority** 与 explicit Reload。Retry 复用 controller 持有的 exact original plan 并重新走一次普通 atomic save；若该次结果仍 ambiguous，仍只做一次安全 reconcile，禁止 blind retry。Discard 只丢 controller draft，不改文件，随后以当前 authority 投影。
- `CommittedDurabilityUnknown` 禁止盲目再次保存；立即且仅一次 reload：exact desired → `Ready` + persistent warning，valid different winner → 采用 winner 且保留 dirty draft/conflict，reload failure → `Invalid(RecoveryRequired)`。pre-commit failure 保留旧 `Ready` authority 与 dirty draft；`SaveTargetChanged` reload 一次后采用 valid winner，否则进入 recovery-required。valid external winner 可给新 run 计价，但旧冲突 draft 仍保持 dirty。
- same-user 外部编辑只有显式 Reload 或 restart 后才成为 authority；不加 watcher。两次安全捕获之间的 complete same-user path swap 属已接受 TOCTOU residual，报告必须如实记录，不得宣称 race-free。

### C2 · 整数成本与 cache 语义（A10-04）

- OpenAI-compatible `input` 保持 API 的 total prompt token；计价时 `uncached_input = input - cache_read`，`cache_read > input` 拒绝为 invalid usage；`cache_write` 按独立字段另计，S4 OpenAI wire 当前无该字段时仍为 0。
- 一次调用的 numerator exact 为 `uncached_input*input_rate + output*output_rate + cache_read*cache_read_rate + cache_write*cache_write_rate`；用 checked `u128` 累加，最后仅一次 half-up 除以 `1_000_000`，结果必须可转 `i64 >= 0`。禁止 `f32/f64`、逐项先舍入、saturating 或 wrapping arithmetic。
- API usage 是每次 provider call 的权威记录；每个 agentic round 独立计价并保留一行 `token_usage`。任务/会话成本只聚合已落库行，checked overflow/负历史值/损坏行 fail closed，不显示貌似可信的部分和。

### C3 · 校准、估算与事件边界（A10-01/A10-02）

- S4 `ProviderEvent::Usage → RuntimeEvent::UsageUpdated → ConversationEvent::UsageUpdated` 双层边界保持唯一事件流；只替换 runtime 中 `cost_microcents: 0` 占位，不新增 UI→runtime 反向依赖，不让 UI 直接读 SQLite。
- 每个 provider call 的 `TextDelta` 只形成内存 **visible-output-only** provisional estimate：累计 Unicode scalar 数后 `ceil(chars/4)`，全程 checked、有固定上限；UI 标 `≈`。Thinking/reasoning 与 tool JSON 不在可见输出估算内，因此不得将该值声称为完整 completion usage。它不修改 API usage、不落库、不写日志，下一轮从 0 开始。
- 对应 `UsageUpdated` 到达时，清除该轮 provisional 值，并以 API 的 input/output/cache_read/cache_write 与实算 cost 更新累计值；若无 usage 便进入 tool round，在该轮首个 `ToolCallProposed` 边界清除 provisional，保证下一 provider call 从 0 开始。`MessageFinished`/error/interrupted 无 usage 时同样清除 provisional 且显示“usage unavailable”，绝不把近似写成权威值。
- 重试未产生 usage 不写行；一次 provider call 只接受一个 terminal usage。重复/malformed/usage-after-terminal fail closed；测试必须覆盖多轮工具调用、重试、取消、迟到/重复 usage 与 overflow。
- run preflight 在 durable Thread/Project load 后、`AppAgentController::begin`/artifact generation/channel/worker/config/Keychain/provider 前，从当前 `Ready` authority 按 durable `Thread.model` exact case-sensitive 选择 immutable pricing capability。失败时 begin/spawn/key/provider request 均为 0，并引导打开 Settings；不得用 config default、alias、prefix/glob 或 stale UI projection替代。
- immutable selection 随 run ownership 移交并贯穿全部 agentic rounds；Settings 在 run 中保存/Reload 不得替换该 run selection，runtime/provider 不得逐调用重读 pricing file 或 controller current authority。
- 每个 logical provider call 在第一次 `provider.chat_stream` 之前立即捕获 Unix UTC 秒；provider 内部 HTTP retry 复用该 timestamp，后续 tool/agentic round 捕获新 timestamp。该 call 的首个且唯一 authoritative Usage 必须用 frozen selection + frozen timestamp quote；quote 失败不得写 zero/partial usage row，事件链保持唯一。

### C4 · UI 投影与性能（A10-05/A10-06）

- Composer 右下角常驻 compact counter；空闲/已校准显示 `<tokens> tok · US$<cost>`，流式 provisional 显示 `≈<tokens> tok · ≈US$<cost>`，未知 usage/price 明确显示 `—`，不能显示 `$0`。token 使用 k/M 紧凑格式，cost 至少保留足以区分非零 microcents 的精度。
- `ConversationStream` 只消费 `vega_conversation::types` 的 bounded meter/summary projection与既有事件；pricing file、SQLite query、成本公式均不在 `vega_ui`。路由切 thread/project/run generation 时丢弃晚到更新，重开 thread 从 conversation 查询恢复已校准累计值。
- 任务结束追加一张只读 summary card：input/output/cache read/cache write、总成本、耗时、工具调用数、cache hit rate；仅聚合本次 assistant message 的 provider-call rows与既有 tool call audit。token/cost/cache/tool count 可按现有 `message_id` 持久化归属恢复；完整任务 wall-clock duration 只在当前运行内存可用，因现有 `messages` 无 finished timestamp，重启后该项显示 `—`，不以 tool duration 冒充。无 usage 字段显示 `—`，除 0 input 时 cache hit rate 显示 `0%`。不做日/周/月 dashboard、预算、跨模型对比或 CSV（A10-07+ 后置）。
- meter 更新不得做同步 IO，不得每 delta 查库/读 pricing 文件；沿用 S3 批次上屏，目标仍为 P2 `<16ms`，已冻结会话区不因 counter 数字宽度变化而重排。

### C5 · 数据、错误与红线

- 保持 `projects/threads/messages/tool_calls/token_usage/permissions` 恰好六表；不改/删旧 migration，只追加下一号 migration，为 `token_usage` 增加 nullable `pricing_version` 列。已有 S4/S5 `cost_microcents=0` 行的 NULL 表示 `legacy_unpriced`；S7 后完成定价的行写 exact `pricing_v1`，从而区分“历史占位 0”与“经计价后舍入为 0”。不回填/重算历史成本，不新增表。查询/聚合 API 落在 `vega_store`，由 `vega_conversation` 调用；遇到 NULL/未知 version 时该行成本显示 unavailable，不冒充免费。
- 定价/usage 错误映射为 typed `VegaError`/conversation error；错误文本可含 safe model id 与字段名，不含 key、请求正文、文件内容或绝对用户路径。
- 零真实 key、零真实 provider 请求、零真实费用；所有测试只用 `MockProvider`、本地 fixture 与 temp data root。依赖只用现有白名单与 workspace 内部 crate，`Cargo.lock` 如因内部依赖接线变化必须随卡提交。

---

## 卡依赖图

```text
S7 SDD PR → T36 pricing catalog + integer engine → T37 pricing settings/custom persistence
                                                   ↓
             T38 calibrated runtime/store pipeline → T39 stream estimate + Composer counter
                                                       ↓
                                                   T40 task summary
                                                       ↓
                                                   T41 acceptance/report
```

> 每张实现卡使用独立 sibling worktree/PR，并在上一卡 squash merge 后从最新 `master` 创建；S6 并行期间只允许无交叉写入的准备/审阅，发生 UI seam 交叉时先 rebase 再适配。

## T36 · Versioned pricing catalog + integer cost engine（A10-03/A10-04）

- **前置**：S7 SDD · **参考**：C1/C2/C5；tech-spec §3/§6；features A10-03/A10-04。
- **范围**：填实 `vega_token`；补 workspace 内部依赖声明；不改 runtime/store/UI/app。
- **产出**：strict `pricing_v1` codec、内置至少五模型目录、显式路径 load/seed/atomic save、exact lookup、checked integer quote 与 safe typed errors。
- **验收**：内置三系列覆盖；missing seed/reopen；custom exact override；missing/extra/duplicate/oversize/malformed decimal/negative/7位小数/overflow；cache 计价与 half-up 边界；原文件在失败保存后 byte-identical；crate 无 GPUI/SQLite/network。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_token
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy -p vega_token --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && ! cargo tree -p vega_token | rg 'gpui|rusqlite|reqwest|keyring|tiktoken'
  git diff --check
  ```

- **commit**：`feat(A10-03): add versioned model pricing`（≤3 commits）。

## T37 · Pricing settings + custom model persistence（A1-12/A10-03）

- **前置**：T36 + 已合并 S6 最新 master · **参考**：C1/C4/C5；ui-spec §4.6/§6。
- **范围**：`vega_token::CatalogSnapshot` private-byte authority、conversation headless pricing policy/service、app-owned `PricingController`、现有 Settings route 的 safe projection/actions；UI 不直接读写 pricing file。T37 不接 runtime usage、不改 store schema/DDL。
- **产出**：data-root `pricing.json` wiring；built-in/custom 统一列表与 typed 新增/编辑/删除/Reset；GPT/Claude 四项 USD/1M string input、DeepSeek base+peak 八项、custom 四项，所有 profile metadata 按 C1 锁定；malformed/unsafe typed inline error + explicit Reload；`Loading/Ready/Saving/Reloading/Invalid` durability state；真实 provider call 前 exact durable-thread model preflight。保存后必须 byte/semantic exact reload reconcile；durability-unknown 按 C1 单次 reload 且零自动 retry。
- **验收（E2E-first）**：保留 T36 codec/process/atomic safety kernel；新增一个 owned temp data-root headless production journey，覆盖 missing seed → custom add/update/delete → restart → malformed `Invalid` byte-preserve → external repair + explicit Reload；新增一个 production Settings/app journey，覆盖未定价 model 的 begin/spawn/provider-request 均 0、close during save 后 controller 完成并 reopen 投影，以及配置成功后只用 `MockProvider` 越过 gate exact once。只保留无法由 E2E 稳定证明的窄测试：snapshot byte/semantic、built-in locked profile/DeepSeek 8-input、durability-unknown winner、generation first-wins、keyboard/960×600/theme/CJK；不得复制 T36 错误笛卡尔矩阵。证据按 exec-guide §7 写入仓库，raw log 仅放 fresh `/private/tmp`。
- **禁区**：不做模型在线拉价、汇率、provider/key 编辑重构、通配匹配；不触碰真实 data root/key，不用真实 provider/key/network；不新增依赖、DDL 或事件 variant。
- **commit**：`feat(A1-12): add custom pricing settings`（≤3 commits）。

## T38 · API usage calibration + runtime/store cost pipeline（A10-01/A10-04）

- **前置**：T37 · **参考**：C2/C3/C5；tech-spec §4.2；现有 S4 cost hook与 `token_usage` insert。
- **范围**：T37 immutable exact-model selection handoff、`vega_runtime` 纯计价接钩、`vega_conversation` 事件/持久化、`vega_store::token_usage` typed query，以及 C5 单一追加 migration；不改 UI、不读 pricing file/controller current authority。
- **产出**：run-start selection 安全注入 runtime；每个 agentic round 在 logical provider call start 冻结 UTC 秒，provider internal retry 复用，后续 round 重新捕获；每次首个 authoritative `ProviderEvent::Usage` 用 frozen selection/time 实算 cost；既有双层事件映射与一行一调用持久化；`pricing_version` 区分 legacy/unpriced 与 priced zero；按 thread/message 的 checked aggregate查询，恢复后结果一致。
- **验收（E2E-first）**：一个 headless production journey 用 `MockProvider` 运行两次 provider call + 一次 tool round，断言两个独立 call-start timestamp/profile、exact usage/cost/event、两行 DB 与 restart aggregate；Settings mid-run authority变化不影响 frozen run selection，provider internal retry不改变 call timestamp。仅保留 narrow unit/property 表覆盖 zero/cache/maximum/invalid、UTC profile boundary、duplicate/late/cancel/overflow与 checked aggregate；不复制 T36 pricing codec/atomic矩阵。删除 thread 后 usage 审计仍可聚合；S4 TODO E2E 仅改期望成本 fixture且继续全绿。全程只用 MockProvider/temp store，零真实 key/network/费用。
- **命令**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_runtime usage
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_store token_usage
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation usage
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p vega_conversation --test todo_e2e
  git diff --check
  ```

- **commit**：`feat(A10-01): persist priced provider usage`（≤3 commits）。

## T39 · Bounded stream estimate + Composer live counter（A10-02/A10-05）

- **前置**：T38 + 已合并 S6 最新 master · **参考**：C3/C4/C5；ui-spec §4.4/§5 P2/P3/P5。
- **范围**：conversation meter projection、`ConversationStream`/app route wiring；不改 provider wire、pricing file或 DDL。
- **产出**：按 `ceil(chars/4)` 的每轮 provisional output；API usage 原位校准；Composer 右下常驻 compact token/cost；thread/run/generation fence；restart 从 durable aggregate恢复。
- **验收**：ASCII/CJK/emoji/空 delta/exact cap/+1；多轮 estimate→calibrate不双算；无 usage/error/interrupted清 provisional；route switch/reopen/late event；`≈`/`—`/US$语义；1,000 delta/s counter 更新不做 IO且 P2 不回退。
- **禁区**：不引 tokenizer；不持久化 estimate；不做 @引用、/命令或模型选择器。
- **commit**：`feat(A10-05): show calibrated live token costs`（≤3 commits）。

## T40 · Per-task cost summary card（A10-06）

- **前置**：T39 · **参考**：C4/C5；ui-spec §4.2/§4.6/§6。
- **范围**：conversation typed summary query/projection + compact read-only UI card；不做 dashboard。
- **产出**：MessageFinished 后显示本任务 token 四项、cost、duration、tool count、cache hit；重启恢复 token/cost/cache/tool count，duration 明确降级为 `—`；missing usage/pricing/overflow 为 typed unavailable，不伪造 0。
- **验收**：0/1/N provider calls、0/N tools、cache ratio、无 usage、interrupted/error、restart 持久字段与 duration 降级、thread deletion保留 usage审计；卡片 token/Light-Dark/CJK/键盘焦点不阻断会话导航。
- **commit**：`feat(A10-06): add task cost summaries`（≤3 commits）。

## T41 · S7 end-to-end acceptance + report（A10-01~06）

- **前置**：T36-T40 · **参考**：phase1-plan §2 S7验收；PRD §9；ui-spec §5/§6；exec-guide §3/§7。
- **产出**：
  - deterministic mock E2E：两轮 provider call + tool → provisional counter → 两次 API usage 校准 → exact DB rows/cost → task summary → restart恢复；fixture 同时计算模拟“账单”并断言误差 0。
  - `docs/vega-s7-report.md`：全部 PR/commit、测试总数与原始命令输出、T36-T40/DoD/ui-spec §6/红线对照、官方价格来源+日期、偏离/后置/未测证据；更新 README 状态行与文档表。
  - 真实任务 vs provider 账单 `<5%` 明确记为 **⚠️ human dogfood pending**，附不含 key 的操作步骤与记录模板；禁止用 mock 0% 误差冒充真实 KPI。
- **门禁**：

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH" && cargo fmt --all -- --check
  export PATH="$HOME/.cargo/bin:$PATH" && cargo clippy --all-targets -- -D warnings
  export PATH="$HOME/.cargo/bin:$PATH" && cargo test --workspace
  export PATH="$HOME/.cargo/bin:$PATH" && cargo build --workspace
  export PATH="$HOME/.cargo/bin:$PATH" && cargo xtask bench
  export PATH="$HOME/.cargo/bin:$PATH" && cargo tree
  rg -n '#[0-9a-fA-F]{6}' crates/vega_ui
  rg -n '\.(unwrap|expect)\(' crates --glob '*.rs'
  rg -n 'tiktoken|API[_ -]?KEY|sk-[A-Za-z0-9]' crates docs --glob '!target/**'
  rg -n 'CREATE TABLE|DROP TABLE|ALTER TABLE' crates migrations
  git diff --check
  ```

  所有 scan 逐条分类测试段/既有安全文本/真实违规，禁止用“有输出所以失败”或“零输出所以完成”替代审阅。
- **commit**：`docs(A10-06): report S7 token economy acceptance`（≤3 commits）。

---

## S7 完成定义（DoD）

- [ ] T36-T41 全部 squash merge；master 四门禁全绿，`Cargo.lock` 与内部依赖接线一致。
- [ ] `pricing.json` 位于注入的数据根；内置至少 5 个模型且覆盖 DeepSeek/GPT/Claude；custom CRUD、损坏保留与原子保存有测试证据。
- [ ] 每个 mock provider call 的 API usage 四项与实算 `cost_microcents` 一行落库；多轮总计、事件、DB、重启恢复一致。
- [ ] Composer 流式显示 `≈`，usage 到达后校准为权威值；任务结束 summary 六项完整；unknown/unavailable不伪装为 0。
- [ ] mock 账单对照误差 0；真实账单 `<5%` 只标 human pending，零真实 key/请求/费用。
- [ ] 恰好六表；旧 migration 不改/不删，仅有 C5 的 `token_usage.pricing_version` 追加 DDL；`vega_runtime`/`vega_token` 零 GPUI；UI 零 SQLite/定价文件 IO；共享类型只在 `vega_conversation::types`；非测试 `unwrap/expect` 零新增；色值/字号只用 token。
- [ ] ui-spec §6 逐项有自动化/人工/硬件三分证据，不能自动化的项不写 ✅；P1-P8 不回退，真实窗口/ProMotion/RSS 项可明确留 S8。

## ui-spec §6 Sprint 末检查矩阵

| 检查项 | 自动化最低证据 | 人工/硬件边界 |
|---|---|---|
| token | counter/summary 全用 theme/Typography token；色值 scan 分类 | 真实字体与金额对齐观感 |
| Light/Dark | 双 theme render/state tests | 真实切换无闪烁 |
| CJK | CJK/emoji 估算与布局不 panic | fallback/豆腐块真实窗口 |
| keyboard | Settings custom pricing + Composer/summary 可达 | 完整真实窗口链路 |
| 960×600 | layout constraints/compact formatter tests | 像素截图人工 |
| P1-P8 | `xtask bench` 原始值并与 S6 baseline 比较 | ProMotion/首帧/RSS归 S8 |
| competitor | 无自动化替代 | Codex/ZCode并排截图未做即 ⚠️ |

## 已知偏离与后置（原样进入 S7 报告）

1. `tiktoken-rs` 因白名单红线未引入；流式 v1 是字符近似，只有 API usage 是权威值。
2. ui-spec §4.4 的 `¥` 样例改为 `US$`，因为 `Microcents` 与内置官方价格均按 USD；Phase 1 不做 FX。
3. 真实账单 `<5%`、真实 provider、key 与 dogfood 属人类活动；executor 只提供 mock E2E 与操作模板。
4. A10-07 dashboard、A10-08预算、A10-09跨模型对比、A10-10优化、A10-11闲时联动、A10-12导出均不在 S7。
5. T37 已从合并 S6 T35 的 `90e5e35` 基线开工；T37-T41 只按当前真实 Settings/Composer/diff/route seam 接入，不反向修改已冻结 S6 契约。

## 变更记录

- v0.4 (2026-08-31) T37 authority review：冻结 dirty/conflict draft 的 controller ownership；draft 存在时普通 mutation fail closed，只允许 Retry original plan、Discard adopt authority 或 explicit Reload，且 ambiguous Retry 仍禁止 blind retry。
- v0.3 (2026-08-31) T37/T38 implementation preflight：冻结 built-in/custom 编辑策略与 DeepSeek 八项输入/locked metadata，并要求所有 authority ingress 复验 policy；malformed external recovery、controller-owned Saving desired/generation/draft 与完整 durability 终态；run-start immutable exact-model selection、logical provider-call start UTC-second 计价及 Settings mid-run semantics；验收改为 E2E-first 并保持零 key/network/新依赖；实现基线更新为已合并 S6 T35 的 `90e5e35`。
- v0.2 (2026-08-31) T36 官方价格核验后补齐 flat schema 缺口：冻结 strict UTC weekly schedule、显式 quote timestamp、OpenAI 272K standard 上限及 DeepSeek peak 半开窗口；Anthropic cache-write 限定为 5m standard profile；原子保存明确 rename commit point、post-commit durability-unknown 与保守 target preflight。
- v0.1 (2026-08-31) S7 首版任务拆分与冻结契约。
