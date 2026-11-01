# 子 Agent 聚合条目保留时间比较

## 用户授权与范围

修复父对话流在子 Agent 聚合条目后丢失时间分隔的问题，并补对应回归测试。仅修改时间分组投影，不改子 Agent 生命周期、数据库、消息时间来源、正文复制或可见文案。

## 契约

`StreamEntry::Subagents` 是父助手的附属投影。时间分组遍历直接跳过它，不清空 previous、segment_owner 或 seen，不推进 saw_user，不使用子 Agent 的创建或完成时间，也不为聚合条目生成前缀；是否有可见行不影响此规则。

前后逻辑消息仍使用原权威时间和严格阈值：assistant → user 大于 1 小时；assistant → assistant 大于 10 分钟；恰好阈值不显示。压缩、摘要、技能来源、无归属的其它条目、显式时间断点与真实续接/Fork 连接点仍打断比较。重开、分页重算和实时子 Agent 插入共用同一遍历规则。

本契约修订原 [774 映射](../774/conversation-timestamps.md) 中将 Subagents 视为断点的行为；其它规则不变。

## 最小实现

在 `refresh_timestamps` 第二次遍历中，在 owner/断点处理前跳过 Subagents，并从断点匹配移除该变体。第一次遍历继续排除 Subagents，避免把聚合条目当成时间来源或可见前缀目标。

## 验收与验证

开发回归调用真实 `ConversationStream::refresh_timestamps`，覆盖：

- assistant → 子 Agent 聚合 → user，超过 1 小时生成用户前缀，等于或小于不生成。
- assistant → 子 Agent 聚合 → assistant，超过 10 分钟生成前缀，等于不生成。
- 实时插入与历史恢复、多个聚合条目、零行聚合和强制重算结果一致，不生成聚合前缀。
- 缺失前序时间不伪造比较；显式断点、摘要/压缩等真实断点继续抑制前缀。

本地仅格式、差异与静态检查，不运行 Vega、编译或测试。自动测试由现有 PR Check 执行。按仓库现行政策，开发 Rust 测试及其独占接线在云端成功验证并固定源码、完整 SHA 与结果后移除，最终无 Rust 测试 head 再经完整 required CI；不以空集门禁代替开发回归结果。原生 UI 由用户更新后手测。
