# 更正后行为 RED 与剩余最小修正审查

修改前基线 `f7128d6d25c7d4ea864b8974d975de9125053eed` 的 [Actions 39369557697](https://github.com/puzige/vega/actions/runs/39369557697) 完整运行133项：129通过、4失败、0跳过；Format、全目标Clippy和Python34成功。[原始失败摘录与日志SHA](vega-issue-494-corrected-behavior-red.json)已保存。原先更正的模型/API身份、工具图片、滑块和设置事件用例均实际通过；以下三个未改测试的失败确认真实实现缺口。

## 一个额外测试自身错误，先独立更正

容量测试第二分支在原 `unwrap()` 处得到 `Err(Context(OverLimit { estimated_tokens: 42923, input_budget: 400 }))`，未走到 `assert!(outcome.failed)`。生产未改基线 [8c57f7e的预算门禁](https://github.com/puzige/vega/blob/8c57f7efb3d886c1c10429edb2bc1219a7b7a753/crates/vega_runtime/src/agent/loop_.rs#L1765)已经在发送前明确返回同一typed Err；当前此分支逐字相同。它不是本轮实现新造成的失败形态，不能为了错误的测试假定把生产API改成Ok。

本测试的逻辑验收一直是“超预算不发送，目录元数据不越过已保存容量权威”。允许仅把该分支的 `await.unwrap()` 和 `outcome.failed` 更正为精确匹配 `Err(VegaError::Context(ContextRuntimeError::OverLimit { .. }))`，保留provider bodies为空、全部输入和前一分支精确800断言。不能把任意Err或任意失败都算成功。此更正确实改变一个断言表达的错误类型，单独留证和提交，不再宣称所有结果断言字节不变；它不改变验收含义。原冻结清单保持，133名称/属性不变。

## 三处真实缺口的生产修正范围

1. `credential_guard.rs`：把Responses的response_reasoning、response_replay_layout中所有JSON键/值、response_id及FunctionCall关联字符串纳入共用检查。projection遇受保护原始内容直接拒绝，不能redact/改写签名或opaque bytes；reject/pre-attempt重用同一检查并沿用既有例外策略。只修请求投影和最终发送边界，不扩展本轮输出事件处理。
2. `compaction.rs`、`pipeline.rs`：提取当前精确provider/model policy读取和checked预算转换两个共享helper，保留主链“仅缺行使用assumed_default；显式None/None为未知；部分/损坏/溢出为错误”的现有语义。provider-aware projection用此policy生成settings、输入上限及compactable；manual accounted从实际provider绑定快照取得精确身份并核对FrozenReasoning，使用该projection，按input+reserve checked换算且手动automatic=false。精确provider存在时不读取旧thread预算兜底，不写旧表、不复制目录容量、不改变主链优先级。
3. `tools_exec.rs`：InvalidRead独立返回真实Authorization::Terminal，执行前处理取消，保留原失败反馈并附validation audit。移出Approved组；不改执行计数代码、合法nullable输入、InvalidGrep或其他权限规则。

三个目标分别有本次完整原始行为RED；对应测试/输入/结果期望保持不动。上述单独测试自身更正审查并提交后，授权专职子agent只实施这四个生产文件；不增加用例、模型硬编码、依赖、外部调用、配置迁移或注释。主会话随后审查差异和原133测试摘要，交同一完整Actions验证GREEN；未全绿不得删测、正式PR或合并。
