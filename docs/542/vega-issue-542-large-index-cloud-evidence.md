# #542 大索引捕获修复：验证留痕

## 固定版本与可恢复源码

- 修复基线：`0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`。
- 开发实现与五项临时测试：`3fbc14467b8dc76a941179a50ed33a82fe23ade3`，树 `308ca56ad3b0bf0ff96a66372ce55e2367e66f7a`。
- [完整临时测试源码](https://github.com/puzige/vega/blob/3fbc14467b8dc76a941179a50ed33a82fe23ade3/crates/vega_conversation/src/git_workspace/snapshot.rs)，从末尾 `#[cfg(test)] mod tests` 开始；测试 helper 与五项测试全部位于同模块。
- 已通过 GitHub contents API 读回该固定 ref：blob `646843222c8f1f39f777ad1f091c06ab3b9d8288`，完整文件 SHA-256 `2d29879138dc58f207293092e7b40da0dc82a958e61a15198c909ed0092d7fbd`。不要从历史测试框架恢复其他测试。
- [原始提交差异](https://github.com/puzige/vega/commit/3fbc14467b8dc76a941179a50ed33a82fe23ade3) 和 [PR #595](https://github.com/puzige/vega/pull/595) 保留实现／源码访问入口。

## 开发期实际云端行为回归

[Actions 40115604332](https://github.com/puzige/vega/actions/runs/40115604332) 成功，head `3fbc14467b8dc76a941179a50ed33a82fe23ade3`。实际 Nextest checkout 为 synthetic merge `75dd5317d29225b3f0ae71c393451223c49f11b2`，父提交为基线与 head；API 树 `308ca56ad3b0bf0ff96a66372ce55e2367e66f7a` 等于 head 树。

- [Nextest job 120936126174](https://github.com/puzige/vega/actions/runs/40115604332/job/120936126174) 实际在 `2026-10-24T17:23:05Z` 输出 `Starting 5 tests across 15 binaries`，随后五项 PASS，`Summary [0.406s] 5 tests run: 5 passed, 0 skipped`。
- 测试覆盖：14,502 与 100,000 跟踪路径通过；100,001 拒绝；危险相对路径、重复、不完整 NUL 拒绝；10,000 变更路径通过且 10,001 拒绝；filter 属性拒绝及属性集合校验保留。
- 同一 job 中现存 Python 三组分别 5 / 15 / 40 tests，均 OK。
- [fmt / 全目标 Clippy job 120936126417](https://github.com/puzige/vega/actions/runs/40115604332/job/120936126417) 成功。
- [required check job 120936922092](https://github.com/puzige/vega/actions/runs/40115604332/job/120936922092) 成功。
- 通过 gh 取得的 Nextest 完整日志为 69,695 bytes，SHA-256 `f891e7c1364690b42cc7f896da7b0d1793ea0b555c6493ea051427f127ff2fdf`。

这五项是真实 parser 行为测试，不是完整 Git 子进程或 UI 集成测试。worker 错误、取消、成功与原消息归属未新增运行测试，依靠静态调用链审查；不能宣称已经自动验证七文件 UI 聚合或 steering。

## 最终交付边界

临时 Rust 测试将在单独清理提交中删除，生产修复必须与已验证版本一致。最终无测试 head 的 required CI 与独立审查仍待完成；空集只写 no Rust tests（0 发现/0 执行），不写行为测试通过。未本地运行 Vega／构建／测试，未安装或启动应用。合并与用户手测分别确认。
