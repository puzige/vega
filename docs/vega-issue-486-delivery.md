# Issue #486 · 实现与验证记录

关联 [Issue #486](https://github.com/puzige/vega/issues/486)、[PR #491](https://github.com/puzige/vega/pull/491)。需求为 [v1](vega-issue-486-markdown-link-preview.md)，技术方案为 [v2](vega-issue-486-markdown-link-preview-implementation.md)。

当前阶段：2026-10-19 已确认最小目标接口缺失 RED，专职实现者已交回完整实现，Codex 完成静态集成审查；待同一冻结测试云端验证。尚无实现 GREEN、删测或合并结论。

## 顺序与可信来源

| 阶段 | 固定证据 | 结果 |
|---|---|---|
| 生产未改方案基线 | `35d443dbade4f0766cadc81e915613719b358279` | 只含需求/技术方案与索引文档 |
| 先行测试 | `5d46a1955d0212182f128e46dec20c67898dce55` | 22 个服务、20 个 UI/模型定义，其中 18 个 GPUI；42 是静态定义数 |
| 有效 RED | [Actions 39201250059](https://github.com/puzige/vega/actions/runs/39201250059) | 最小目标 API 缺失；全部 Rust 行为断言未执行 |
| 生产实现 | [实现阶段交接](vega-issue-486-implementation-stage.md) | 完整代码已写出并经静态审查；未本地编译或运行 |
| 同一测试 GREEN | 待实现与云端验证 | 未获得 |
| 独立删测 | 待 GREEN 及证据固定 | 未执行 |
| 最终 head 完整门禁 | 待删测 | 未执行；须明确 Rust 0 发现/0 执行，不能代替上一阶段 GREEN |
| 合并及用户 M01/M02 | 待最新 head 门禁 | 未合并、未手测 |

冻结测试可从 PR 和不可变提交直接找回，不依赖本机文件、分支名称或 Actions artifact：

- [服务测试](https://github.com/puzige/vega/blob/5d46a1955d0212182f128e46dec20c67898dce55/crates/vega_conversation/src/link_preview_tests.rs)，SHA-256 `cd688119fe429bc05cb592cfbf09164dc472b1b931c76d7dbfbf073efac1909b`。
- [外部依赖 fixture](https://github.com/puzige/vega/blob/5d46a1955d0212182f128e46dec20c67898dce55/crates/vega_conversation/src/link_preview_test_support.rs)，SHA-256 `b43ae6426450ca724611ef39021b7c70b24ee89fc2b3e56cd7089adaf8eee3be`。
- [UI/模型测试](https://github.com/puzige/vega/blob/5d46a1955d0212182f128e46dec20c67898dce55/crates/vega_ui/src/conversation_stream/link_preview_tests.rs)，SHA-256 `40c1d0cd5b87a38ecc43f85448c9453e578a42fd4aa890e1bbb87d00bebd167f`。
- [Thinking fixture](https://github.com/puzige/vega/blob/5d46a1955d0212182f128e46dec20c67898dce55/crates/vega_ui/src/conversation_stream/link_preview_thinking_fixture.rs)，SHA-256 `6683f075a1fcfd3185958d60393abc19a9d5e60573a01725513344ba07080ca6`。

## RED 原因与执行边界

Run 的 head 为 `5d46a1955d0212182f128e46dec20c67898dce55`。[Nextest job](https://github.com/puzige/vega/actions/runs/39201250059/job/117894487878) 在 `cargo nextest list --workspace --message-format json` 的测试目标构建阶段以 101 退出；[Clippy job](https://github.com/puzige/vega/actions/runs/39201250059/job/117894487967) 同样停止于缺失目标 API。

```text
error[E0433]: cannot find `link_preview` in `crate`
  --> crates/vega_conversation/src/link_preview_test_support.rs:12:5
error[E0432]: unresolved imports `crate::link_preview`, `crate::types::LinkPreviewFailure`
error[E0432]: unresolved imports `crate::types::LinkPreviewFailure`, `crate::types::LinkSiteKind`
```

已检查原始完整日志，错误均属于冻结方案要求新增的入口，未见无关依赖、格式或 runner 故障。fmt 成功；Python 三组 5、15、14 个执行通过，共 34 个。未获得有效 Rust 发现计数；42 个行为断言全部尚未执行，不把接口缺失称为行为断言失败。服务构建先停止，UI 目标也未运行。

[PR 原始 RED 记录](https://github.com/puzige/vega/pull/491#issuecomment-6259957942) 固定原因、提交和工作流归属。之后才授权同一专职实现者写生产代码；测试目标/期望与四个文件 hash 保持冻结。

## 本地验证边界

只允许快速差异、格式、依赖元数据和静态检查；本地未运行 Vega、编译、Clippy、Rust/Python 测试、benchmark 或打包。实际命令、退出码、实现审查结果和后续云端计数在对应阶段取得后补记，未取得项不标通过。

## 实现静态审查

Codex 主会话逐项审查服务及 UI 接线；实现由专职子 agent 独占写入。审查修正包括：实际连接地址固定与 peer 核对、禁代理/自动跳转/自动解压；图片维度探测和解码均设置限制；同 URL 的各 flight 独立追踪；缓存 weight 包含地址；图片未完成时保留已取得的文字。

真实 resolver 的两个 permit 由底层原生 DNS closure 持有到实际结束，取消或超时后也不提前释放，避免仅限制 worker 而让工作队列增长；计时 future 在首次 poll 前丢弃也取消工作。窗口释放同时取消 Armed、Leaving 和 Active 工作。

UI 使用 scope、owner、occurrence 和精确 target 共同区分目标，真实 role/label/description 同时供原生代理和只读观察使用；预览身份不替代完整文字的延迟点击屏障。虚线用实际字体基线和像素对齐几何绘制，卡片固定站点/Open，内容局部滚动。会话、思考、文件预览和更新说明接到同一个窗口层。

主会话独立执行 `cargo fmt --all -- --check`、`git diff --check`、`cargo metadata --offline --locked --no-deps --format-version 1`，均 exit 0。四份冻结文件 SHA-256 与 RED 提交一致；实现没有新增/删除/升级 lock package，924 个身份及 checksum 相同，只增加已批准生产依赖边。未修改 CI、Python、迁移或发布流程。

静态审查不证明类型、借用、事件或行为执行正确；这些结论等待当前 PR 的完整云端门禁。规格偏离：无。

## 用户手测入口

合并且自动发布完成后，M01 检查浅/深主题、窄窗口、跨行/表格及窗口四边：虚线确有间隙，图标对齐，hover 不改变正文布局，图片比例正确，卡片可进入且不越界；同时用 Tab、Enter、Escape 操作并核对拖选/复制。

M02 使用公开 GitHub Issue 和普通网页，确认真实标题/摘要/图片与缺失回退，再检查断网、登录页和 Retry。正文链接及卡内 Open 应始终打开原始完整目标。该部分只能由用户实际执行，不以受控网络或 GPUI 场景代替。

Issue 在合并后改为 In review，保留分支/worktree；用户明确通过 M01/M02 后才清理资源、关闭 Issue 并置 Done。
