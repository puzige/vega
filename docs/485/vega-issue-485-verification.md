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
| 接线修正后 RED | `9fad414853eaebf441b377e13aee4a5ab32f68c9`；[run 39391629989](https://github.com/puzige/vega/actions/runs/39391629989) / [Nextest job 118524298969](https://github.com/puzige/vega/actions/runs/39391629989/job/118524298969) | 4 发现 / 4 执行 / 2 通过 / 2 失败 / 0 跳过；两项目标引用仍为相同溢出失败；Thinking、短引用/代码保护通过，Clippy 通过 |
| 最终接线基线 RED | `3974c56508cb9c782213c36fbd032f4a66569275`；[run 39391982124](https://github.com/puzige/vega/actions/runs/39391982124) / [Nextest job 118525446775](https://github.com/puzige/vega/actions/runs/39391982124/job/118525446775) | 4 发现 / 4 执行 / 2 通过 / 2 失败 / 0 跳过；仍只有两个目标引用右边界失败，保护用例和 Clippy 通过。完成后才明确委派生产修复 |
| 最小生产实现 | `97bc60b5fa75094c2bc70ae561d3435a56ff7e84`；最终 RED 完成后才修改两个 Quote 分支的正文容器 | 只增加 `div().flex_1().min_w_0()` 包裹正文；冻结测试、观测与依赖未变 |
| 同一冻结测试 GREEN | `97bc60b5fa75094c2bc70ae561d3435a56ff7e84`；[run 39393025425](https://github.com/puzige/vega/actions/runs/39393025425) / [Nextest 118528873301](https://github.com/puzige/vega/actions/runs/39393025425/job/118528873301) / [required check 118530841744](https://github.com/puzige/vega/actions/runs/39393025425/job/118530841744) | 4 发现 / 4 执行 / 4 通过 / 0 失败 / 0 跳过；两个目标引用与两个保护用例全部通过，fmt / Clippy / 三组 Python 通过，完整 gate 成功 |
| 独立删测 | GREEN 与恢复补丁已固定在 [PR 评论](https://github.com/puzige/vega/pull/511#issuecomment-6286818254) 后才委派 | 清理已完成；只删除四项开发测试、透明观察器/模块及 dev-only test-support，lock 恢复生产基线；两个已验证的正文容器逐字保留 |
| 最终空集完整门禁 | 待删测后最新 head | 尚未取得；空集须标 `no Rust tests`（0 发现/0 执行），不是回归测试通过 |

首次原始失败：普通路径文本 `Bounds { origin: Point { x: 38px, y: 48px }, size: Size { 1888px × 23px } }`；可选择路径为 `(38px, 32px)`、同样 `1888px × 23px`。文本预算只有 226px。完整失败已固定在 [PR 评论](https://github.com/puzige/vega/pull/511#issuecomment-6286485439)。Clippy 另因冗余 `AppContext` 导入失败；Thinking 因缺 `vega_theme::Theme` 全局初始化失败。上述两项是测试接线错误，单独修正并重新云端验证，不放宽断言或冒充产品修复。独立审查进一步确认链接绘制同样读取 Vega 主题全局，因此其余三个测试也按真实应用初始化 Vega 主题；不能依赖另一个测试的独立 AppContext。原始修正见 [第一次接线补丁](vega-issue-485-test-wiring-correction.patch) 和 [主题初始化补丁](vega-issue-485-theme-wiring-correction.patch)，仅改环境，不改冻结验收。

GREEN 原始摘要：`Starting 4 tests across 14 binaries`；四项逐条 `PASS`；`Summary [0.139s] 4 tests run: 4 passed, 0 skipped`。三组 Python 原始输出分别为 5、15、14 项通过。独立生产审查确认唯一源代码差异是两个正文容器，文本、选择、链接与竖线不变。冻结测试与所有观察器、manifest、lock 相对最终 RED head 逐字一致。

准确计数均以 Nextest 原始输出为准；忽略数未单独取得，不猜测。完整 PR gate 保留 fmt、全目标 Clippy、三组 Python 检查和 Nextest。

## 本地静态检查

以下检查已执行，退出码为 0：

- `git diff --check`。
- 锁定 Rust 1.98.0 的 `rustfmt --edition 2024 --check`（本轮修改文件）。
- `cargo metadata --offline --locked --no-deps --format-version 1`，仅元数据解析，无编译或执行。
- 锁文件图静态比较：仅新增 10 个 test-support 传递包；既有 package 版本/来源/checksum 不变，既有 GPUI 包仅增加其测试支持依赖边，其他多版本依赖仅补充消歧版本号。
- 测试先行阶段的子 agent 生产投影静态比对：移除本轮测试模块和观察接线后，涉及的既有 Rust 文件与生产基线逐字一致。
- GREEN 后清理：workspace 384 个 Rust 文件的测试属性/测试 cfg、专属 hooks、test-support manifest、显式测试 target 和可执行 doctest 静态清点为 0（仅静态结果，实际发现/执行仍由 Actions 确认）。原有 `text` 文档围栏不变。
- 清理后非文档差异相对 `3a2ef8e9` 只有 `render_rows.rs` 两个正文容器（3 插入 / 3 删除）；其余源码、manifest、lock 与生产基线逐字一致。对照带测试 GREEN，renderer 仅移除了两个透明 bar 观察闭包，正文容器实现不变。
- 最终非文档差异 SHA-256：`372cd2ac85ba26e6403ed3f19a0495fac81b654198ba8c6e545a016f7356f9c8`；`render_rows.rs` SHA-256：`5d657bc04b5c6939019a3baae18c7c345f8ae2133857d36fe6b0c39d64b8be4f`。

未本地运行 Vega、Cargo check/Clippy/test/nextest、编译、构建或打包。没有使用 CUA。Issue 附件仅通过下载和本机 OCR 读取，未驱动桌面或复现 UI。

## 用户手测与回滚

尚未取得用户手测结论。最终需要用户在窄/宽正文中核对引用，分别检查实时和历史消息、中文/英文/富文本、跨视觉行选字复制、链接交互及浅深主题。发布或合并不代表应用已安装，也不代表手测通过。手测前保持 Issue 开放。

回滚只撤销本卡生产布局差异，不涉及数据、解析、字体、主题或权限配置。
