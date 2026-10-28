# #705 验证记录

## 开发版本与回归留痕

开发最终版本：`908ab47f69d5de8bf6060596e2fafb8a3d436688`。

- [原始开发测试源码](https://github.com/puzige/vega/blob/908ab47f69d5de8bf6060596e2fafb8a3d436688/crates/vega_ui/src/conversation_stream/core/staged_reasoning_tests.rs)
- [开发补丁](https://github.com/puzige/vega/commit/8a1a454a24abb5ba66abcd573b5040b2c2ccce69.patch)
- [恢复修复及回归增强补丁](https://github.com/puzige/vega/commit/908ab47f69d5de8bf6060596e2fafb8a3d436688.patch)
- [Actions run](https://github.com/puzige/vega/actions/runs/40627726446)：Clippy、Nextest、required check 均成功。
- [Nextest 日志](https://github.com/puzige/vega/actions/runs/40627726446/job/122616576802)：2 discovered，2 run，2 passed，0 skipped。

行为回归覆盖：B 的真实档位、重复打开、思考变更事件指向 B、A 提交思考快照不变、恢复 pending selection、旧模型 profile/event 拒绝、无档位清理、权威偏好回滚后再次发出保存意图。

## 独立审查

非实现者 review-705 审查开发版本，首轮发现缓存会话在保存回执后重新显示时未更新配置。修复为仅在缓存 stream 重新成为可见会话时重投影权威配置。增量审查绑定 `908ab47f69d5de8bf6060596e2fafb8a3d436688`，确认该问题关闭，无新具体发现。

## 最终交付边界

按仓库规则移除开发测试文件、`cfg(test)` 接线和独占 `test-support` dev dependency，不改生产逻辑或弱化断言；以上完整 SHA 保留原始测试与实际结果。最终无 Rust 测试 head 必须重新通过完整 required CI；其结果记录在 PR #707 / Issue #705，不将开发版本结果冒充最终验证。

本地仅执行 `cargo fmt --all -- --check`、`git diff --check` 及静态阅读，均未本地编译、运行 Vega 或测试。

## 未由自动回归验证的项目

真实持久化 worker 与缓存会话切回的整条异步路径由静态审查支持，未作为进程级自动测试执行；原生鼠标/键盘、运行 A 的真实服务请求不受影响、下一轮实际调用 B、失败与恢复、完成/取消后的行为仍须用户手测。手测按 spec 回归表执行，合并与 CI 不等于用户验收。
