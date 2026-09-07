# Vega R6 Diff 刷新呈现 SDD

版本 v0.1 · 2026-09-05 · Owner Codex · Executor 原生 Luna / max

## 1. 范围与问题

本卡只收敛 Diff 面板的刷新呈现。当前 `VegaWindow::schedule_diff_refresh` 在每次
请求（首次打开、750ms 后台轮询、工具 terminal 变更和用户 Retry）都把
`DiffView.refreshing` 置为 `true`；当已有快照为空时，UI 因 `row_count == 0`
显示整页 `Refreshing workspace diff…`。后台轮询因此会让 clean workspace 的空态
反复闪烁，也会让已有内容被误认为暂时不可用。

## 2. 冻结契约

### 2.1 刷新意图

刷新请求保留既有单飞、750ms cadence、取消、route identity、sequence、coalescing、
projection 和 stale-result 围栏。只增加调用方可观察的呈现意图：

| 意图 | 触发 | 进度呈现 | 旧快照/空态 |
| --- | --- | --- | --- |
| `Initial` | 打开 Diff，尚无快照 | 显示首次加载进度 | 尚无内容 |
| `Retry` | 用户点击 Retry | 显示显式重试进度 | 保留已有快照或空态 |
| `Background` | 750ms poll、tool terminal / workspace action refresh | 不显示整页 loading | 保留已有快照或空态 |

`Initial` 与 `Retry` 只控制可见进度，不改变 worker、snapshot 或权限语义。
后台刷新完成后仍以真实 `WorkspaceSnapshot` 替换内容；只有完成结果才更新 stats、
文件列表和 projection generation。

### 2.2 状态呈现

- 无快照且 `Initial` 请求进行中：显示首次加载文案。
- 有快照且任何刷新进行中：保持当前 rows、header 和 stats；只对首次加载和显式
  Retry 显示轻量 header 进度标记，Background 不显示周期性刷新标记，也不替换正文。
- 无快照且后台刷新进行中：保持当前空态，不把它升级成 loading。
- 刷新成功：清除 typed refresh error，应用完整 snapshot；snapshot 的非空→空变化
  必须自然得到 `0 files +0 -0` 和 `No workspace changes`。
- 刷新失败：保留既有快照/空态，同时保留 typed `GitWorkspaceErrorCode` 和可用
  Retry。失败信息不得被 loading 或空态遮蔽；没有快照时可显示错误页，有快照时显示
  与当前内容并存的错误提示及 Retry。保留的快照是只读显示：pending/failed projection
  capability 清理，错误态拒绝新的 projection request 和晚到 projection apply；成功
  新快照后才恢复展开和 projection。
- 显式 Retry 在失败期间不得重复发起；完成前 Retry 不可重复触发。

`refreshing` 表示 worker 是否仍在途；`show_refresh_progress`（或等价的最小状态）
表示首次 / Retry 是否需要可见进度。二者不能再由 `row_count == 0` 互相推导。
当请求 coalesced 时，显式 Retry 的可见进度优先级高于后续 Background，不能被降级。

### 2.3 事件和围栏

`DiffRetryRequested` 保持原字段和 subscription。窗口层将其映射为 `Retry` 意图；
poll、`WorkspaceToolTerminal` 和 workspace action 完成映射为 `Background`，打开映射
为 `Initial`。请求仍由 `ActiveDiffRoute::request_refresh` 分配 sequence；coalesced
刷新只保留最新排队序列，任何晚到结果继续按 route / sequence / snapshot generation
丢弃。取消或 route 失效时不投影空 snapshot，也不清除另一条有效 route 的视图。

## 3. 空快照 stats 结论

历史报告把 clean workspace 的旧 stats 残留列为线索。本树当前
`git_workspace::snapshot::build_snapshot` 从本次 `parsed.files` 重算
`file_count`、additions 和 deletions；`public_files` 为空时这些 stats 由现有聚合逻辑
得到零值，且 `DiffView::apply_snapshot` 用该 snapshot 重建 rows。因此静态源码不能
推出“空快照仍显示旧统计”，本卡不添加 UI 强制清零或修改 service 缓存。

验收补一条真实 app/controller 快照序列：同一路由先应用非空 snapshot，再应用真实
clean empty snapshot，断言 rows、header stats 和空态分别为零/`No workspace changes`。
若该 E2E 失败，按 snapshot/service 根因修复；不以 UI 投影覆盖错误。

## 4. 实现边界

- 只改 Diff refresh intent / view presentation 和对应真实 controller/UI 回归。
- 保留 diff 文件导航、lazy projection、ListState/scroll、cancel、route、sequence、
  coalescing、projection fence、所有安全断言和现有 750ms cadence。
- 不改 provider、store、migration、DDL、依赖、Git 安全命令、权限逻辑或性能策略。
- 非测试 Rust 不新增 `unwrap` / `expect`；不读真实用户文件，不访问 Keychain 或真实
  provider，不运行 bench/soak。
- 每个 Rust 文件保持不超过 1000 行，最多 3 个本地 commit。

## 5. 验收设计

优先真实 app subscription / `VegaWindow` controller 与 owned temporary Git repo：

1. 首次打开无 snapshot：显示一次加载进度，完成后呈现真实内容。
2. 已有非空 snapshot 的后台 refresh：worker 在途期间 rows/header/stats 保持，完成后
   应用新 snapshot；不出现整页 loading。
3. clean empty snapshot 的后台 refresh：空态保持，不闪成 loading；stats 为零。
4. typed refresh failure：已有内容或空态保留，错误可见且 Retry 可再次请求；首次无
   snapshot 失败显示错误与 Retry。
5. 用户 Retry：显示显式进度、coalesce 仍只发一个有效 worker，成功/失败都回到正确
   状态；tool terminal refresh 使用 Background 语义。
6. route switch、取消、晚到结果和 projection request 继续由已有 fence 回归覆盖。

纯 UI helper 测试只验证不可稳定由真实窗口断言的字符串/状态映射；不增加颜色、布局
镜像测试，也不以 helper 结果替代 controller E2E。

## 6. 第二轮失败诊断补充

第二轮 workspace 回归中，`diff_refresh_intents_keep_content_during_background_and_retry`
在通用 pump 的超限处失败；原有成功谓词只等待成功快照，因此无法区分首次加载、Retry
或 clean-empty 阶段的真实终态错误。该测试手动调度 refresh，并未启动 750ms 后台 poll，
所以本补充不改变 cadence 或生产刷新行为。

诊断只调整该测试的观察方式：三段 pump 分别带有 `initial`、`retry` 和 `clean-empty`
标签，沿用原有 400 次虚拟 `DIFF_RESULT_POLL` 步进和每次 5ms 的 wall sleep。每次观察
记录无文件内容的安全摘要：`generation`、`refreshing`、`refresh_error`、`row_count` 和
`snapshot_stats`。当 worker 已终态且带 typed error，或终态未满足原成功谓词时立即失败并
输出该摘要；成功谓词本身保持不变。超限时同样输出最后摘要，不延长等待预算。

这项补充用于定位是哪个阶段及哪类 controller/worker 终态未达预期，不把 transient Git
process failure 改写为成功，也不以放宽 timeout 掩盖失败。若定向运行证明是 worker 或
fixture 在并行 workspace 下的实际故障，再按该错误的生产根因修复；否则不改生产调度。

## 变更记录

- v0.1：冻结首次 / Retry / 后台刷新呈现分离、typed failure 保留、空快照 stats 证据
  与真实 controller 验收边界。
- v0.2：补充第二轮 Diff 回归的三阶段终态诊断边界；保留原有 pump 预算和成功断言。
