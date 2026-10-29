# master 流水线只保留最新运行

关联：[Issue #741](https://github.com/puzige/vega/issues/741)、[PR #742](https://github.com/puzige/vega/pull/742)。基线：`3589d43245537a50d43f3fd3b7423577e024416b`。

## 范围与验收

2026-10-29 用户澄清并确认：master 只跑最新一条，不逐条排队；PR Check 保持现有 PR ref 分组。本节取代最初“排队且不取消”的验收。

| 场景 | 预期结果 | 验证方式 |
|---|---|---|
| master 旧运行尚未结束，又有新 push 触发运行 | 固定组 `vega-master-pipeline` 取消旧运行，保留新运行；不累积逐条发布队列 | YAML 结构与 GitHub 并发契约核对；真实调度待后续正常 Actions 观察 |
| master 已有等待运行，又有新运行进入该组 | 使用默认单 pending 规则，新的等待运行替换旧 pending；不使用 `queue: max` | 配置静态检查 |
| master 调用共用发布 workflow | 入口组与发布 job 的 `vega-stable-publication` 组不同，避免自锁；调用方取消会传递给其调用的发布流程 | workflow 调用关系静态审查 |
| 同一 PR 连续推送 | 保持 `${{ github.workflow }}-${{ github.ref }}` 与 `cancel-in-progress: true`，新检查取消同一 PR 的旧检查 | PR Check 与基线逐字比较 |
| 不同 PR 或 fork 同名分支各自更新 | PR ref 分组隔离，不因 master 组或来源分支同名而互相取消；不按来源分支合并分组 | PR Check 基线与分组核对 |
| 自动发布在取消前已保留 tag 或建立 draft | 不删除 tag/draft，不覆盖正式资产；允许跳过未完成版本，后续正常 master 运行仍按原版本与防倒序保护发布 | 保留 release workflow 和 scripts 的原始实现；静态风险核对 |
| 合并本次 CI 配置 PR | 最新 head 正常完成 required checks 和非实现者独立审查，再用标题含 `[skip ci]` 的 squash 提交合并，跳过本次 master 构建/发布 | PR 检查与合并提交读回 |

## 最小实施

1. `.github/workflows/cicd.yml` 顶层使用固定 `vega-master-pipeline` 组，设置 `cancel-in-progress: true`，移除 `queue: max`；GitHub 并发组本身限制同时运行一条，不使用数值 `concurrency: 1`。
2. `.github/workflows/pr-check.yml` 完全不变：保留 PR ref 分组，不同 PR 独立检查；required check、权限和任务拓扑均不改。
3. `.github/workflows/release.yml` 完全不变：保留人工 tag、手动重跑及自动发布共用的串行锁和原有 `queue: max`、`cancel-in-progress: false`。该内部锁不能阻止 master 调用方取消其自身发布流程；不让新 master 运行取消独立的人工发布。
4. 同步发布指南与入口规范。保留固定事件 SHA、缓存、版本保留、幂等、资产完整性和防倒序保护；不承诺 GitHub 按提交时间排序，不撤回已完成的发布。

## 验证与限制

- 本地只做快速 YAML 解析、配置结构与差异静态检查，不运行 Vega、编译、打包或测试。
- 独立审查绑定停写的精确 head；最新 head 的完整 required check `check (fmt, clippy, test)` 必须通过。旧排队版本的 CI 不代表本轮通过。
- 不新增 Rust 测试、依赖或 CI 阶段；不制造空提交、多 push 或真实发布来验证取消。并发取消有收尾延迟，不等同于原子中断，也不撤销已完成的外部副作用。
- 取消可能留下保留版本、未发布 tag 或 draft。按既有恢复流程处理，不自动清理或冒充已发布；旧 SHA 在新版本发布后仍受防倒序保护约束。
- 仅最终 squash 提交使用 `[skip ci]`；开发提交正常触发 PR checks。本次 merge 不用于证明实际 master 调度，后续正常 push 使用新配置。Issue 保持开放，等待用户观察确认。
- 回滚：撤回 master 入口并发块及对应说明，恢复仅发布 job 串行的原行为。规格偏离：无。

变更记录：2026-10-29，依据用户澄清将原排队验收改为 latest-only；PR Check 不改为来源分支分组，仍按 PR ref 隔离。原排队版本保留在提交 `23022f5dc8795066a80360dc4ab28e53b6918198`。

参考：[GitHub workflow concurrency](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)。
