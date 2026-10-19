# 会话未读与运行完成通知

## 范围与基线

- Issue：[#481](https://github.com/puzige/vega/issues/481)。采用[最新产品裁决](https://github.com/puzige/vega/issues/481#issuecomment-6253705604)中的 A：侧边栏与原生 macOS 通知同卡实现。
- 初始代码基线：`b3c2ffafb9ae39e32654c6692b2f35da3955d76a`；测试接线同步到 `efd92833488fb3432042682e81d7f4eec1a10d0d`（#482），后续集成 `6c782a630ceb4c83f4f6b1e09bdc645ac8e87092`（#479复制功能）。任务分支：`feat/481-session-completion-notifications`。
- 本规格 v1.3，2026-10-19。v1.1 同步最新基线与[测试交付生命周期](../vega-rust-test-delivery-lifecycle.md)；v1.2 仅记录经预先审查的测试显示环境接线修正，A1–A17 目标和预期不变，原容量输入及全部断言保留；v1.3 仅同步#479共享stream方法与集成验证记录，四份测试内容保持v1.2已审查哈希。主 agent 负责规格、审查和集成；同一专职子 agent 依次负责先行测试与生产实现。
- 沿用 GPUI Kit 已公开的 `App::show_system_notification`、`dismiss_system_notification`、`on_system_notification_response`。禁止新增依赖或直接平台 FFI。
- 不新增消息计数、通知历史、声音、单会话静音开关、分组标记或排序规则，不修改模型原文或用户配置。
- 本地只读源码及执行快速差异、格式和静态检查。编译、Clippy、测试均由现有 PR Actions 执行。

## 验收矩阵（先于实现计划）

测试使用合成会话、隔离存储和 GPUI 测试平台，不访问真实 provider、用户数据库、系统通知或其他外部应用。使用真实生产决策和会话服务；替身只隔离系统投递和外部执行。新增测试不得恢复已从 master 清除的历史测试集合。

| 用例 | 前置与操作 | 可观察结果 | 层级与证据 | 当前状态 |
|---|---|---|---|---|
| A1 | 会话 A 运行，主窗口聚焦且实际显示 A，进入完成/失败/非用户中断终态 | 不新增未读，不发系统通知；保留真实终态和正常收尾 | GPUI 终态入口与存储读取 | 进程内 GREEN；见证据 |
| A2 | 窗口聚焦显示 B，A 进入上述三个终态 | A 的 unread 持久化为 true，B 不变；有合格输出时通知 A | GPUI 终态入口、平台通知与持久存储 | 进程内 GREEN；见证据 |
| A3 | 窗口不聚焦，当前路由分别为 A/B，A 结束 | 两种情况均置 A 未读并按条件通知；后台路由缓存不等于可见 | 与 A1/A2 构成聚焦 × 会话 × 终态矩阵 | 进程内 GREEN；见证据 |
| A4 | Settings 遮住缓存的 A，窗口聚焦时 A 结束 | A 视为后台；不因缓存 id 相等抑制通知或未读 | GPUI 实际路由与平台通知 | 进程内 GREEN；见证据 |
| A5 | 用户主动取消 A；分别当前可见与后台；另测 review 内部 handoff | 用户取消不发系统通知；后台取消终态仍保留 unread，当前可见不新增 unread；内部 handoff 继续同一用户任务，不冒充结果完成；不把其他 Interrupted 当用户取消 | GPUI 取消、handoff 与真实终态状态 | 进程内 GREEN；见证据 |
| A6 | 同一 run 重复 Finished，或旧 generation 在新 run 中迟到 | 对同一 run 最多一次通知/完成副作用；旧消息不终结新 run，不覆盖其 running 状态 | 真正 ingress、RunningThreads 与平台记录 | 进程内 GREEN；见证据 |
| A7 | 同一会话连续两次后台 run，各有不同输出 | 每个新 run 各通知一次、内容归属各自本轮；不得沿用上轮摘要或去重键 | GPUI 两轮事件、通知 tag/body | 进程内 GREEN；见证据 |
| A8 | run 没有输出、仅 thinking、仅工具、仅 commentary、仅空白文本，且历史有旧答案 | 均不发通知；后台仍置未读。历史答案和内部输出不能充当本轮结果 | 真实流投影、终态入口和平台记录 | 进程内 GREEN；见证据 |
| A9 | run 有普通 assistant 输出，或 commentary 后有可信 Final；包含多行、中文与长文本 | 取本轮最后合格 assistant 文本；不混入 commentary/thinking/工具/历史；正文单行、UTF-8 完整且有界 | 流投影与最终平台通知内容 | 进程内 GREEN；见证据 |
| A10 | 有结果的完成/失败/非用户中断，分别有/无会话标题 | 标题使用会话标题或“新任务”；失败/中断正文明确终态，不能看起来像成功 | 平台通知的可见内容 | 进程内 GREEN；见证据 |
| A11 | GPUI 通知投递不可用，后台 run 结束 | 正常结束并持久化 unread，没有应用错误或重试/自制授权 UI；无系统通知交付 | 无平台 app identity 的测试平台；真实拒绝见 M5 | 进程内 GREEN；见证据 |
| A12 | 已未读的 A 经元数据刷新、组织投影/重排或项目展开，然后真正访问 A | 所有刷新不清 unread；组织操作不改任务时间/项目归属；元数据 reload 保留现有 open_thread 活动时间语义，visit_thread 才清未读并持久化 | 会话/组织服务与存储读取 | 进程内 GREEN；见证据 |
| A13 | 后台完成后重新激活窗口，实际显示 A；另测 Settings 遮挡及已前台时手动标未读 | 新的真实访问通过 visit 清未读；Settings 仍遮挡时保持 A 未读；普通 render 不清除用户在已可见 A 上刚作的手动未读标记；不顺带清 B | 窗口 activation、导航与存储 | 进程内 GREEN；见证据 |
| A14 | A 未读又在运行，随后停止；分别无操作区、hover 与键盘操作区 | 尾部单一状态：操作区 > running > unread > 既有时间戳；保持现有几何、主题色和操作可发现性 | 实际行投影/渲染决策，必要的 GPUI 布局检查 | 进程内 GREEN；见证据 |
| A15 | 从 B/Settings 点击 A 的通知；A 仍存在 | 激活主窗口、导航 A、按 visit_thread 清 A 未读；B 的业务数据不变，不创建消息或新 run，不丢草稿 | GPUI simulate_system_notification_response 与持久状态 | 进程内 GREEN；见证据 |
| A16 | 点击无关/未知 tag，或目标已不存在的通知 | 忽略无关响应；不存在的任务不重建、不误导航、不开始 run；不得以清其他任务未读作为回执 | GPUI 响应与会话/消息状态 | 进程内 GREEN；见证据 |
| A17 | 未读写入后紧接真实访问/切换；或手动标记已读在完成前发起但稍后返回 | durable 状态遵守真实操作/完成的先后顺序，不由过期异步结果重新置未读或清错对象；访问失败保持真实错误恢复路径 | 隔离存储与现有导航 mutation 时序 | 进程内 GREEN；见证据 |

所有正常/失败/取消矩阵验证运行不再活动及 `RunningThreads` 的真实状态。通知次数只用于“不得重复通知/不得通知”的业务契约，结合正文、存储和运行终态断言，不能以 mock 调用数量代替行为。

## 行为契约

### 可见性与未读

1. 仅实际展示该会话且所在主窗口 active，才算用户正在看结果。Settings 遮挡、另一会话/路由、另一窗口或应用获得焦点均不满足。使用 GPUI 窗口真实激活状态；不能用编辑框 focus 或最近一次截图猜测。
2. 当前 run 接受一次有效终态时按当时可见性决定是否置 `ThreadUpdate { unread: Some(true), .. }`，沿现有会话服务持久化并刷新侧栏。未读更新本身不改活动时间；不改变运行源、排序、置顶、项目关系或现有 `open_thread`/reload 的活动时间语义。
3. 完成、失败和非用户中断同等置未读。用户取消只抑制系统通知；后台取消仍遵循终态的未读规则。
4. 普通刷新不能清未读。导航进入会话、重新激活窗口后真正显示已未读的当前会话，以及 Settings 关闭使其重新可见，均复用 `visit_thread` 的真实访问语义。只在真实可见性/访问转换发生时触发，不能每次 render 都清未读，否则用户在已可见会话手动标未读会立即失效。不得把 Settings 激活、通知提交或通知被移除算作访问。
5. 终态未读、`record_navigation_visit` 和手动 unread-only 更新采用与 sidebar `open_thread` 相同的短同步会话服务事务，在 UI 操作序列内提交，避免旧异步已读写覆盖新完成。这里只统一 attention 写，不改 rename 等其他异步菜单操作，不加写队列/线程/schema；保留 action_pending/task_mutation_busy guard、fence、字段级合并与错误恢复。发生在真实访问/一次终态，不进入 token 热路径。失败不捏造已保存或已访问成功；通知不可用不能影响独立的未读持久化。

### 通知资格与摘要

1. 后台终态、有本轮合格可见 assistant 输出、非用户主动取消时才调用系统通知。空输出/纯 commentary/thinking/工具的抑制优先于兜底文案。
2. 从当前 run 对应的真实 `ConversationStream` 文本/身份/response 投影提取摘要。已有历史、用户输入、工具结果、thinking、错误诊断、自动标题和预测请求均不是本轮 assistant 答案。
3. 有可信 response metadata 时只取合格 Final 范围；纯 Commentary 不通知。无 metadata 的普通 provider assistant 文本可用。metadata 无效或冲突时不猜测 Final，不把已知 commentary 当答案。
4. 选择本轮最后一条合格非空 assistant 文本。正文沿现有文本处理惯例合并空白为单行、剔除其他控制字符并按字符边界截断；总长最多 160 个 Unicode scalar，包含失败/中断前缀和截断时的“…”（前 159 个字符加一个截断号）。未超限不添加截断号。禁止字节切片破坏 UTF-8。
5. 标题用去首尾空白后的会话标题，空标题用“新任务”。成功正文为摘要；失败/非用户中断分别使用“任务失败”/“任务已中断”加摘要，以保留会话标题同时区分终态。“任务已完成”是完成兜底，但 C6 的无合格输出抑制仍优先，不能为空 run 发兜底通知。
6. 原生通知 `tag` 采用 `vega-run-{thread_id}-{generation}`。同一 run 由 generation/现有 finish 所有权严格去重，平台 tag 替换只是额外保障，不能把重复 show 当“不发重复通知”。连续不同 run 不互相去重；本卡不承诺合并同会话不同 run 的通知。
7. 资格判定覆盖普通运行与现有 review 后续运行的有效终态入口。`delegated_parent_thread.is_some()` 的内部 child 不单独发顶层完成 attention；自动标题、预测等辅助请求也排除。review 内部 handoff 是同一任务的执行切换，旧 worker 不产生结果完成 attention，后续实际 run 按通常终态处理。
8. 用户主动停止须记录显式原因，不能仅凭 cancellation token 或 `finish_composer_run` 返回值判断，因为两者也用于内部 handoff/非用户中断。自然 End 为完成，Length/TurnLimit 与非用户 Interrupted 为中断，Error/失败 Finished 为失败；review handoff 与用户 stop 分别记录。
9. steering 可能使一个 run 包含多个 message id，须记录 run-owned message ids，并从这些消息中选最后合格输出；不能只用第一个 root_message_id 或整条历史最后答案。

### 平台与点击

1. 在应用级安装一个响应处理器，避免多个实体重复注册覆盖进程级 handler；设置已存在的 Vega 应用 identity，不引入授权界面或主动开关。
2. 权限首次需要才由 GPUI 请求。拒绝、无 bundle 或平台不支持时使用其 no-op 降级；不提示失败、不重试请求、不阻断 run 收尾。真实平台授权行为由用户在正式 bundle 手测，进程内测试不冒充真实授权验收。
3. 只接受本卡已投递记录对应的通知响应，定位既有会话后激活主窗口，走当前导航/草稿保护和真实访问路径。不要解析任意外部字符串后创建会话或运行。
4. 点击是用户的明确访问，允许清该会话 unread；不得新增 message/run、改变其他会话的业务数据、丢弃草稿或清除权限/待回答等状态。被导航 guard 阻止时保留原有可恢复提示，不能宣称已打开。
5. 成功导航后可以 best-effort dismiss 对应通知。未知 tag、目标已删除或窗口已销毁时安全降级，不扩大为新窗口恢复或跨重启历史系统。

### 侧边栏

沿用当前 accent 圆点、标题字重、真实 `RunningThreads` 与既有时间显示策略。集中表达尾部选择，确保 running、unread 与时间互斥；hover/键盘菜单等既有操作区优先。时间仅在原来允许的行中回落显示，不恢复已取消的常驻时间或新增可见说明。

## 实现计划与文件归属

验收用例冻结后按以下顺序推进，不允许提前生产实现：

1. 专职子 agent 在 `crates/vega/src/tests/`、`crates/vega_ui/src/conversation_stream/tests/`、侧边栏/会话模块的测试区写齐 A1–A17 所需的最少测试及必要 `cfg(test)` 接线。基线 #482 删除了旧测试 harness，本卡仅临时提供自身消费的最小隔离存储/GPUI/config fixture、GPUI test-support/dev dependencies 与其锁文件接线，不恢复旧测试或整套 runtime mock。开发 GREEN 后连同本卡 Rust 测试一起删除。测试只调用生产边界，不复制通知决策算法。
   - `vega/src/tests/session_completion_notifications.rs`：真实 `begin` → RunningThreads → `apply_agent_batch_ingress` 链、应用绑定/activation/点击与持久状态。
   - `vega_ui/src/conversation_stream/tests/completion_response.rs`：只读生产投影 `ConversationStream::completion_response(&[String], &App) -> Option<String>` 的消息归属、可见内容与 response metadata 边界。
   - sidebar 私有测试区：真实行尾选择 `session_row_trailing`（操作区、running、unread、既有时间、空）及 unread-only 更新的持久顺序/时间不变量。
   - 系统出口直接使用 GPUI `shown_system_notifications`、`delivered_system_notifications`、`simulate_system_notification_response`；生产 `app_notifications::bind(window, root_weak, cx)` 同时作为应用与测试的统一绑定边界，不另造测试通知决策或投递 recorder。
2. 主 agent 审查并提交测试先行补丁，与本规格一同进入 draft PR；现有 `.github/workflows/pr-check.yml` 产生 RED。目标 API 尚不存在时可记录最小接口缺失 RED，必须说明行为断言尚未执行；无关接线或环境错误不能充作 RED。
3. 有效 RED 后冻结测试文件内容、目标与预期，专职子 agent 再最小实现：
   - `crates/vega/src/main.rs`、新的应用通知模块与 `window/` 通知模块：应用 identity、单一响应 handler、窗口 activation/路由可见性、投递记录和导航。`window/navigation.rs::record_navigation_visit` 可沿 sidebar 已有的短同步 `visit_thread` 边界最小统一实际访问的写入顺序，保留导航 fence、草稿 guard、精确投影与错误恢复；resolver 只读操作仍异步。
   - `crates/vega/src/app_agent.rs`、`window/artifact.rs`、`window/agent.rs` 及必要的现有终态调用方：一次完成所有权、当前 run 消息身份、终态分类，接入未读与通知。不修改 runtime 的 headless 依赖关系。
   - `crates/vega_ui/src/conversation_stream/phase.rs` 等已有流投影模块：提供真实消费所需的只读摘要投影；跨 crate 不新增两套共享结构，不为测试扩大 public API。
   - `crates/vega_ui/src/sidebar/row_helpers.rs`、`threads_block.rs`、`sidebar/threads_block/organization/projections.rs` 和必要现有入口：主行与组织搜索投影的尾部互斥、未读更新及其顺序。不改组织/排序契约，不直接写 SQLite。
   - 会话存储入口只在实现已列明未读/访问契约确有需要时最小调整；不新增表或设置。
4. 专职子 agent 停止写入并交回文件清单、静态检查、冻结测试未改证明、风险及 `/tmp/vega-481-executor-report.md`。主 agent 审查/提交到同 PR，Actions 验证同一测试 GREEN 和最新 head 完整门禁。
5. 带同一冻结测试的 GREEN 验证后，先在同一 PR 固定完整提交 SHA、可访问测试源码/补丁与 RED/GREEN run/job、准确计数，再委派同一专职子 agent 用独立清理提交删除本卡 Rust 测试与独占 harness/config override/feature/dev dependency。删除前后生产行为保持相同，不删除失败或未执行测试来换绿。
6. 删测后最新 head 的完整 required check `check (fmt, clippy, test)` 成功、Rust 0 发现/0 执行且明确 `no Rust tests` 后，才转正式 PR 并由主 agent squash merge。空集不是测试通过，不替代上一步带测试 GREEN。卡片置 In review 等用户手测；合并时不关闭 Issue、不删除本卡分支/worktree、不安装或本地运行应用。

允许修改的模块仅限上述任务调用链与临时测试接线；不改生产依赖、CI、发布流程、用户配置/schema、其他功能。`vega`/`vega_ui` 的临时 dev dependencies 只用于既有 GPUI 测试支持和隔离存储，不升级保留包版本，不引入生产依赖，GREEN 后删除。主 agent 是 docs 唯一写入者，专职子 agent 是代码/测试唯一写入者。

## 检查、证据与回滚

本地可用：`git diff --check`、限定差异审查、`cargo fmt --all -- --check`。这些是静态证据，不等于编译/运行成功。

仅供现有 Actions 执行：

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/release_tests.py
python3 scripts/prepare_computer_use_tests.py
python3 -m unittest discover -s scripts/tests -v
cargo nextest list --workspace --message-format json
cargo nextest run --workspace
```

开发阶段新增 Rust 测试，不应走空集分支；带测试 GREEN 后独立删测，最终交付 head 必须走已存在的空集分支。按仓库规则每 5 分钟查询 CI；收到完成/失败通知或用户明确要求即时查看可立即查询。保留每轮原始失败、测试先行源码/补丁、实现顺序、同一测试 GREEN 与独立删测的可访问链接，不重试换绿。发现遗漏验收先停生产、独立重开先测后修周期。

| 证据 | 记录 |
|---|---|
| 生产未改基线 | 初始 `b3c2ffafb9ae39e32654c6692b2f35da3955d76a`；同步后 `efd92833488fb3432042682e81d7f4eec1a10d0d` |
| 测试先行提交与 frozen 文件摘要 | [`05c9d46e9be4354d3e97d9de5f30a1520e76ce9b`](https://github.com/puzige/vega/commit/05c9d46e9be4354d3e97d9de5f30a1520e76ce9b)，25 个定义；四个测试文件与初始 `28cdd7d1` 的 SHA-256 相同，固定源码见 PR #483 |
| Actions RED 与失败原因 | [39158242375](https://github.com/puzige/vega/actions/runs/39158242375)：目标 `app_notifications::bind`、`completion_response`、`session_row_trailing`/`SessionRowTrailing` 接口缺失；行为断言尚未执行，发现/执行计数未取得。Format、Python 步骤成功 |
| 生产实现提交与先后顺序 | 2026-10-18 19:25 UTC 核实接口缺失 RED 后授权实现；`fd639d860fe9365b184044323d399a2092fbb377` 首轮编译检查因 running 容器缺少控件 ID 失败，行为断言未执行；`c6cf4ed91aa6a030a1fde1bb6cb9cd92233933c7` 补上 ID，保留 aria 标签与冻结测试，Format/Clippy 成功 |
| 首次实际执行及测试接线修正 | [39160375451](https://github.com/puzige/vega/actions/runs/39160375451)：25 发现/执行，24 通过、1 SIGKILL、0 跳过，未取得 GREEN。容量保护用例在强制渲染1 MiB+1连续文本后166.661秒被终止，不能仅凭日志断言 OOM。静态排版路径存在重复 shape 剩余长文本的资源放大；本卡不改既有生产排版。经[修正前审查](https://github.com/puzige/vega/pull/483#issuecomment-6254481111)，仅在原 oversized 输入前加入 `f.route(false, true, true, cx);`，先用真实 Settings 隐藏同一B editor，保留live composer、1 MiB+1原输入、实际通知点击与全部原断言。修正后该文件 SHA-256 为 `448d898a826b8fc7b0bdc4b6cf61e4162ef81c7f537800043dd343e57c27e28a`，删去这一行与原文件逐字节一致，其余三份测试文件未改；提交链接在 PR 固定，再验证完整 GREEN |
| 旧基线带同一测试 GREEN | 完整 head [`98fa9dbdb92997221452dd8a29a4635e655fdca6`](https://github.com/puzige/vega/commit/98fa9dbdb92997221452dd8a29a4635e655fdca6)，[Actions39161474919](https://github.com/puzige/vega/actions/runs/39161474919) / [Nextest117775504853](https://github.com/puzige/vega/actions/runs/39161474919/job/117775504853)：25发现、25执行、25通过、0失败/跳过，忽略数未单列；容量用例2.477秒通过。Python5+15+14=34执行通过，Format、全目标Clippy、required check成功。[GREEN留存记录](https://github.com/puzige/vega/pull/483#issuecomment-6254557028)含四份固定源码/完整SHA-256及原始输出；已在删测授权之前固定 |
| 最新基线集成与同测验证 | 清理前fetch发现master更新，未删除测试。同步到`6c782a630ceb4c83f4f6b1e09bdc645ac8e87092`，测试提交变为`ed562597bc83f881e3c6b93f7c3d175862085118`；仅处理phase.rs同处插入冲突，完整保留双方方法。通知摘要方法SHA-256=`94faff2ea9176069ea10725955b568e20b433804ba04eee54e744c989762c01d`未变，移除本卡方法后phase.rs与上游逐字节一致；其余13个上游文件未变。四份测试内容未改，完整永久源码已在[PR读回核实](https://github.com/puzige/vega/pull/483#issuecomment-6254579238)。最新集成完整head `defd578fe44a6a3c1f231d228471eabf2fcfd446` 的 [Actions39162382524](https://github.com/puzige/vega/actions/runs/39162382524) / [Nextest117778207411](https://github.com/puzige/vega/actions/runs/39162382524/job/117778207411)：25发现、25执行、25通过、0失败/跳过，忽略数未单列；容量用例1.837秒通过。Python34、Format、全目标Clippy及required check全部成功。[最新GREEN固定证据](https://github.com/puzige/vega/pull/483#issuecomment-6254676408)在重新授权删测之前发布 |
| 独立删测 / 最终 head 空集 required check | 最新集成GREEN固定后，独立第三提交仅清理16个允许路径：6个测试/fixture文件与独占接线，静态25→0，371个Rust源码保留，无测试条件属性/可执行doctest残留。14个生产路径去除明确测试投影后与GREEN相同；454个其他文件及73个受保护路径未变。Cargo.lock/两manifest/session/updater/streammod共6文件与最新基线逐字节一致，924→914个包，仅移除原10个测试独占包，914个保留身份/版本/source/checksum不变。[完整恢复补丁](https://github.com/puzige/vega/pull/483#issuecomment-6254746294)已从GitHub读回核实SHA-256=`560ab4cfbc20a0337cb255ebbbc59d113cebde1d8d1a6db5e85e2aa4e33755ac`，仅静态git apply --check成功，未实际恢复或运行。最终head的0发现/0执行、`no Rust tests`与完整required check由PR最新提交原始云端记录确认，本文不预填结果 |
| 真实 macOS 通知/权限/点击 | 待用户手测 |

用户手测：

- M1：在 A 发任务后切到 B，完成后 A 显示未读圆点，running 消失；真实进入 A 后清圆点。
- M2：A 运行时切到其他应用，完成后收到会话标题及结果摘要；点击后 Vega 激活并进入 A，不发起新的 run。
- M3：持续前台阅读 A 直到完成，无未读和通知；前台显示 Settings 时完成按后台处理。
- M4：分别验证失败、非用户中断和用户取消；失败/中断标识准确，取消无系统通知；纯 commentary/工具/thinking 与空输出不产生通知。
- M5：在正式 bundle 首次需要时允许/拒绝权限。拒绝后 run 正常结束、侧栏仍标未读，后续 run 不反复要求授权。平台声音沿用现有 API 的无声音内容，不额外设置声音。
- M6：浅/深主题查看 running 与 unread 同存、停止、hover、菜单和键盘入口，确认尾部互斥、槽位稳定且操作可发现。

回滚为撤销本卡 squash 提交并走现有 PR/云端发布流程；无 schema/配置变更。现存未读数据继续沿原字段和手动/visit 语义处理。用户手测通过前保留 Issue 开放及本卡资源。
