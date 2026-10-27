# 工具完成后的运行时失败诊断

## 证据与边界

两次旧运行在成功恢复/等待子任务后、下一次模型请求前失败，仅留下 runtime/unknown。等待运行取得完整 completed/released 回执。旧记录无法唯一定位根因，不能追认为数据库锁、模型或压缩失败。不重试原会话、不恢复旧子任务、不修改用户配置或数据库。

## 契约

- 保留原错误传播、取消、准入校验、成功工具事实、完整性校验和子任务生命周期；不增加自动重放。
- 待处理消息分别标记 pending_read、pending_admission、pending_decode、pending_references、pending_claim、pending_worker；下一轮能力快照标记 request_preparation。
- 数据库失败只保留 SQLite 主码/扩展码，IO 只保留类别；准入、消息解码、引用准备、阻塞任务、能力快照、预算非法、估算算术溢出采用闭合原因。不得保存 SQL、路径、消息、错误正文或凭据。
- 结构化运行时错误写 runtime_failure，不再使用 unknown_safe_failure；后者保留兼容旧记录和真正未分类错误，不伪造旧记录原因。
- 实时与历史从同一 runtime_reason 得出用户可操作的分类；沿用已有存储/状态/IO/事件错误文案，新增请求准备、引用和计量错误文字。细分阶段只进入诊断。
- 既有具体阶段不被外层 event_pipeline 覆盖。预算错误与算术错误准确区分，InputTooLarge 保持既有超限映射。

## 实现

1. runtime error 类型扩展闭合阶段/原因，安全归类 Context Estimate 和能力快照错误。
2. QueueSteeringSource 保留 Store/Io 信息，并准确标记校验/解码/引用/认领/worker 错误；两处工具后 take 入口标记后备阶段。
3. 诊断码 runtime_failure 贯穿 runtime/conversation/store，迁移扩展 CHECK 词表并保持原字段、数据和索引。
4. 实时/重开分类及中英必要错误提示保持一致。

## 验证

开发回归覆盖具体失败分类、阶段保留、取消和安全格式化、SQLite 错误码、待处理消息无数据与错误路径、诊断落盘/读回/导出。仅 Actions 执行自动测试；本地仅格式、差异和静态检查。开发 Rust 测试须在分支留存原始版本和实际云端结果后清理，最终 head 无 Rust 测试且 required CI 成功、非实现者独立审查后才合并。真实系统集成由用户手测。旧失败根因仍不可还原，诊断增强不等于原故障已修复。


## 开发验证与留痕

- 开发版本：`361b120978ac317f7a1a1d0df85eed835058961f`，base `f75bd0f634063ff63327f6b8d50f64734769d398`。
- 永久测试源码：[evidence/673-runtime-diagnostics-tests-361b1209](https://github.com/puzige/vega/tree/evidence/673-runtime-diagnostics-tests-361b1209)；完整 [原始 patch](https://github.com/puzige/vega/commit/361b120978ac317f7a1a1d0df85eed835058961f.patch)。已读回远端 tag peeled SHA 与开发版本一致。
- Actions [40437116038](https://github.com/puzige/vega/actions/runs/40437116038)：Clippy、Nextest、required 汇总成功；发现 11、执行 11、通过 11、跳过 0。覆盖队列/SQLite 故障、准入、解码/引用、正常消费、分类/导出、脱敏与阶段、真实 checked arithmetic 溢出边界。
- 独立非写入者审查绑定上述 SHA 和 diff SHA256 `e1f7ebd02bdd78af904aa3f231043c2dfecf095dc5955e06215a970e2fbf4bd0`，无阻塞发现。
- 回归不等于旧失败复现或真实系统集成；未覆盖完整主运行故障注入、带数据的旧库升级、worker/capability 端到端故障。本轮不声称修复旧故障根因。
- 按仓库政策清理两个开发测试文件、三个模块入口/内联测试块；生产断言与实现不变。最终无 Rust 测试的 head required CI 仍待验证。清理不覆盖上述已通过回归的原始源码和结果。
- 本地只执行 `cargo fmt --all -- --check`、`git diff --check`、i18n 静态检查；未执行 Vega、编译、构建、打包或自动测试。
