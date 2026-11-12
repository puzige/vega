# 后台 Shell 控制提案与终态释放

## 范围和事实

基线：2a29adf9b2ea78eaf59d591838531b45adc7373e。用户授权修复后台等待/取消提案、保留严格参数与真实清理状态，并补失败后运行状态释放回归；不升级、重启、清理远端，也不重跑结果未知命令。

0.1.413 现场诊断为 tool_proposal / approval_audit；具体提案未落库，不能断言具体工具。代码确定存在 runtime 已规范化 bash_wait/bash_cancel，而 conversation validate_runtime_proposal 把非空控制输入按未知工具拒绝。失败收尾的 outcome_unknown 是独立真实事实，不能变成成功或自动清标记。

## 验收用例（先于实现）

| 前置与操作 | 可观察结果 | 验证 |
|---|---|---|
| 后台会话收到合法 wait/cancel 提案 | 提案落库、审批/运行/终态可持久化；不会中断父任务 | SQLite 事件链回归 |
| wait 的 cursor/wait_ms 缺省、边界值 | 与 runtime 规范化契约一致；有效输入通过 | 参数回归 |
| 非 ULID handle、超时越界、负数、未知字段或 cancel 带非 handle 字段 | 原始非法输入不放行；runtime 产生的 {} 拒绝投影可落库 | 参数/拒绝链回归 |
| 非控制未知工具携带参数 | 继续拒绝，不扩大工具白名单 | 负对照 |
| 等待 running 与终态、取消终态及 outcome_unknown | 状态/exit/cleanup 真实性校验仍有效；不将 running 当完成 | 回执与事件回归 |
| worker 报 Error 但仍在 drain | 不提前释放替代任务入口 | 通道/控制器回归 |
| worker 完成失败或通道断开 | 控制器、composer、sidebar 运行态释放；可开启下一轮；旧 generation 不影响新 run | 状态回归与实际接线核对 |

## 最小实现计划

1. 在 conversation 提案边界为 bash_wait/bash_cancel 增加专用分支，复用/收紧已有控制参数验证；仅允许明确的 {} 拒绝投影，不把未知工具或非法参数泛化为合法。
2. 检查终态释放链；保留 worker drain 前禁止重入的安全边界，仅修复实际证实缺陷，不用持久 failed 替代 worker 完成事实。
3. 必要开发回归验证提案→审批→运行→终态和失败/断连释放；不接真实模型/用户命令，不增加依赖。
4. 本地验证、非实现者审查；留存可恢复测试源码和结果后清理开发 Rust 测试；最终 head required CI 后合并，In review 等用户手测。

## 非目标

不改进程跟踪容量/清理算法，不重放或修复用户未知命令，不改工作区 blocker 策略，不通过隐藏错误或提前释放未完成 worker 绕过安全。
