# 引用折行修复：验证与证据

关联：[#485](https://github.com/puzige/vega/issues/485)、[draft PR #511](https://github.com/puzige/vega/pull/511)。规格：[引用折行](vega-issue-485-markdown-quote-wrap.md)。

## 测试先行冻结

- RED 生产基线：`3a2ef8e9795d609f022b7b81011360db102e3de6`。
- 测试先行提交：[`524a558b2f9141035cdbf40711f2813e3c2882cf`](https://github.com/puzige/vega/commit/524a558b2f9141035cdbf40711f2813e3c2882cf)。此提交未修改生产折行逻辑，只有开发测试、透明只读观测、dev-only test-support 和规格。
- 冻结源码：[`quote_layout_tests.rs`](https://github.com/puzige/vega/blob/524a558b2f9141035cdbf40711f2813e3c2882cf/crates/vega_ui/src/conversation_stream/quote_layout_tests.rs)、[`thinking_body.rs`](https://github.com/puzige/vega/blob/524a558b2f9141035cdbf40711f2813e3c2882cf/crates/vega_ui/src/conversation_stream/thinking_body.rs)。
- 可恢复原始补丁：[frozen-tests.patch](vega-issue-485-frozen-tests.patch)，相对上述生产基线，仅含本轮 Rust 测试、只读观测、manifest 与锁文件接线；SHA-256：`17ca355a402411b2df69b8f59701f96aa3a21971d159b07cde0e589075ac0ce1`。
- `quote_layout_tests.rs` SHA-256：`572dbfb4988c31335394c425b91b51207498c7bd22647a83fe6c4bc3f417d81d`。
- 先行补丁经独立审查。实际文本必须等于冻结预期，实际字符坐标验证纵向分行和横向边界；富文本按完整片段集合观测，不要求单个定位片段再次折行。

### 冻结测试

| 测试 | 行为与保护边界 |
|---|---|
| `markdown_quotes_reflow_within_the_quote_gutter` | 普通 Markdown 生产入口；中英文普通/富文本长引用、pending/finished、240→480→240 新布局；完整文本、竖线/间距、字符与文本边界、实际多视觉行、宽窄高度变化 |
| `selectable_quotes_reflow_without_copying_soft_newlines` | 同一矩阵走可选择生产入口；额外观察真实 `MessageCopy::visible_text()`，不增加软换行 |
| `short_quotes_stay_single_line_and_code_keeps_horizontal_layout` | 两条正文路径中短引用保留单行、长代码保留单行与横向内容范围，投影不变 |
| `thinking_quotes_keep_width_constrained_layout_and_copy_projection` | 思考区原有显式宽度排版的长短引用与复制投影保护 |

输入、预期和断言冻结于测试先行提交；实现后不得追改断言、增加忽略或补测追认。默认 GPUI 测试平台使用合成字体度量，这些是实际生产布局约束测试，不是原生字体像素测试。三次宽度在同一 GPUI 上下文生成新布局，不代表真实持久化窗口 resize 验收。可见文本投影不等同于执行了选择/剪贴板或链接激活。

## 云端阶段

首次 RED Actions：[run 39390563261](https://github.com/puzige/vega/actions/runs/39390563261)，head `524a558b2f9141035cdbf40711f2813e3c2882cf`。

| 阶段 | head / 证据 | 结果 |
|---|---|---|
| 首次目标行为 RED | 上述测试先行 head；[Nextest job 118520786535](https://github.com/puzige/vega/actions/runs/39390563261/job/118520786535) | 4 发现 / 4 执行 / 1 通过 / 3 失败 / 0 跳过；两个目标引用测试实际文本为 1888px × 23px，在 240px 列中超出右边界，形成可归因行为 RED。短引用/代码保护通过；Thinking 缺主题全局的接线 panic 不计作目标 RED |
| 接线修正后 RED | 待测试-only 接线修正 head | 移除未使用的 AppContext 导入，补齐 Thinking 主题初始化；输入/预期/断言不变，先取得保护用例通过再实现 |
| 最小生产实现 | 待有效 RED 后委派 | 尚未修改 |
| 同一冻结测试 GREEN | 待实现 head 的完整现有 gate | 尚未取得 |
| 独立删测 | 待 GREEN 固定后执行 | 尚未执行 |
| 最终空集完整门禁 | 待删测后最新 head | 尚未取得；空集须标 `no Rust tests`（0 发现/0 执行），不是回归测试通过 |

首次原始失败：普通路径文本 `Bounds { origin: Point { x: 38px, y: 48px }, size: Size { 1888px × 23px } }`；可选择路径为 `(38px, 32px)`、同样 `1888px × 23px`。文本预算只有 226px。完整失败已固定在 [PR 评论](https://github.com/puzige/vega/pull/511#issuecomment-6286485439)。Clippy 另因冗余 `AppContext` 导入失败；Thinking 因缺 `vega_theme::Theme` 全局初始化失败。上述两项是测试接线错误，单独修正并重新云端验证，不放宽断言或冒充产品修复。

准确计数均以 Nextest 原始输出为准；忽略数未单独取得，不猜测。完整 PR gate 保留 fmt、全目标 Clippy、三组 Python 检查和 Nextest。

## 本地静态检查

以下检查已执行，退出码为 0：

- `git diff --check`。
- 锁定 Rust 1.98.0 的 `rustfmt --edition 2024 --check`（本轮修改文件）。
- `cargo metadata --offline --locked --no-deps --format-version 1`，仅元数据解析，无编译或执行。
- 锁文件图静态比较：仅新增 10 个 test-support 传递包；既有 package 版本/来源/checksum 不变，既有 GPUI 包仅增加其测试支持依赖边，其他多版本依赖仅补充消歧版本号。
- 子 agent 生产投影静态比对：移除本轮测试模块和观察接线后，涉及的既有 Rust 文件与生产基线逐字一致。

未本地运行 Vega、Cargo check/Clippy/test/nextest、编译、构建或打包。没有使用 CUA。Issue 附件仅通过下载和本机 OCR 读取，未驱动桌面或复现 UI。

## 用户手测与回滚

尚未取得用户手测结论。最终需要用户在窄/宽正文中核对引用，分别检查实时和历史消息、中文/英文/富文本、跨视觉行选字复制、链接交互及浅深主题。发布或合并不代表应用已安装，也不代表手测通过。手测前保持 Issue 开放。

回滚只撤销本卡生产布局差异，不涉及数据、解析、字体、主题或权限配置。
