# 续派拒绝诊断

## 目标与范围

Issue #763，基线 7a91c122。仅拆分 Supervisor::reserve 的资格拒绝与模型可执行指引，不更改允许集合、不自动转换或派发。

保留现有顺序：取消/父执行结束、输入与精确最新执行 ID、busy/占用先检查；随后 followup 优先报告父执行不匹配，再报告非 Completed 的实际终态。两者同时不满足时归属原因优先，因为等待或恢复旧 child 也不能解除跨父执行限制。resume 的准入与旧诊断不变，boundary、所有权、权限、容量和事务检查均不改。

跨父执行反馈明确要求按需 spawn_agent，交接目标、已有产物可访问链接及精确版本、已完成/未完成项、约束证据与下一步；新 Agent 不自动继承历史，不重复 followup、不用 resume 绕过。返回错误，不自动创建 Agent。文案沿既有工具错误链返回模型和展开详情，不新增 UI 提示。

## 验证

临时 Rust 行为测试验证实际资格判断函数的状态/占用/父归属矩阵、busy 优先、归属优先、具体终态及交接引导。不将这组定向决策测试冒充完整 Supervisor、暂停恢复或真实 App E2E；#760 接续证明及其已留档回归未修改。独立审查核对 reserve 接线和后续安全检查不变。自动测试仅 Actions，本地仅格式/差异检查。开发测试通过留档后按仓库政策移除，再检查最终 head。

开发版本 `c8f39d4b53d44cd40e253f82bb0a33a78e33b889`：[Actions 40893161501](https://github.com/puzige/vega/actions/runs/40893161501) 全部成功；Rust 3 发现/3 执行/3 通过/0 跳过，Python 5/15/40 通过，fmt/Clippy 通过。独立审查无发现。本地仅 cargo fmt、git diff --check，未编译/运行/测试。

[可恢复原始测试](https://github.com/puzige/vega/blob/c8f39d4b53d44cd40e253f82bb0a33a78e33b889/crates/vega_conversation/src/agent/subagents/continuation_diagnostics_tests.rs)，GitHub API 读回 blob `1778f3ed67c102147252945fe9b74f6a51a40414`。随后仅移除临时测试文件及模块接线，无独占依赖、无生产行为变化。最终 head required CI 待验证；空集不称行为回归通过。未用户手测。
