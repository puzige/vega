# Issue #455 实现交付

实施日期：2026-10-18（Asia/Shanghai）。分支：`feat/455-http-autolinks`。任务契约：[HTTP/HTTPS 自动链接规格 v2](vega-issue-455-http-autolinks.md)。本记录描述实现和本地静态检查，功能结果待 GitHub Actions 与用户手测确认。

## 变更

| 文件 | 对应行为 |
|---|---|
| `conversation_stream/autolinks.rs` | 共用 HTTP/HTTPS 扫描与 span 物化；保留 UTF-8、目标字节、端口、IPv6、query、fragment 和配对括号，剔除尾部句读与未配对闭括号 |
| `conversation_stream/model.rs` | 普通 Markdown 文本在块物化时扫描；显式链接继续覆盖标签中的自动目标，代码 span 与代码块不接入扫描 |
| `conversation_stream/render_rows.rs` | 用户消息保持纯文本、空行和原文；普通与可选路径传递链接范围；普通路径继续观测实际 `USER_TEXT_LAYOUTS` |
| `conversation_stream/inline_code.rs` | 让用户普通路径复用现有 `StyledText` 和链接交互元素，不替换字形布局 |
| `conversation_stream/mod.rs`、`tests/mod.rs` | 注册私有扫描模块与本卡回归 |
| `tests/issue357_markdown_links.rs` | 仅调整复用所需的测试 helper 可见性，保留原有断言 |
| `tests/issue455_http_autolinks.rs` | 用户/Markdown 容器、主题样式、显式目标优先、代码排除、选择复制、取消动作、折行几何、指针顺序、历史与思考、流式冻结缓存回归 |

上述源码路径均位于 `crates/vega_ui/src/`。规格 v2 的 HTML 来源边界修订由 Codex 主会话在实施前确认：解析器已降级的可见 HTML 文字沿用普通扫描规则，不改解析器或来源映射。无新增依赖、产品提示文案或公开 API；`links.rs`、`selection.rs` 的交互状态机未改动。

## 静态检查

| 实际命令 | 退出码 | 结果 |
|---|---|---|
| 首次 `cargo fmt --all -- --check` | 1 | 新增代码存在排版差异；无编译或测试执行 |
| `cargo fmt --all` | 0 | 已应用必要排版；随后补充回归后重复格式化均为 0 |
| 最终 `cargo fmt --all -- --check` | 0 | 无输出 |
| 最终 `git diff --check` | 0 | 无输出 |

未在本地运行 Vega，未执行构建、打包、Clippy、`cargo check`、test 或 nextest。

## 待云端验证

| 验收 | 已编写的回归 | 执行状态 |
|---|---|---|
| A01、A03、A04 | 扫描器 4 例及纯文本用户 span、原文字节、换行、句读、IPv6 与端口边界 | NOT RUN，待 CI |
| A02、A05、A06 | 两条 Markdown 渲染路径的标题、段落、列表、引用与表格；粗斜体和 1px accent 下划线；显式目标、非白名单目标、代码与 HTML 降级边界 | NOT RUN，待 CI |
| A07 | 用户/Markdown 的拖选、复制、多击、Shift、右键、跨目标、拖出返回、失焦及替换取消；普通用户同一身份文本替换 | NOT RUN，待 CI |
| A08 | 用户和 Markdown 的普通/可选路径窄窗口折行；由真实末字符 advance 与 caret 独立计算点击点；前后普通文字不打开；IBeam 后覆盖手形 | NOT RUN，待 CI |
| A09 | 分 delta URL、finish 模型一致性、冻结块零重新物化、相同快照不重物化、历史恢复与展开思考 | NOT RUN，待 CI |

Actions 命令：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
```

本卡回归及 #357、#432、#147、#78 的既有测试都由 workspace nextest 覆盖。本地未生成云端结果或原始日志；CI 后由 Codex 主会话补充对应提交的实际结果。

## 限制与剩余验收

- 规格偏离：无；遵循实施前 v2 修订。
- NOT RUN：编译、Clippy、全部自动测试与真实浏览器集成；当前仅有静态检查证据。
- M01：合并后由用户在浅/深主题和窄窗口核实下划线、手形、浏览器打开及选择复制。
- 自动识别继续受既有 `allowed_link` 边界约束，不新增 URL 规范化或网络校验。普通可见文字按冻结的分隔与尾句读规则识别；显式 Markdown 链接仍保留其完整目标。
- 本次不更新或启动本机应用。回滚为撤销本卡集成提交；手测前保留 Issue、分支及 worktree。
