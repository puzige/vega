# #774 手测反馈修复：整轮结束后显示助手时间

## 契约与实现

用户确认：已发送用户消息可显示 hover / 键盘焦点时间；当前整轮运行中的所有助手消息都不显示单条时间，工具调用、正文分段完成、重试和续接不算整轮结束。整轮释放后按已有时间来源显示，不改为结束时刻；旧消息、缺失时间策略与分组分隔线不变。

基线：`b831719345b8f10f34f6fe05e8b5b931aee6c156`。修复 [PR #787](https://github.com/puzige/vega/pull/787)。`MessageStarted` 在运行期间记录 owner，`finish_composer_run` 统一释放；时间投影过滤本轮 owner，保留隐藏投影缓存以避免逐 token 全量重算。结束时投影失效并重测相关列表项，焦点接线随可见时间更新。

## 开发期验证与来源

- 固定版本：`d87c6a1e186d12b3c39b3ad93c43368286c93f24`。
- [原始测试源码](https://github.com/puzige/vega/blob/d87c6a1e186d12b3c39b3ad93c43368286c93f24/crates/vega_ui/src/conversation_stream/timestamps/tests.rs)，SHA-256 `4524be1bcec1beefdcf72d63e95e578e6e5207252bf4d2013edfc999f9035c65`。
- [Actions 41153968014](https://github.com/puzige/vega/actions/runs/41153968014)：Format、Clippy、Nextest、required check 均成功。Rust **6 发现、6 执行、6 通过、0 跳过**；Python **5 + 15 + 40 = 60 通过**。
- 六项测试验证生产状态 helper 的当前助手隐藏、用户／旧助手不受影响、多助手续接直到释放、下一轮不隐藏上一轮、缺时间保持缺失及缓存／重复状态失效。并非原生 UI 或运行生命周期集成测试。
- 独立非实现者审查上述固定版本及受影响生命周期、历史替换、活跃子会话、列表和焦点契约，**无发现**；只做静态审查，不代表真实 UI 验收。
- 清理动作只移除本轮测试文件与 `cfg(test) mod tests`，不改变生产非空白行。基线已由 #785 清理此前授权诊断测试，本轮不恢复。

## 最终门禁与手测

测试清理后将运行完整最终 head required CI，结果记录在 PR / Issue；不得以带测试版本结果替代。最终仓库预期 Rust 0 发现／0 执行，这不等于行为测试通过。

本地只做定向 rustfmt、差异和静态清点，未运行 Vega、编译、check、Clippy 或测试。

用户手测：运行输出、工具等待和续接中 hover／键盘均不显示当前助手时间；用户消息和历史助手时间仍可显示；正常完成、失败、停止及断连释放后恢复已有时间，不能伪造缺失值。原生交互仍待用户在合并后手测，Issue 保持开放。
