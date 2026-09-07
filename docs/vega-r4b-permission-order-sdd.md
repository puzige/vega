# R4b 权限确认事件顺序修复

v0.1 · 2026-09-05 · Owner root · Executor Luna/max

## 范围和来源

依据 tech spec §4.3、既有 PermissionQueue/PermissionCard 的 fail-closed 契约，以及本轮 R4b 源码复核。生产 runtime 先发送 ToolCallProposed 到 app channel，随后请求权限；独立 PermissionQueue listener 可以早于 app poll 应用工具事件。当前无工具卡时直接 Drop pending，会把合法确认请求变成 Timeout。本卡只修复此功能竞争，保留既有 UI 风格和默认权限模式。

## 冻结行为

1. ConversationStream 最多保留一个尚未关联工具卡的 PendingPermission。工具卡尚未到达与已经到达但身份不匹配必须区分。前者暂存，后者按原语义拒绝；不能凭 call_id 单独批准。
2. 精确匹配 call/tool/target 且属于当前 run/stream 的 proposal 到达后，才转换为现有 PermissionCard lease。重复通知不能生成两张卡、重复 resolve 或重复执行。
3. 暂存与正在显示的卡继续使用原有权限请求超时与 Drop 守卫，不延长 deadline，不预先批准。允许增加不暴露 payload 的 is_resolved 查询，清理已失效请求。不得引入新依赖、DDL、原始参数日志或敏感 Debug。
4. Settings、Stop/cancel、切任务、关闭窗口、run terminal/error 均清理 queue、暂存和 active lease；迟到事件不能重新出现权限卡。ToolCallFinished 清理对应请求，必须精确绑定；不能无条件清掉另一调用的合法 pending。无 proposal 最终按现有超时拒绝。终态先到也不得重新安装卡。
5. 保持 production app ingress 和队列通路；不要通过让生产 worker 等待 UI 或禁用并发来掩盖顺序问题。render 不做 IO。

## 验收

先写可失败的真实应用入口回归，再实现：owned temp repo、MockProvider、真实 start_agent_run/worker/app ingress、PermissionQueue 与工具 recorder。由确定性测试调度控制事件应用先后，验证 request-first 与 proposal-first，Once 恰好一次；拒绝/超时/身份不匹配零执行。补足重复通知、cancel 后迟到、无 proposal、terminal-first、切任务/Settings 清理。若某安全反例只能 unit seam 覆盖，在交付中明确，不能称全为 E2E。

保留现有全部权限安全断言。日志使用唯一名称，保留首次失败和复跑，不把 0 tests 算通过。先运行必要功能回归，再 fmt/clippy/build；联合全量由 root 安排。用户已延期性能测试，不跑 bench/soak。

## 归属与交付

工作树 codex/vega-r4b-permission-order，base d01ae6c。主要归属 ConversationStream 的权限处理、PermissionQueue 的最小只读状态 API、app 入口回归；不得修改 @file、thinking、Git 或其他 UI 行为。R5 在另一树实现 @file，共享文件由 root 集成并复核。

所有 Cargo 等 root 明确分配 target 后再运行。实现提交不超过 3 个；交付 docs/vega-r4b-permission-order-delivery.md，包含 HEAD、精确命令/原始日志、首次失败、剩余边界。禁止 push、master merge 或替换已安装应用。

## 变更记录

- v0.1：冻结暂存一条、精确身份、原 deadline、全生命周期清理及真实应用链验收。
