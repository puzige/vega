# #486 崩溃修复：独立删测阶段

2026-10-19。主审先核实并在 [PR #493 永久固定完整 GREEN、源码与历史证据](https://github.com/puzige/vega/pull/493#issuecomment-6263358878)，再明确授权独立清理。本阶段不增加生产行为或修改验收。

## 删测前的真实 GREEN

- 带测试 head：`02d25a72f546d0b076be9d1bd28adf4de1cbc2d0`。
- [Actions 39224636493](https://github.com/puzige/vega/actions/runs/39224636493) 的 [Nextest 117967836963](https://github.com/puzige/vega/actions/runs/39224636493/job/117967836963)：**5 发现、5 执行、5 通过、0 失败、0 跳过**，14 binaries，0.813s。B03 的四个滚动/异步路径属于同一个测试定义，不扩充执行数。
- fmt、全目标 [Clippy 117967836573](https://github.com/puzige/vega/actions/runs/39224636493/job/117967836573)、Python 34 项及 [required 117969548894](https://github.com/puzige/vega/actions/runs/39224636493/job/117969548894) 全部成功。
- 原有效 BorrowError RED、首次 4/5 未全绿、B04 前置布置错误及事先批准的一处修正保留于[测试阶段记录](vega-issue-486-crash-test-stage.md)和 PR；不把旧 42 项或原始 B04 未通过版本冒充本轮完整 GREEN。
- 清理前重新核实两份 GREEN 源码 SHA-256：UI 测试 `de18c58d1563b6eb797aa68286d2e2e4a75bdc5cbb66c916fd54048853624965`；conversation fixture `a83eba45519905a4a2e948500734d94d9d6142b93c8810eb0d8c31a6a3a0e331`。均可从上述完整 head 恢复。

实施前 fetch/ff-only 更新，HEAD/远端任务分支均为该 GREEN head，工作树干净；master 仍为 `b32ad22e530d27925e375df00b6bd401bf9646a3`，master…HEAD 为 `0/17`，没有新的基线漂移。

## 精确清理范围

删除本轮两份文件：

- `crates/vega_ui/src/conversation_stream/link_preview_tests.rs`。
- `crates/vega_conversation/src/link_preview/test_support.rs`。

删除 conversation `link_preview.rs` 的 feature-gated 子模块、UI `conversation_stream/mod.rs` 的 cfg(test) 子模块、conversation 临时 `test-support` feature，以及 UI 的 GPUI/conversation 两个 dev 边。

UI `link_preview.rs` 仅删除 cfg(test) 的 opener 别名/字段/分支、依赖安装/链接只读观察模块，以及卡片、标题、图片的三个 `.test_support()` 标记。`open_target` 继续直接调用 `cx.open_url`。真实 ID、role、label、description、focus、scroll、selection 和其生产消费者全部保留。

锁文件 **924 → 914**：精确移除 `bit-set 0.8.0`、`bit-vec 0.8.0`、`convert_case 0.11.0`、`proptest 1.11.0`、`proptest-macro 0.5.0`、`quick-error 1.2.3`、`rand_xorshift 0.4.0`、`rusty-fork 0.3.1`、`unarray 0.1.4`、`wait-timeout 0.2.1`。现有父包仅移除 GPUI → backtrace/proptest 两条测试边，消除旧版本后规范化同名依赖标签；backtrace 自身及生产 image 等继续保留。全部 914 个保留包的身份、来源、checksum 未变，无新增或升级。

没有恢复整目录、旧测试集合或旧代码；没有修改真实 ports/私有工厂、安全策略、benchmark、Python/fixture、CI/Nextest、资源、迁移或发布能力。主审自己的既有交付/索引文档保持原样。

## 生产投影与保护哈希

精确核对删除清单后，最终 UI 文件等于 b32ad22e 原生产文件加本轮已 GREEN 的采样修复：子项保留 scope 克隆与空 scroll 暂存，最后根层在发布 visible/比较旧新签名前采样全部 staged。七行根层采样循环及占位替换完整保留，其他生产逻辑无差异。其余六个正常文件恢复 b32ad22e 原字节。

| 最终文件 | SHA-256 |
|---|---|
| `crates/vega_ui/src/link_preview.rs` | `6cca823ef7c09375c9af2d9267951ac62e7ecaecedab74f7ed2bd935315c50c6` |
| `crates/vega_conversation/src/link_preview.rs` | `d9b5d8bf6f90d80eab8e3e10975909c0b042e9901b2996cf97c31049adcfff87` |
| `crates/vega_ui/src/conversation_stream/mod.rs` | `5774e655493845fa154d8e81bdccbceeaa52807cf81406199ddf0ffad9e8d278` |
| `crates/vega_conversation/Cargo.toml` | `7dc1000baa003b394f3a1761f9d6e0020e2ab30d033b9f5e7ffbc2e92f6205e2` |
| `crates/vega_ui/Cargo.toml` | `45f54ccb5909337fc1e31329bf85aaa254a829225e3e952d44ffa3fc5950787d` |
| `Cargo.toml` | `7bb8296128c9e92fe284bb81b63c1bc66c43fd93c8f09b64a3b9f297594c4be0` |
| `Cargo.lock` | `d803cdb1cd420921fa47ce091155bb0db1ce0845ecfa4027b95b46ebe2db36f9` |

其他代码路径相对 GREEN 未改，涵盖真实 ConversationStream Render/历史物化、InlineCodeText、FilePreview、窗口根层、真实服务 ports/metadata、选择和几何；测试阶段已记录的受保护哈希继续成立。

## 本地静态结果与最终门禁

- `cargo fmt --all -- --check`、`git diff --check`、完整 `cargo metadata --offline --locked --format-version 1` 均 exit **0**；metadata 仅解析依赖图，没有编译。
- 七个预期哈希、生产精确投影和允许变更路径核对通过；锁文件身份审计通过。
- 最终保留 **376** 个 Rust 文件。静态清点测试/bench/property 属性、测试 cfg、test-support 引用、dev/test/test-feature 清单项均为 **0**，可执行 doctest 为 **0**；原 `text` 文档围栏与 benchmark 代码字符串保留。该静态清点不是 Nextest 发现/执行结果。
- 未本地运行 Vega、编译、构建、Clippy、测试、benchmark 或打包；执行方未 commit/push 或操作 Issue/PR/CI。

**最终云端 head 门禁 PENDING。** 本次清理待主审审查、独立提交并触发现有完整 gate，要求最新 head 的 fmt、全目标 Clippy、Python、Rust **0 发现/0 执行、`no Rust tests`** 及 required check 成功。清理前五项 GREEN 不替代删测后的最终空集门禁。交回后停止写入，未经用户手测通过不关闭 Issue 或清理任务分支/worktree。
