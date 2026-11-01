# 子 Agent 聚合条目保留时间比较

## 用户授权与范围

修复父对话流在子 Agent 聚合条目后丢失时间分隔的问题，并补对应回归测试。仅修改时间分组投影，不改子 Agent 生命周期、数据库、消息时间来源、正文复制或可见文案。

## 契约

后续用户反馈修订：下文摘要断点及旧开发测试中的零行 Summary 预期，已由 [隐藏统计摘要修订](hidden-summary-timestamps.md) 替代；旧验证记录仅证明当时的子 Agent 契约。

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

## 开发验证记录

- 开发验证完整 SHA：`20703e06482e3a4edf40bbf28f9755acc4dfd993`；[测试原始源码](https://github.com/puzige/vega/blob/20703e06482e3a4edf40bbf28f9755acc4dfd993/crates/vega_ui/src/conversation_stream/timestamp_tests.rs)，远端读取确认 blob `c4785f96a15aa51b365bd1d0339d1915d8db7a82`，SHA-256 `d19ce86b54a7f51aa9e43af4eb1fdb9ed02e3aab4f6a04ef9da0fb3228dcfa36`。
- [Actions 41256592520](https://github.com/puzige/vega/actions/runs/41256592520)：完整 required check 成功；Clippy job `124700950812`，Nextest job `124700951473`，汇总 `124701558493`。`cargo nextest list --workspace --message-format json` 发现 5 项；`cargo nextest run --workspace` 实际执行 5 项，5 通过、0 跳过。三组 Python 脚本回归在同一 Nextest job 成功。
- 独立审查确认生产修复符合契约；首用户夹具问题已修正并增量复核无发现。此前 run `41255892194` 的回归亦 5/5 通过，但 Clippy 因测试文件多余导入失败；删除未使用导入后取得上述完整成功，不放宽断言。
- 本地 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --no-deps --offline --format-version 1` 成功，未运行 Vega 或测试。
- 按政策清理测试文件、模块声明和 `gpui_kit/test-support` 独占 dev dependency，生产修复保持不变。最终清理 head 的 required CI 结果以 [PR #808](https://github.com/puzige/vega/pull/808) 精确 head 为准，待取得；不把开发回归结果替代最终门禁。
- 直接覆盖限于上述 5 项测试：非零行聚合、分多页恢复、压缩及技能来源断点未直接运行开发回归；这些生产路径未更改。原生视觉效果及真实对话验收待用户更新后手测。
