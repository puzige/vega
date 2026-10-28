# 协议失败的脱敏分支诊断

## 范围与基线

用户确认缩小 #671：只增加协议失败原因、持久化与合成响应回归，不改压缩机制。基线 `c42148f69c9cb987901aeae56932e7ecf57a24ce`。

当前 Chat SSE 的多个拒绝分支共用 `invalid_protocol()`；Responses 共用 `invalid()`。`ProviderDiagnostic`、运行指标及 `run_diagnostic_events` 没有解析原因字段；recovery 终态会重新构造错误。历史仅有 protocol/response，无法补推原始失败原因。

## 契约

- 给 ProviderDiagnostic 增加可空、类型化的 `protocol_reason`；仅 Protocol 可带原因。运行指标与存储通过闭合词表传递，禁止从响应正文、错误 message、字段值或工具参数生成原因。
- Chat 原因区分 JSON、帧形状、delta 类型、终态后内容、reasoning 别名冲突、reasoning details、工具片段、工具身份冲突、未完成工具、终态矛盾、缺失终态。SSE 解码及帧容量单独标记。
- Responses 至少区分 JSON、事件结构、终态后事件、文本快照冲突、输出完整性、usage 不一致；提前断流保持原有 StreamInterrupted，不添加协议原因；请求构造的旧通用协议错误可保持无原因，不伪称响应解析失败。
- 只给现有失败标注原因，不改变任何校验条件、容错、usage 处理、错误分类、重试/可见输出围栏、工具执行或压缩控制流。
- 原因经重试终态、主模型及摘要失败诊断映射保存。新增递增迁移，可空列保留历史 NULL；读写校验及 SQL CHECK 拒绝未知词及非 provider/protocol 关联。不修改用户数据库/配置，不重放原会话。

## 验收与验证

开发分支使用合成输入，无外部服务：

1. Chat 非法 JSON、非对象、非法 delta、reasoning 别名冲突、终态后 delta、终态矛盾、缺失终态、工具片段/身份/完整性拒绝均保留期望原因。
2. Responses 非法 JSON、非法事件、终态后事件、快照/输出不一致及 usage 不一致按现有拒绝行为返回原因；提前断流保持 StreamInterrupted。
3. 正常文本和工具响应仍成功；`error:null`、未知 finish reason、不可用 usage 的既有兼容行为不退化；过载、拒绝、超时仍不冒充 protocol。
4. recovery 终态保留原因与 HTTP/retry 元数据，不更改重试条件；运行/摘要映射携带原因。
5. 临时 SQLite 完整迁移、写入/读取 round trip；旧行 NULL；未知值和错误关联在 API 及直接 SQL 均被拒绝。测试只使用合成数据。
6. 敏感哨兵不得出现在原因、Debug/Display 或持久化诊断中。

本地仅差异、格式及静态检查，不编译、不运行 Vega 或测试。自动测试交现有 Actions；通过后固定可恢复测试源码版本和云端结果，再按仓库政策清理开发 Rust 测试及独占支持。非实现者独立审查和最终无 Rust 测试 head required CI 后合并，Issue 保持开放等待用户手测。CI 不证明旧故障根因或真实服务已修复。
