# #768 开发验证与交付记录

## 行为与边界

`read_conversation` 复用唯一参数解析契约，并接入提议、只读审批、终态落库、取消及重开校验。运行期错误类型保持现有分类，#770 的历史分类回灌不在此卡范围。

启用历史回读时同时检查跨页凭据变化、嵌套来源和摘要依赖：只读核对当前线程原记录；无法确认安全的内容在本次模型出站中暂时失效，原始记录保留并可重读。摘要记录实际安全输入的调用依赖，引用按 checkpoint ID 绑定，避免旧失效页影响新安全摘要。所有来源读取及重复分页投影有预算上限。

## 开发证据

| 提交 / PR | 云端结果 | 后续处理 |
|---|---|---|
| d606e5d6c7ff57bcfc76a2b52d86dcbcbf3dc3ea / #772 | [40994718228](https://github.com/puzige/vega/actions/runs/40994718228)：8 执行/通过，0 跳过；发现数未取得 | 后续独立审查发现分页与摘要安全缺口，不能仅凭此轮通过交付 |
| 0cecab7f37b2e8f85c8930b4233e96464c4b7554 / #773 | [40997726405](https://github.com/puzige/vega/actions/runs/40997726405)：13 执行，12 通过，1 失败；Clippy 通过 | 测试缺少后续用户轮次、未形成可压缩前缀；修正 fixture，保留断言 |
| 2b7e3a46f5e10a2242c7426d4c51f667777c512a / #775 | [40999420924](https://github.com/puzige/vega/actions/runs/40999420924)：18 执行，17 通过，1 失败；Clippy 失败 | 脱敏 ID 用例提前命中工具输入；改为读取后切换凭据。新增内部字段导致事件过大，改为装箱 |
| 6eae6239087585b40e9000235c498f5c903cdae4 / #776 | [41000184613](https://github.com/puzige/vega/actions/runs/41000184613)：编译 E0506，测试未执行 | 将依赖赋值移至投影计划借用结束后 |
| f651f7dec69a7d30c891e526f93d76d43e587ab7 / #777 | [41000702048](https://github.com/puzige/vega/actions/runs/41000702048)：19 发现/执行/通过，0 跳过；Clippy 仅开发测试锁作用域失败 | 捕获记录改为独立快照，并补强逐调用 ID 出站断言；生产代码未变化 |

最终开发源码：[91527865e39b30d6409f01bafad42e98206bfa77](https://github.com/puzige/vega/blob/91527865e39b30d6409f01bafad42e98206bfa77/crates/vega_conversation/src/agent/history_audit_tests.rs)，[完整补丁](https://github.com/puzige/vega/pull/778.patch)。远端源码 blob `f0db3f80bb5ad5ed85faf88e5fd6c8a69645cd84` 已与本地提交核对。分支及 PR 留存，不依赖临时 artifact。

最终开发 [Actions 41001434639](https://github.com/puzige/vega/actions/runs/41001434639) 对应 `91527865e39b30d6409f01bafad42e98206bfa77`：19 发现、19 执行、19 通过、0 跳过，fmt、Clippy、Python 检查及 required check 成功。使用现有 `cargo nextest list --workspace --message-format json` 和 `cargo nextest run --workspace`，未修改 CI。

## 独立审查与最终清理

非写入者对 f651f7dec69a7d30c891e526f93d76d43e587ab7 的增量审查未发现新的确定代码缺陷；指出的出站空集断言已在 91527865 中补强，生产内容相同。静态审查不冒充执行验证。

最终移除开发测试文件与 agent 模块中的 cfg(test) 接线，保留生产实现、安全断言、Python 检查和 CI。最终 head 的 required check 和 Rust 0 发现/0 执行另行记录；空集不代表功能回归通过。本地仅格式及差异静态检查，未构建或运行 Vega。

## 独立压缩问题

诊断中的 `summary_source_changed` 覆盖多个失败位置，不能把所有根阶段运行耗时视为丢失的摘要耗时。此前读取的 2026-10-30 记录共 8 个根运行失败，其中 6 个运行包含成功摘要阶段，阶段耗时合计 944950 ms（约 15 分 45 秒）；另 2 个没有同运行摘要事件，整轮耗时不能直接归为摘要浪费。此统计是当时快照，不是当日最终总量。

尚未证明这些记录具有同一根因；需要结合预计算、消费和源校验阶段继续定位。本卡未创建新 Issue，也不宣称修复这一独立问题。#768 的拒绝诊断未保存失败工具名，不能由调用 ID 独立证明特定 49/85 分钟事故均由 read_conversation 导致。

## 用户手测与回滚

合并后用户检查正常历史回读、持续运行及重开会话。CI、合并不代表应用已更新或用户验收；Issue 保持开放、In review。若回归，在单独 PR 回滚本卡 squash 提交，不修改用户运行数据库。
