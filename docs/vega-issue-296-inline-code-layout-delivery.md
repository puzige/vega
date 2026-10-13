# Issue #296 · 行内代码与表格排版交付

依据 [冻结规格](vega-issue-296-inline-code-layout.md)。本轮测试基线为 `5c7885c7517b3f1ba4978fd40ef9ead7501d78b1`；执行期间获知 master 新增 `effcb9f`，本轮未在写入中 rebase，更新基线后的集成验证由主控补充。

## 变更与验收

共享行内代码绘制使用原 StyledText 布局和 UTF-8 ranges，按真实视觉行绘制居中的薄圆角底，再绘制选区和字形。实际背景高度为 `min(font_size + 4, line_height)`；窄片段用 GPUI `clamp_radii_for_quad_size` 主动限制圆角。字形端点查询排序去重后按原 glyph 顺序单遍求值，保留 GPUI first-match 语义；背景 ranges 按片段截取，避免每次换行重扫全文。

表格在 materialization 时缓存每列最长物理行 display width，两个 renderer 与估高共享冻结权重。估高按各 cell 分配后的内容宽计算行数、上下 padding 和分隔线；列表表格的缩进条件与实际 renderer 一致，包括首 child 就是表格的例外。未改 parser、原文、运行状态、主题色、fenced code 或链接行为。

| 验收 | 自动证据 | 结果 |
|---|---|---|
| T01 | 两主题、两 renderer 的实际 quads 高度/四角、字体/行高、邻字边界；10px 实际字形的6px窄片段四角夹到3px | PASS，`review-green.log` |
| T02 | CJK/emoji/长 code/相邻代码的各片段范围，真实拖选与 Cmd-C；中/右对齐表格逐行背景；旧 #147 两主题完整/局部选区绘制层级 | PASS，`review-green.log` |
| T03 | 宽/窄两 renderer 的长内容列分配、各行同轴/最小宽；实际 cell 宽与共享公式对照 | PASS，`final-precision.log` |
| T04 | 需求更新、相同版本复用 Arc、逐行估高、空 cell/物理空行、宽度变化、实际测量优先/非 tail 原身份锚点；列表缩进及首 child 表格；原 #59 窄四列480与独立局部横滚 | PASS，`regression.log` 中相关已通过项、`final-table.log`、`review-green.log` |
| T05 | 原 #147 混合块复制/失效/长 fenced code、#119 流式和重放、模型内容与冻结 materialization；思考复用 Markdown 的选区/原文复制 | PASS，`regression.log` 中相关42项；共享背景优化后的选区增量见 `review-green.log` |
| M01 | 原生浅深主题、宽窄窗口观感及复制/横滚 | **NOT RUN**，留给用户手测 |

旧 #59 实际表格测试的长中文样本在描述列变宽后不再必然换行，因此仅在该测试的构造输入内重复中文 cell；共享 `ISSUE59_TABLE` 和所有原换行、同列边界、最小宽、480总宽、局部滚动断言保留。未把旧失败日志改写为全绿。

几何精度区分：背景 quad 单边允许最多0.5物理px；cell 宽度允许最多1物理px（两边界各0.5累积），均按实际 scale 验证。诊断曾测得 `actual=138 expected=137.9277 selectable=true width=800`。锁定 GPUI `taffy.rs` 禁用 Taffy 自身 rounding，在设备像素空间对绝对边界执行 `round_half_toward_zero` 后回到逻辑坐标；这不是列宽或 padding 公式偏差。

## 命令、退出码与原始结果

均从本 worktree 根目录执行；每次 stdout/stderr 保存到 `~/Workspace/artifacts/vega-296-inline-code-layout/<log>`。下面编号代表完整 cargo 命令，未启用 retry。

```sh
# A
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t01/) | test(/issue296_inline_t03/)'
# B
cargo nextest run -p vega_ui -E 'test(/issue296_inline/) | test(/issue147_inline_highlight/)'
# C
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t03/)'
# D
cargo nextest run -p vega_ui -p vega_markdown -E 'test(/issue147_markdown_selection/) | test(/model_markdown/) | test(/issue119/) | test(/issue296_expansion_p07_markdown_selection/) | test(/width_remeasure_preserves_anchor_identity/) | test(/prepend_and_column_remeasure_restore_stable_pixel_anchor/)'
# E
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t04/) | test(/issue59_real_table/) | test(/issue59_nested_tables/) | test(/width_remeasure_preserves_anchor_identity/) | test(/prepend_and_column_remeasure_restore_stable_pixel_anchor/)'
# F
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t03/) | test(/issue296_inline_t04/)'
# G
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t01_narrow/) | test(/issue296_inline_t04/)'
# H
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t01_narrow/)'
# I
cargo nextest run -p vega_ui -E 'test(/issue296_inline_t01/) | test(/issue296_inline_t02/) | test(/issue296_inline_t04/) | test(/issue147_inline_highlight/)'
```

| 日志 | 命令 | 实际退出码 | 原始 footer / 编译结果 |
|---|---|---:|---|
| `red.log` | A | 101 | 编译失败：测试窗口 helper 返回借用，与声明的拥有类型不符；未运行测试 |
| `red-business.log` | A | 100 | `Summary [   0.051s] 2 tests run: 0 passed, 2 failed, 718 skipped` |
| `green-first.log` | A | 100 | `Summary [   0.052s] 2 tests run: 1 passed, 1 failed, 718 skipped` |
| `geometry.log` | B | 101 | 编译失败：测试对 ScaledPixels 调不存在的 abs；未运行测试 |
| `geometry-second.log` | B | 101 | 编译失败：残留一处相同测试 API 用法；未运行测试 |
| `geometry-third.log` | B | 100 | `Summary [   0.115s] 11 tests run: 10 passed, 1 failed, 712 skipped` |
| `column-diagnostic.log` | C | 100 | `Summary [   0.054s] 1 test run: 0 passed, 1 failed, 722 skipped` |
| `regression.log` | D | 100 | `Summary [   0.355s] 43 tests run: 42 passed, 1 failed, 716 skipped` |
| `final-table.log` | E | 0 | `Summary [   0.187s] 5 tests run: 5 passed, 718 skipped` |
| `final-precision.log` | F | 0 | `Summary [   0.080s] 2 tests run: 2 passed, 721 skipped` |
| `review-red.log` | G | 101 | 编译失败：新增 fixture 误在 ListState 调用 stream 的 invalidate_item；未运行测试 |
| `review-business-red.log` | G | 100 | `Summary [   0.079s] 2 tests run: 0 passed, 2 failed, 722 skipped` |
| `review-narrow-red.log` | H | 100 | `Summary [   0.032s] 1 test run: 0 passed, 1 failed, 723 skipped` |
| `review-green.log` | I | 0 | `Summary [   0.123s] 11 tests run: 11 passed, 713 skipped` |

失败解释：业务首红分别是旧23px满行背景和等宽列；`green-first` 的剩余失败是113与112.600006物理px的绘制位置取整，厚度/圆角已通过。`geometry-third` / `column-diagnostic` 记录新增列宽精确断言对设备像素对齐未留精度；`regression` 唯一失败为上述旧中文样本不再换行。`review-business-red` 中首 child 表格确实多估23px（350对327）；另一个 hair-space 样本在测试平台仍是8.1px，未制造窄片段，属于 fixture 失败。`review-narrow-red` 改用真实继承10px字号后检出半径8物理px对目标6的业务失败。随后仅跑受改动影响集合。

`green-first` 的 unused Result 和 `final-precision` 的 unused scale 均为测试代码警告，已修正；最终日志只有依赖 `block v0.1.6` 的既有 future-incompatibility 提示。没有本地全量、Clippy、原生应用或 provider 操作。

### 原始日志 SHA-256

```text
red.log fdafabbc5f97ad903c34d75177a466e1eca04389df38af5fdb10ae08c0b4eb44
red-business.log 08fc0f4ee9926ae447958ac3ac3b62bb7a1d032cde9ca3db34d7985904c32544
green-first.log ebcd760fee6af5747705a6f0a46d9c3d500d67f34ea6105ba79bcc6cfdf47d61
geometry.log 1d50bf5d91f79ec35b144c39b39bfe93f8bf7252229625cc6491229fc0f8a4b5
geometry-second.log 1a2a1a7dd278adaa0016617daedce0662839a255bece149e28d9deca9b5a2bf6
geometry-third.log 563b87c5112336c2f96b40685306e10fdc609891004fd5b5c013422c1155e668
column-diagnostic.log 36398344adf07a34c9a7d391bc326ace5e3fe4b8ed43c5842870f77e43546a7a
regression.log aefd575552aa8c068a7940fe86294ec45545f2dd30c973a0f82a104ffe094fae
final-table.log 384d8271266116ac856d22d540abdd19ebc2b97c2cafe4238f67da7901c5b7c2
final-precision.log 1084c702abf938ac3d234837d1338791b4f1c454e1d87a09aa198e4fb7620d8b
review-red.log 57abe29750888c0a1f432c8f948b5445bbba3c3c85fc6783c5d36997c3887f81
review-business-red.log 29b2877886ea7786574815655083710794e8f7fd6993e99367f32d8cc1c91e35
review-narrow-red.log cec3d3d7772325e36f9be4ae5d2085b15e2d7737a14b8f27810b026e97c19e0f
review-green.log 43305dc9a459fc2b57bb6fd56e5847c9480c30aa433d36f9ac09451759b43eff
```

## 源码冻结与限制

源码冻结 UTC `2026-10-13T04:03:56Z`，本地 `2026-10-13T12:03:56+08:00`。环境：Darwin arm64；`rustc 1.98.0 (88d9e12ae 2026-08-18) (Homebrew)`；`cargo 1.98.0 (797e8a9bc 2026-08-05) (Homebrew)`；`git version 2.55.0`。

`source-fingerprint.txt` 收录按路径排序的9个变更源码/测试文件，包含未跟踪新文件 `inline_code.rs`；每行格式为 `relative_path<TAB>SHA256<LF>`。整份 manifest SHA-256：`92fb079be30a9526e45b7b6479d493dc9fec97d7b06ae964d0ad60aac65408f7`。主控规格文档和本交付文档不计入源码 fingerprint。

`cargo fmt -p vega_ui -p vega_theme --check` 与 `git diff --check` 均退出0。未 commit/push。GPUI 进程内几何、选择及复制已验证；原生字体观感/截图对照仍需 M01，未宣称像素级手测完成。无持久化迁移；可回滚本轮共享 Markdown 展示改动。

## 主控集成验证

实现审查通过后，主控将冻结改动提交并无冲突 rebase 到 `effcb9fc11f1b2c5df7501001d2191ac806acbab`。`git range-diff` 确认本轮补丁等价，未修改已审查实现。集成测试提交为 `dc96772df352bd054a0e3d9a6bda34f4b03520f2`，随后仅补充此交付记录。

重新执行上面的完整命令 D，实际退出0；`integration.log` 原始 footer：

```text
Summary [   0.358s] 44 tests run: 44 passed, 724 skipped
```

此结果包含所有最终新增用例、修正后的旧 #59 中文样本及相关选择、复制、语法、表格、估高和阅读锚点回归，不沿用旧基线失败批次的状态。日志 SHA-256 为 `8a9de3e05d94e1ccc096e5a84353a4cf380430374747c1ecf1474f4439e966eb`；集成后9个源码/测试的 `integration-source-fingerprint.txt` SHA-256 为 `2c680cbf2de217038d3b7d4c50660314acb4d3530dfd74d4d31302e16161a58f`。`git diff --check origin/master...HEAD` 退出0。云端全量门禁以 PR check 为准，M01 仍为 **NOT RUN**。
