# 压缩错误与取消传播后端交付

Issue #397；契约：vega-compaction-error-presentation.md「错误与取消传播修订」。

## 实现前测试设计

自动和手动压缩：返回真错误且 token 同时取消时仍保留 Failed/具体失败；返回 Cancelled 时保留 Cancelled 状态与既有 usage。覆盖普通自动压缩和 Skill prospective 自动压缩入口。

Pipeline：processor/runtime 两支错误组合中，真实错误优先于 Cancelled；纯 Cancelled 持久化和投递 Interrupted，保留 message id、内容、计时与诊断，并经过共同撤销审计收尾。没有 message id 的准备期取消不制造 Error 或虚假归属。

## 最终实现

- runtime loop_.rs：普通自动入口沿用既有取消 outcome，判据改为返回的 VegaError::Cancelled；Skill 前置压缩按 typed error 分类并向上返回 Cancelled，usage/status 先照常投递，不吞掉取消。
- conversation compaction.rs：手动返回的错误决定终态，移除 token 的后验覆盖；算法、预算、timeout 与重试不变。
- entry.rs：合并两支结果时优先真实错误；取消持久化 Interrupted；保存/关闭持久化 actor 成功后进入正常共同尾部，保留 Skill 撤销审计，返回 interrupted=true、failed=false。持久化或关闭错误不伪装成功。
- pipeline.rs：Cancelled 投递 Interrupted；准备期没有 message id 时不造错误事件。
- agent/events.rs（经主 agent 追加授权）：取消 acknowledgement 保留 Cancelled 类型，其他错误沿用原安全包装；避免取消被包装成 persistence error 后错误合流再次误判。
- App 上层 app_agent.rs 使用 result.is_ok_and(|run| !run.failed)。纯取消现在返回正常中断 ConversationRun，避免 Finished(success=false) 的二次报错；不需改 App/UI 公共接口。

## 回归覆盖

| 测试 | 断言 |
|---|---|
| issue397_automatic_compaction_uses_returned_error_despite_token_race | typed取消与token独立；真实安全失败/可恢复超时不被覆盖；usage投递一次；不调用主provider |
| issue397_skill_prospect_compaction_preserves_typed_failure_and_cancellation | Skill前置入口状态与返回错误一致 |
| issue397_manual_compaction_keeps_returned_failure_when_token_is_cancelled | credential reader取消token且返回失败；持久化Failed；重开保留原因；不安装checkpoint |
| issue397_pipeline_merge_prefers_real_failure_over_cancellation | 两支顺序、单支、双取消、双真实错误优先级 |
| issue397_pipeline_forwarding_preserves_cancellation_identity_and_timing | Interrupted归属/计时、无归属取消、真错误仍Error |
| issue397_pipeline_cancellation_returns_interrupted_run_with_content_and_duration | 生产pipeline ack取消、partial内容与interrupted状态持久化、耗时一致、返回Ok且不failed |

既有 issue356 手动取消 acquisition/stream 用量与历史测试继续保留。原普通自动分类表中的 Cancelled 从“返回错误”断言移动至专门的中断 outcome 回归；非取消分类断言未放宽。

## 本地检查

- verified_at_utc: 2026-10-15T15:46:12.164143+00:00
- branch: feat/compaction-error-presentation
- backend_content_sha256: 3c86bfb850ec0e6f44ee4af9185ec49bafade49f7e7af14089042eacce47bfee
- `rustfmt --edition 2024 --check --config skip_children=true` 后接上述八个后端 Rust 文件：exit 0，stdout/stderr 为空。
- `git diff --check`：exit 0，stdout/stderr 为空。
- 未运行本地编译、cargo check、Clippy、测试或打包。未提交/push。

## 待验证与边界

CI 待运行：`cargo nextest run -p vega_runtime -p vega_conversation` 与仓库完整 pr-check；上述新回归目前均 NOT RUN，不能标记通过。无真实外部模型/进程自动测试。UI外观、用户真实停止和新消息替换由用户手测。

非取消错误继续走既有错误返回、恢复策略与安全脱敏；可恢复压缩失败策略仍不变，token确实取消时可以随后中断，但先前具体Failed状态不会被改写成Cancelled。没有新公共类型、依赖或数据库结构。未修改 types/events.rs 或 UI executor 文件。
