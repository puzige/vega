# Issue 119 · Markdown 行内保全交付证据

关联 [Issue #119](https://github.com/puzige/vega/issues/119) 与 [冻结规格](vega-issue-119-markdown.md)。当前为实现与本地回归交付，真实应用验收、云端门禁及集成由主控完成；本文不宣称 Issue 已关闭。

## 根因与实现

紧凑列表的 pulldown-cmark 事件没有 `Paragraph` 包裹。原 `parse_blocks` 遇到 `Strong` / `Emphasis` / `Strikethrough` / `Link` / `Image` 的 Start 时先 flush 裸文本，再按未知块跳过整个容器，造成内容丢失和额外段落。

修复将现有行内转换提取为私有 `parse_inline_event`，显式段落和块层裸行内事件共享该消费器；块层只在真正块边界 flush。图片继续沿用现有 alt 文本降级行为，不增加图片节点。缓存、引用定义失效、公共 API、依赖及 UI 生产代码均未修改。

改动文件：`crates/vega_markdown/src/nodes.rs`、`crates/vega_markdown/src/stream.rs`、`crates/vega_ui/src/conversation_stream/tests/model_markdown.rs`。

表格中两种情况必须区分：`改为 **"…"** 的` 是合法强调；`改为**"…"**的` 是字面量。这符合 [CommonMark 0.31.2 §6.2 Example 380](https://spec.commonmark.org/0.31.2/#example-380) 的定界符规则。新增测试直接比较 pulldown 事件转换与 MarkdownStream 的逐字分片、整段重放，并断言合法粗体/代码/链接，以及转义星号、代码内星号、未闭合强调和中文引号边界，不重写用户原文。Issue 没有原始 Markdown，本次 fixture 是自主构造的同类输入。

## Freeze

- branch：`feat/issue-119-markdown`；基线与实际本地路径只保存在持久证据 `implementation-manifest.json`。
- verified_at_utc：2026-09-22T08:01:42Z；verified_at_local：2026-09-22T16:01:42+08:00。
- tracked code diff SHA-256：`4d908fe24e0bc818003fb2c7ad62be15f6643577ad3736e038fcdf5d1472406f`。
- 环境：macOS 15.8 / arm64；rustc 1.98.0；cargo 1.98.0；git 2.55.0。
- 独立 worktree 默认 Cargo target；没有链接其他 target。首次执行遇到环境 sccache 引用已删除临时目录，主控批准仅本次命令 `RUSTC_WRAPPER=`，没有修改用户配置。首次环境失败和真正失败复现均保留。
- 原始日志和 `.exit` 文件留在 worktree 外的持久 `issue-119` 证据目录。仓库文档只列文件名、摘要与哈希。时间为日志文件创建至最后写入的观测区间，不作为性能基准。

## Results

| 要求 | evidence class | exact command | 结果 | 观测时长 | 原始日志 |
|---|---|---|---|---|---|
| 环境首次尝试 | 环境诊断 | `cargo test -p vega_markdown issue119_tight_list_preserves_inline_content_in_one_paragraph` | exit 101；编译前失败，0 tests | 0.55s | `parser-before.log` |
| T1 修复前复现 | UNIT/PROPERTY，公开 MarkdownStream | `RUSTC_WRAPPER= cargo test -p vega_markdown issue119_tight_list_preserves_inline_content_in_one_paragraph` | exit 101；0 passed / 1 failed | 6.82s | `parser-red.log` |
| T1 同一断言修复后 | UNIT/PROPERTY，公开 MarkdownStream | 同上 | exit 0；1 passed / 0 failed | 5.92s | `parser-green.log` |
| T1–T4 parser、流式、缓存边界 | UNIT/PROPERTY | `RUSTC_WRAPPER= cargo test -p vega_markdown` | exit 0；36 unit + 3 doc passed / 0 failed | 7.85s | `markdown-tests.log` |
| T4 首次 UI 编译 | 编译诊断 | `RUSTC_WRAPPER= cargo test -p vega_ui conversation_stream::tests::model_markdown` | exit 101；0 tests，测试 selector 生命周期错误 | 104.04s | `ui-markdown-tests.log` |
| T4 会话生产 model / renderer | GPUI 生产渲染路径集成，非原生应用 E2E | 同上 | exit 0；17 passed / 0 failed | 30.05s | `ui-markdown-fixed-tests.log` |
| 格式 | 静态检查 | `cargo fmt --all -- --check` | exit 0；空输出 | 约 3s 命令耗时 | `fmt-final.log` |
| T5 真实 Vega 流式输出与重开 | 原生应用 E2E | 主控执行 | NOT RUN | — | 待主控补证据 |

修复前原始失败 footer：

```text
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 32 filtered out; finished in 0.00s
```

失败断言中实际为两个 Paragraph，仅分别含「选 」与「，继续」；预期为同一 Paragraph 内依次保留 Text、Strong(A)、Text。先失败后成功没有改此断言。

完整 markdown 日志 footer：

```text
test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.97s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
```

| 日志 | SHA-256 |
|---|---|
| parser-before.log | `728837b28046668b29729155eee3bd45a775efee24f3d774faaa68f2ec1e3afe` |
| parser-red.log | `8a95b57c7049cc1203ef32715d327256cdb0d35219d350f38a8bb17637e3339b` |
| parser-green.log | `097272040279e23e80e54eae1b850e67c340863827e402aaae4621201804e0f2` |
| markdown-tests.log | `956729f63b79f74f70b475b6517786f5f15f4691cb371546d4e9faf731beb252` |
| fmt-final.log | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

UI 首次编译暴露新增测试给 `debug_bounds` 传了非静态字符串。修正为静态 selector，并先断言生产 block ID 与表格 ordinal；没有泄漏字符串或修改任何语义断言。重跑 footer：

```text
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 410 filtered out; finished in 0.17s
```

| 日志 | SHA-256 |
|---|---|
| ui-markdown-tests.log | `35139fc3fffc597ec0df52e190b2f09f0190caa6fbb52b234cc57b8ed4ccfd11` |
| ui-markdown-fixed-tests.log | `b4058caee5a6639e8da7147e2a45df8ad16c23b5a59820f59c6167d081850d43` |

## 验收矩阵与边界

- T1 PASS：单个紧凑列表内完整保留前文、粗体、后文，只生成一个隐式段落。
- T2 PASS：有序列表从 3 开始、嵌套无序任务列表、嵌套粗斜体、删除线、链接标题、图片 alt 与 inline code 均保留。逐字符分片（包括拆开定界符及 URL）与历史整段输入节点一致。
- T3 PASS：松散列表真实空行分段与 blockquote 保留；紧凑列表软换行映射空格、硬换行映射换行符；任务状态保持；空文档没有虚假节点。既有冻结块只解析一次、后到引用失效、未闭合 fence 和 finish 测试全绿。
- T4 PASS：合法样式与字面量精确断言；新 UI 回归以真实 MarkdownStream → StreamModel → markdown_item 链验证完整列表文字/样式、无额外段落及结构化表格列对齐。
- T5 NOT RUN：主控报告日常 Vega 正在执行另一个任务，不替换或退出其进程；需要空闲后的本次构建原生流式和重开证据。

## Residuals

- LIMIT：本地 GPUI 测试能够证明生产模型和渲染布局，不能证明真实 provider、原生窗口像素或持久化重启行为。
- LIMIT：图片仍为 alt 文本；不成对强调或不满足 CommonMark 定界条件的星号仍可见，属于输入语义。
- NOT RUN：云端 PR check、真实应用验收、merge / 安装 / 清理 / Issue 回写由主控负责。未执行 push、merge 或日常应用安装。
- LIMIT：现有依赖 `block 0.1.6` 有 Cargo future-incompatibility 提示；本卡未修改依赖，当前测试通过。
- 规格偏离：无。无 schema 迁移；回滚为撤销本卡代码改动。
