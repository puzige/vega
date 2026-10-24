# #545 验证与交付记录

## 开发版本

- 生产基线：`b7cb6be20aa6e6c72bcfad45d9649804c6d9ce45`。
- 规格：`78eb5aebf2401d0974f13b6ecd3c3244603b0733`，见 [v2](vega-issue-545-scroll-cache-v2.md)。
- 开发实现：`e1cf86a29df7abfdf75a66b8581bdeef5a44f0cd`。
- PR：[600](https://github.com/puzige/vega/pull/600)，不自动关闭 Issue。
- [原始开发测试源码](https://github.com/puzige/vega/blob/e1cf86a29df7abfdf75a66b8581bdeef5a44f0cd/crates/vega_ui/src/conversation_stream/text_cache.rs)；已从 GitHub API 读回，blob `a56ed12efd32601a8790cd290fe1b8a3feb082ae` 与本地提交一致。
- [完整开发补丁](https://github.com/puzige/vega/commit/e1cf86a29df7abfdf75a66b8581bdeef5a44f0cd.patch)。

## 实际验证

- 本地：实现者 direct rustfmt check（8 个修改文件）、git diff --check、静态约束/调用点检查；主会话及独立审查 diff check。未本地运行 Vega、编译、check、Clippy、测试、profile、CUA 或安装/重启。
- 非实现者独立审查 exact `e1cf86a29df7abfdf75a66b8581bdeef5a44f0cd`：READY，无 actionable finding。检查 source revision/incarnation、bounded admission、projection 复用、phase composition、borrow 生命周期和 #344/#535 不变量；不是运行验收。
- [Actions 40119627454](https://github.com/puzige/vega/actions/runs/40119627454)，exact 开发 head：completed/success。
- Nextest job `120948118728`：实际 **11 发现 / 11 执行 / 11 通过 / 0 失败 / 0 跳过**，1.175s；Python 三组 **5 + 15 + 40** 全部通过。
- Clippy job `120948118968`（含 fmt）success；required aggregate `120949036951` success。
- 读取的 Nextest 原始 job 日志 SHA-256：`b80351aa119be01de6c22de7a7bba28822a4e5e39630cedbde9c5e527c0b5651`。权威可访问日志仍为上述 Actions/job。

11 个测试覆盖真实 cache/model helper 的冷暖复用、脱敏、精确/未知宽度、同长内容版本、append/finish/reset/pending 移除、同 key incarnation 替换、历史不重算、条数/字节/超限与全 sweep admission、移除重排、即时 failure/ownership 组合、table/code 估高。

## 边界与后续

这些是 helper/model 行为证据，不是完整 drawn ConversationStream renderer、实际 rail/选字/焦点/阅读锚点或用户滚动性能证据。没有生产 profile/耗时/真实帧间隔数字，不声称整体 O(1) 或已解决所有掉帧原因。

后续清理只移除开发测试及两处计数器支持，保留上述源码/补丁/日志。最终 head 必须独立增量审查、静态无 Rust 测试/独占支持、完整 required CI 确认 no Rust tests（0 发现/0 执行）；最终空集不称测试通过。合并后 In review，Issue 保持开放等用户手测 A1–A7，尤其长历史、长回复、表格代码、生成时上翻及选字/复制/展开/导航/底部跟随。

当前尚未记录最终 head/门禁/合并或用户验收结果。
