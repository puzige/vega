# #486 崩溃修复：最小生产实现阶段

2026-10-19。依据[修复方案 v1](vega-issue-486-list-state-borrow-crash.md)，主审核实并公开固定有效行为 RED 后，明确授权本阶段最小采样修复。

## 已取得的真实 RED

- 测试先行提交：`4b60f0b28eb1c7734d51dc1003d9141d45515872`，新 [Draft PR #493](https://github.com/puzige/vega/pull/493)。
- [永久 RED 证据](https://github.com/puzige/vega/pull/493#issuecomment-6262985052)及 [Actions 39222176469](https://github.com/puzige/vega/actions/runs/39222176469)：[Nextest job 117959818844](https://github.com/puzige/vega/actions/runs/39222176469/job/117959818844) 实际 **5 发现、5 执行、1 通过、4 失败、0 跳过**，14 binaries，0.561s。
- B01–B04 实际因 pinned `gpui-pre/src/elements/list.rs:561:16` 的 `RefCell already mutably borrowed` 失败；B05 真实 FilePreview 全部断言实际通过。fmt、全目标 Clippy 与 Python 34 项均通过，无编译、依赖或 runner 故障。不是接口缺失 RED。

## 基线与修改

实施前 fetch 并 ff-only 同步，HEAD/远端任务分支均为上述测试提交；master 为 `b32ad22e530d27925e375df00b6bd401bf9646a3`，master…HEAD 为 `0/15`，起始工作树干净，未出现基线漂移。主审随后更新的既有文档保持原样。

唯一生产修改为 `crates/vega_ui/src/link_preview.rs`：

1. `prepaint_link_targets` 保留全部链接登记与 scope 句柄克隆，`scroll` 暂存为空；不在 List 子项 prepaint 内读取其滚动状态。
2. 最后 sibling `MarkdownLinkPreviewLayer::prepaint` 的既有 `entity.update` 内，先遍历全部 staged 链接，从各自 scope 读取本帧拥有的 ScrollStamp；随后才执行原有 staged→visible 替换和旧/新值比较。

列表子项 prepaint 返回后，根层在同一帧采样布局/定位后的真实状态。未改 List/ScrollHandle/None 签名含义、liveness、关闭、取消、抑制重入或焦点逻辑；没有 try_borrow 回退、吞 panic、延后一帧、删登记或依赖/CI 改动。精确差异反向投影后整个文件等于测试先行提交，其余代码/测试接线和 Cargo 文件无差异。

## 冻结源码与静态结果

两份测试/fixture 内容未改，仍为[测试阶段](vega-issue-486-crash-test-stage.md)冻结的五个定义：

| 文件 | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/link_preview_tests.rs` | `f369318c1875c0cf358e6b561a2daacc39828f2befb9a421564fab5a63fd19b1` |
| `crates/vega_conversation/src/link_preview/test_support.rs` | `a83eba45519905a4a2e948500734d94d9d6142b93c8810eb0d8c31a6a3a0e331` |

`cargo fmt --all`、最终 `cargo fmt --all -- --check`、`git diff --check` 和精确源码差异/hash 静态核对均退出 0。本地未运行 Vega、编译、构建、Clippy、测试、benchmark 或打包；执行方未 commit/push 或操作 Issue/PR/CI。

## 交回与待验证

**停止写入，交主审审查及提交同一冻结测试的云端 GREEN；当前 GREEN 尚未取得。** 根层采样后的首帧、虚拟化、滚动关闭/取消、静止指针、流式物化与文件阅读行为须由这五项实际执行确认，静态核对不能代替行为通过。

不得在 GREEN 前删测或追加/修改断言。GREEN 经主审核实且在 PR 固定源码/原始输出后，才可另行授权独立删除测试及专用接线，清单沿用测试阶段报告；删除后须再验证最新 head 的完整空集门禁，保留生产采样修复与全部真实 UI/服务能力。
