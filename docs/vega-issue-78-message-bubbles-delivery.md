# Issue #78 用户消息气泡交付记录

## Freeze

- verified_at_utc: 2026-09-22T09:55Z；local: 2026-09-22 17:55 Asia/Shanghai。
- branch: `feat/issue-78-message-bubbles`；task_contract: [用户消息气泡](vega-issue-78-message-bubbles.md)。
- git_head / source_sha256: 保存在本地证据集 `issue-78-message-bubbles-20260922/implementation-manifest.json`；不公开机器路径及 raw OID。
- macOS arm64；rustc 1.98.0；cargo 1.98.0；git 2.55.0。

## 变更

- `crates/vega_theme/src/lib.rs`：类型化 80% 最大宽度与 16px 圆角。
- `crates/vega_ui/src/conversation_stream/render_rows.rs`：用户气泡随短文收缩、右对齐、`brand_soft`，不绘制「你」；正文容器明确继承消息字号和行高，内部空行与正文使用相同行高。模型保持兼容。
- `crates/vega_ui/src/conversation_stream/attachments.rs`：用户图片行满列且右对齐，缩略图、换行及操作代码保留。
- `crates/vega_ui/src/conversation_stream/tests/{mod.rs,issue78_message_bubbles.rs}`：实际 `render_entry`、绘制 quad、历史/流式渲染回归。私有 `cfg(test)` 观察器仅捕获真实 `StyledText` 布局，无替换测量或绘制。
- 任务规格与 `docs/vega-design-guidelines.md` 同步用户消息语义；没有依赖或持久化修改，spec 偏离无。

## Results

原始日志保存在本地证据集 `issue-78-message-bubbles-20260922`，对应文件名如下；包含所有失败，不自动重试。

| requirement | evidence class | exact command | result / duration | raw log |
|---|---|---|---|---|
| 初始回归 | production-rendering | `cargo test -p vega_ui issue78_ -- --nocapture` | FAIL：初版测试导入路径错误；修正后现有气泡在 320px 列占满 320px，80% 断言失败 | `vega-issue78-initial.log`, `vega-issue78-before.log` |
| B1–B5 呈现 | production-rendering | `cargo test -p vega_ui issue78_ -- --nocapture` | PASS 4 / 0.54s（编译 29.01s） | `vega-issue78-after3.log` |
| 图片操作 | E2E-REAL UI handler | `cargo test -p vega_ui conversation_stream::tests::attachments -- --nocapture` | PASS 6 / 0.67s | `vega-issue78-attachments.log` |
| 恢复/失败提示 | production-controller | `cargo test -p vega_ui conversation_stream::tests::hydration -- --nocapture` | PASS 6 / 0.01s | `vega-issue78-hydration.log` |
| 流式/工具顺序 | production-controller | `cargo test -p vega_ui conversation_stream::tests::timeline -- --nocapture` | PASS 6 / 0.01s | `vega-issue78-timeline.log` |
| 候选可执行文件 | build | `cargo build -p vega` | PASS exit 0 / 31.27s | `vega-issue78-build.log` |
| 格式 | static | `cargo fmt --all -- --check` | PASS exit 0；stdout/stderr 为空 | `vega-issue78-fmt.log` |

最终聚焦日志原始 bounded footer：

```text
running 4 tests
test conversation_stream::tests::issue78_message_bubbles::issue78_bubble_paints_theme_color_and_radius ... ok
test conversation_stream::tests::issue78_message_bubbles::issue78_history_and_streaming_keep_assistant_outside_user_bubble ... ok
test conversation_stream::tests::issue78_message_bubbles::issue78_user_bubble_hugs_text_and_wraps_within_column ... ok
test conversation_stream::tests::issue78_message_bubbles::issue78_image_entry_aligns_right ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 427 filtered out; finished in 0.54s
```

SHA-256 `vega-issue78-after3.log`: `839ce0e71079e16ff44bb816d0ace83c5f3528082f9964169c4405565d49085e`。其余日志逐项 SHA-256 在本地 manifest。

构建原始 bounded footer：

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 31.27s
```

构建日志 SHA-256: `b433a19ddb8c0a9ef246248b55f2679b63ef9c474625f2d17febef8a40d4fab6`。未安装或启动候选应用。

## 失败保留与修正

1. `initial`：测试模块与生产模块同名，导致 `ImagePreview` 导入路径解析错误；修正为显式生产路径。无产品代码绕行。
2. `before`：旧版气泡整列宽度的期望失败（320px > 256px）。图片低层 `draw` 缺少异步图像上下文，改为真实测试 window view；没有修改图片实现规避错误。
3. `after1`：正文 StyledText 的 TextRun 字号不能控制外层自然行高；旧短文高度为 42px（26+16），空行却为 24.75px。明确容器 15px、1.65 行高后原空行断言通过。
4. `after2`：新增绘制断言的 `ScaledPixels` 转换类型错误，按公开字段取值修正。未削弱布局断言。

## Residuals

- NOT RUN：真实原生窗口 B1–B6 截图、发送/模型回复、原生浅深色、960/1200/1403 与 1229/1230 主壳层边界；交给主控在独占原生应用后完成。以上测试不能替代实机截图验收。
- NOT RUN：本地全工作区 clippy/test；合并云端门禁由 PR 执行。
- LIMIT：320/600/768 是实际消息列测试宽度，不是整个应用窗口尺寸。测试逐字符确认文本完整与气泡内坐标；图片几何测试为单图，现有六条附件操作回归覆盖剪贴板/选择器/混合内容/确认及移除。
- 已知上游 `block v0.1.6` future-incompatibility 提示仍存在，与本卡无关。
- 无数据库迁移；回滚仅撤销本卡呈现、token 及对应规格/回归。
