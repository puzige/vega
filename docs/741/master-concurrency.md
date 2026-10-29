# master 流水线单并发

关联：[Issue #741](https://github.com/puzige/vega/issues/741)。基线：`3589d43245537a50d43f3fd3b7423577e024416b`。

## 范围与验收

用户要求 master 流水线同时只运行 1 条，不改变不同 PR 的检查并发。本次仅调整 master 入口，保留已有发布实现与门禁。

| 场景 | 预期结果 | 验证方式 |
|---|---|---|
| 多个 master push 触发 workflow | 同一固定并发组至多运行 1 条，其余排队，不取消正在运行的任务 | YAML 静态检查；实际调度待后续正常 Actions 观察 |
| 多条 master 任务等待 | `queue: max` 保留等待任务，而非默认替换已有 pending；仍受 GitHub 100 条 pending 上限约束 | 配置与官方契约核对 |
| master 调用共用发布 workflow | 入口组与发布 job 组不同，不发生调用方等待自身；原发布共享锁、固定 SHA 和防倒序保护不变 | 差异及分组静态核对、独立审查 |
| 不同 PR 以不同速度完成检查 | PR Check 配置逐字不变，不因 master 锁互相阻塞或取消；不承诺按 PR 创建或检查完成顺序发布 | 基线字节比较 |
| 合并本次 CI 配置 PR | 分支正常执行 required checks，通过并完成独立审查后，在 squash 提交标题中添加 `[skip ci]`，跳过该次 master push 的自动构建/发布 | PR 检查及最终合并提交读回 |

## 最小实施

1. 在 `.github/workflows/cicd.yml` 顶层新增固定 `vega-master-pipeline` 并发组，设置 `cancel-in-progress: false` 和 `queue: max`。GitHub 的并发组本身将运行数量限制为 1；不是使用 `concurrency: 1` 字面值作为数量。
2. 保留 `.github/workflows/release.yml` 的 `vega-stable-publication` 组，避免 reusable workflow 与调用方共用锁；`.github/workflows/pr-check.yml` 完全不变。
3. 同步 `docs/vega-release.md` 的队列说明。GitHub FIFO 按开始等待该组的时间，不保证 push 事件顺序，已有代码祖先检查继续拒绝倒序发布。

## 验证与限制

- 本地仅快速 YAML 解析、结构与差异静态检查，不运行 Vega、编译、打包或测试。
- 交由非实现者独立审查，并确认本次最终 head 的 required check `check (fmt, clippy, test)`；不更改或跳过 PR 门禁。
- 不新增 Rust 测试或依赖，不触发人为发布、空提交或多 push 来制造排队。真实调度未验证时不标记为通过。
- 不改权限、缓存、发布资产、版本分配及安全保护；无产品界面文案变化。
- 本次合并按用户要求跳过 CI，因此不以该次 merge 声称已验证 master 调度。后续正常 master push 使用新配置；用户确认前 Issue 保持开放。
- 回滚：撤回 master 入口并发块及对应说明，恢复仅发布 job 串行的原行为。规格偏离：无。

参考：[GitHub workflow concurrency](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)。
