# Full access 危险命令审批审计一致性

关联：[Issue #731](https://github.com/puzige/vega/issues/731)。初始基线：`1a75cd86855b572eb9f083cd922867637eb5f1f8`；静态验证前已快进至 `bbe1df89faf3c9bdca934ade5593cca25ade4389`，上游增量仅涉及独立的文件预览修复，本卡权限契约未变。

## 范围与依据

用户确认：移除 Full access 下重复的危险命令审批要求，保留有效授权校验，其他模式不变；补回归测试。本轮已获准提交并走 CI，沿用本地只做静态检查的限制；前轮不提交或推送的边界不再适用于本轮。

#695 已将授权来源改为所选权限模式和 policy v2 运行权限快照；旧 `full_access_grant` 不再是授权前提。本卡不恢复旧 grant 校验，不调整模式选择、授权重置、任务/运行身份、权限 epoch/revision、delegation 或独立能力边界。

静态根因：`decide_execute_permission` 对 Execute + FullAccess 产生 `Once / FullAccess / note=None / danger=None`，但 `bash_danger_audit_matches` 在命中危险命令时仍强制要求 danger 记录，并拒绝 FullAccess 来源。同一校验函数用于实时审批，以及经 `validate_recovered_projection` 的终态、恢复/复用校验，因此只修入口会遗漏保存和复用。普通 Bash 历史展示不必全部经过该 matcher，需另验原终态与审批仍能投影；历史可读不代表获准重新执行。已知诊断为 `runtime_failure / tool_approval / approval_audit`；恢复拒绝说明是后续清理结果，不是原始原因。

## 验收

| 前置状态与操作 | 必须观察到的结果 | 验证方式/当前状态 |
|---|---|---|
| Execute + 有效 FullAccess，调用命中危险规则的 Bash | runtime 无额外 prompt；规范 FullAccess 审计通过实时审批，终态可持久化并重新投影 | 开发 Rust 回归；待 CI |
| FullAccess 工具成功、失败或取消后重新加载/复用 | 保留原审批和终态，不误判 corrupt，不重新执行 | 真实存储/事件/历史 API 的进程内回归；待 CI |
| FullAccess 审计含 Deny/Always、note 或 danger | 仍拒绝不符合规范的审计 | 审计/事件校验回归；待 CI |
| Confirm/Auto/ReadOnly，危险命令且存在普通 allow 规则 | 仍走原危险确认；拒绝/超时/取消/不可用不获准执行，ReadOnly 不获得写权限 | runtime 决策与会话审计联合回归；待 CI |
| Ask/Plan 中 mutating Bash | capability 拒绝，不能靠 FullAccess 进入 Execute | runtime capability 回归；待 CI |
| 恢复运行缺少权限快照，或任务身份、epoch、revision、policy、模式发生变化 | 现有授权匹配拒绝恢复；有效快照为阳性对照 | 运行权限快照/恢复 API 回归；待 CI |
| 普通 Bash 与旧合法 Danger/恢复拒绝记录 | 原有审计语义仍可读 | 开发回归；待 CI |

本卡测试只验证进程内权限决策、审批校验、持久化、历史投影和恢复资格，不启动 shell、Git、网络或真实危险命令；测试源码中的命令只是输入数据。静态审查不能证明运行测试已通过或真实桌面验收完成。

## 最小实施计划

1. `crates/vega_conversation/src/agent/pipeline.rs::bash_danger_audit_matches`：在输入签名与危险检测成功后，为危险 Bash 的规范 `FullAccess + Once + note=None + danger=None` 审计提供一致的接受分支；不改变其余来源或检测规则。
2. `crates/vega_conversation/src/agent/full_access_audit_tests.rs` 与 `agent/mod.rs`：新增开发期回归模块，调用真实 runtime 决策、会话事件/投影、Store 和权限快照 API，不复制权限算法、不恢复旧测试集合、不加依赖。
3. 静态复核实时审批、终态和恢复/复用校验共同使用该函数，并单独检查历史展示；保持 authority、capability、取消、command scope 及 MCP/Computer Use 路径不变。历史审计可读不等于授权执行，恢复仍由现有权限快照验证。
4. 本地只进行格式、diff whitespace、测试接线及安全边界静态检查；固定未提交内容版本，交非实现者只读审查。

## 非目标与风险

- 不删除危险检测或权限系统，不改 Confirm/Auto/ReadOnly/Ask/Plan 行为，不改沙箱、网络/目录范围扩围、凭据及外部能力审批。
- 不调整通用失败文案，不隐藏错误，不修改用户数据库或安装的应用，不重试原提交/推送命令。
- 最大风险为把“审计来源声称 FullAccess”误当成新的授权。本次仅校正来自已授权运行的审批记录一致性；选定模式及运行权限快照继续是实际授权来源，旧 grant 不恢复。
- 新增 Rust 测试仅留在本地开发工作树。后续如获准提交/推送，应先由现有 Actions 运行开发回归，再按仓库政策留痕和清理，最终无 Rust 测试版本通过完整 required CI 后才能合并。前轮仅本地实施；本轮已获准提交、推送和现有 PR Check。

变更记录：2026-10-28，依用户确认范围建立规格；明确 #695 的当前授权语义优先于历史 grant/危险二次确认规则。

## 前轮本地交付记录

- 分支：`feat/731-full-access-danger-audit`，基线 `bbe1df89faf3c9bdca934ade5593cca25ade4389`；没有本卡提交。
- 生产改动仅为 matcher 新增 5 行；另有 2 行开发测试接线、8 个开发回归测试和本规格，无新增依赖、CI、UI、危险规则或授权实现改动。
- 已执行静态检查：Rust 1.98.0 的 `rustfmt --edition 2024 --config skip_children=true --check`（三个变更 Rust 文件）退出 0；`git diff --check` 退出 0；未跟踪测试/规格的 `git diff --no-index --check /dev/null <file>` 均无空白诊断（退出 1 表示新文件差异）；只读清点确认恰好四个变更文件、8 个测试、无 ignore、无暂存。
- 非实现者独立只读审查：无发现；核对实时审批、终态/复用/恢复、其他模式及有效运行权限快照边界，也未发现静态可确认的测试 API/fixture/预期问题。审查绑定下表版本；审查后仅追加本节交付记录，未改生产代码、测试或验收。
- 未执行：Vega、本地编译/构建/测试、CI、桌面验收。未提交、推送、开 PR、合并或安装应用。不能将静态检查或审查结论称为测试通过；待后续授权进入云端验证。
- 跟踪卡保持 OPEN / In progress；本轮仅完成本地实施和静态审查，不代表已合并或用户验收。

冻结审查内容（SHA-256，均相对于本工作树；前三项位于 `crates/vega_conversation/src/agent/`）：

| 文件 | SHA-256 |
|---|---|
| `pipeline.rs` | `7224770fd8521746dc89620e8961c1868365e50dce940eedf8062a21716ac8a0` |
| `mod.rs` | `e97adf5619f7311aba4a05c0010990c86c77c2453e5c512b982fd9dd03c416dd` |
| `full_access_audit_tests.rs` | `e2b3d8076001fcdccf0ec111729c38c0ad065df9302048485c5e20c24d86a432` |
| `docs/731/full-access-danger-audit.md`（追加本节前） | `a773efe9ff9a72e6b0ac4bb4c1e61458c07c15323a668aaf43251e927a2490b9` |

## 本轮云端交付

- 复用上述精确版本的生产修复与 8 个回归测试，未改测试断言或验收；worktree 已安全快进至 `0b31956674ec7315828a87f65a9654c460083d51`。上游 permission_contracts 增量仅为首次提交的自动标题填充，未改本卡权限恢复函数。
- 开发版本使用既有 PR Check 的全目标 Clippy、Python 检查和 Nextest 实际运行回归；不执行命令字符串中的危险操作。结果待 CI，未运行不标通过。
- 开发回归通过后，在 PR 固定可访问测试源码及完整 SHA，按仓库政策清理开发测试与独占模块接线。最终 head 再跑完整 required CI；独立审查及门禁通过后 squash merge，Issue 保持开放并进入 In review 等待用户手测。
- 无 UI 文案、权限检测规则、执行授权或 scope 行为变化；规格偏离：无。回滚为撤回 matcher 的 5 行兼容分支（会恢复原缺陷），不修改用户数据或安装应用。
