# #534 交付与恢复检查点

## 阶段与版本

2026-10-22T09:56:50Z：先行测试准备，尚未进入生产实现；未执行 CI，未取得 RED/GREEN。

- 生产基线：`e4f18c656aa69d192d095a4338a2108e6c9a3c2c`。
- 分支：`feat/534-composer-overflow`，任务 worktree：`vega-534-composer-overflow`。
- [任务契约 v1](vega-issue-534-composer-overflow.md)，SHA-256：`8b929021fedde40a40d3b770cfc8f15e16e7d9d19852cbb771be2272dc012c38`。
- 原写入者已取消，执行状态确认 interrupted；未收到 READY，不能将中断产物当成已审查或完整测试。
- 已保留测试观察器、13 个测试用例、dev-only image/gpui_kit 接线与 cfg(test) 观察点，不重做已完成产物。

停写时精确内容标识：

| 范围 | SHA-256 |
|---|---|
| `git diff --binary`（仅 tracked） | `91b7fb014627fc7381e599cb788b88b324166913a32bbf4eb0d7f8ad56e3bcaa` |
| `crates/vega_ui/src/overflow_test_support.rs` | `da787250805d6b2f8bc778a6b00b7dee15a4460fda03978143d5dbcb8798cbbe` |
| `crates/vega_ui/src/conversation_stream/composer_overflow_tests.rs` | `323a683a4e99da6cd182722780c67edc17a4d987c7d96169228356aad3d07b67` |

## 已知事实与未决项

- Composer 滚轮只接大编辑器模式、无滚动条，是静态定位的产品缺陷；尚无行为测试执行证据。
- 图片缺陷尚待真实解码后布局/绘制确认；现有名义尺寸 72 × 72 不自动证明裁剪正确或失败。
- 采用 GPUI Kit 0.6.1 / gpui-pre 0.3.4 公开 `HeadlessAppContext::with_platform` + `PlatformHeadlessRenderer::render_scene` 记录真实生产 Scene。观察器不生成产品像素截图，不冒充原生外观或系统手测证据。
- `image` 已为 workspace 生产依赖并锁定 0.25.10；本卡仅临时在 vega_ui dev-dependencies 复用，不引入新 crate/版本。
- 本地 `git diff --check` 退出 0。
- 本地 `cargo fmt --all -- --check` 退出 1：`conversation_stream/render.rs` 的测试观察接线存在格式差异；这是静态格式问题，不是有效 RED。
- 主 Agent 静态发现测试 `focus_input` 对 `FocusHandle::focus` 调用只传 window，锁定 API 需要 `(window, cx)`；待独立审查确认并交专职写入者修观察层，不能将编译错误算产品 RED。

## 下一步

1. 独立非写入者只读审查上述固定测试检查点，识别实际 API/观察层问题及 A1–A8 验收缺口。
2. 单一专职写入者仅修测试接线，保留验收含义与断言，报告 READY 停写。
3. 独立审查后建立 draft PR，现有 Actions 确认有效 RED；之后才生产实现、同一测试 GREEN、PR 固定原始 patch/完整 SHA/日志、独立删测与最终空集门禁。
4. 只有最终 head required check 通过并独立审查无缺口才合并，保持 Issue 开放、In review 等用户手测。

所有自动运行仅交 GitHub Actions；本地未运行 Vega、编译、构建、打包或执行测试。

## 2026-10-22T10:21:45Z 测试补丁修复与前置审查

中断检查点经主 Agent（非原测试写入者）只读审查，确认测试目标对应 A1–A8；记录场景与布局而非伪造像素，不将观察器空图像当截图。另一次独立子 Agent 审查未交付结果并已确认 interrupted，不将其记作通过。主 Agent 已逐段核对固定测试、观察支持与锁定 API。

唯一代码写入者已报告 READY test-harness-repair 并停写：四处 `FocusHandle::focus(window)` 按实际 API 改为 `focus(window, cx)`，并修正格式；断言、输入数据、验收和观察器未改。

| 内容 | SHA-256 |
|---|---|
| 修复后的 tracked diff（不含 untracked） | `ad17ac0300684ac1b2c5a764dfe6618959b7c2ba39ec6aaef4cd5d9c36dd79a2` |
| `composer_overflow_tests.rs` | `aff292f0e5c2548c9ee1d3f0e0217e1512495805ad438a65c795461b589484ad` |
| `overflow_test_support.rs`（未改） | `da787250805d6b2f8bc778a6b00b7dee15a4460fda03978143d5dbcb8798cbbe` |

- 修复 Agent：`cargo fmt --all`、`cargo fmt --all -- --check`、`git diff --check` 均退出 0。
- 主 Agent 对停写版本重跑 `cargo fmt --all -- --check`、`git diff --check` 均退出 0；fetch 后与 origin/master 无差异（0 ahead / 0 behind），生产基线未变。
- 共 13 个先行测试；预期长文本滚动/滚动条行为失败，短内容/单行/大编辑器为阳性对照。图片是否产生基线失败等待 CI，不能预先声称 RED。
- 下一步：本补丁作为 tests-first 提交及 draft PR 运行现有 Actions。只有目标行为缺失的有效 RED 才进入生产修复；无关编译/观察层失败先诊断，保留断言与原始失败。

## 首轮 Actions 与责任层诊断

先行测试提交：`71fbd69f7e51574c37595d5bd7b26eba5a23018c`；[draft PR #539](https://github.com/puzige/vega/pull/539)。
[run 39762723610](https://github.com/puzige/vega/actions/runs/39762723610)，[Nextest job 119770594344](https://github.com/puzige/vega/actions/runs/39762723610/job/119770594344)。命令沿用现有 `cargo nextest list --workspace --message-format json` 与非空 `cargo nextest run --workspace`。Clippy/fmt job 成功。

Nextest 原始总结：13 executed，2 passed，11 failed，0 skipped；ignored 精确清点未取得，不猜测。单行输入和 full editor 阳性对照通过。

- **有效行为 RED**：`composer_wheel_and_trackpad_reach_both_ends_without_editing_or_snapback` 在 +3 行滚轮后仍显示 `draft 13`–`draft 20`，期望 `draft 10`–`draft 17`；滚动条用例在缺少可见 scrollbar 处失败。
- **观察点错误，非产品 RED**：图片测试实际 sprite `(45,733;72×36)`，绝对定位 tracker canvas 却报告 thumbnail `(45,787;72×72)`。tracker 放在图片之后且无 inset，并不可靠代表外层缩略图盒。移到实际外层 Element 的透明观察 wrapper，不修改布局或断言。
- **测试清理错误，非产品 RED**：IME 用例在 HeadlessAppContext drop 时报告 leaked ConversationStream/TextInput。Harness 中 context 字段先于 entity 字段 drop；应释放句柄后再 drop context，不关闭泄漏断言。
- **测试尺寸前提，待修观察层**：8 行高度测试用未取整 `13×1.55=20.15`，而 GPUI `TextStyle::line_height_in_pixels` 按像素取整为 20；8 行误差 `1.2 > 1`。仅纠正测量单位，不放宽容差。
- **测试 resize 前提，待修观察层**：`TestWindow::resize` 仅改平台 bounds，不触发布局 callback；需通过公开 `Window::bounds_changed(cx)` 使 viewport 更新。不把未发生的缩窄当产品失败。

主 Agent 已读取原始日志与锁定源码完成责任层诊断；下一轮先修观察层并独立核对，不改 A1–A8，不改行为断言、不重试换绿、不先实现或补测追认。图片是否为有效 RED 等正确观察点的云端证据。

## 修正观察层后的有效 RED 与测试冻结

修正提交：`438b442b22666b316571544cbf0dd18f0884b968`，生产行为仍与原基线一致。

- [run 39765222873](https://github.com/puzige/vega/actions/runs/39765222873)，[Nextest job 119778877048](https://github.com/puzige/vega/actions/runs/39765222873/job/119778877048)。Clippy/fmt 成功，Nextest 如预期失败；没有自动重试。
- 原始输出：13 tests across 15 binaries，13 executed，3 passed，10 failed，0 skipped。通过用例为单行、大编辑器和 IME；ignored 精确清点未单独取得。
- 图片有效 RED：真实外层 thumbnail `(45,715;72×72)`，竖图实际可见绘制 `(45,715;72×144)`，确实越界；多图用例同样在图片不属于任何缩略图边界处失败。
- 长文有效 RED：+3 行滚轮后仍为第 13–20 行而非 10–17；滚动条缺失；1–8 行几何与空/短文本前提已通过，实际溢出时才在缺 scrollbar 处失败。
- 窄宽更新和观察点前提修复有效；软折行用例在窄输入后显示尾部 `LAST` 的断言失败，归本轮视口可见内容问题，不改变断言。

冻结来源：[测试源码](https://github.com/puzige/vega/blob/438b442b22666b316571544cbf0dd18f0884b968/crates/vega_ui/src/conversation_stream/composer_overflow_tests.rs)，SHA-256 `f992b3baeaef49e0d0b11b0dd389ff5afb0890736f7c7f41f82a1ec391100b6e`；观察器 SHA-256 `66bc47a673bb9e0f2753374722b446efd2236ace62dd15dfec16a76101b80747`。主 Agent 已独立核对修正差异；验收、数据、容差未放宽。

本轮从此冻结测试，不补测、不追改预期。专职生产写入者获得实施授权：固定真实 img 尺寸与裁剪，修 Composer 滚轮、滚动条和光标跟随协调；不更改预测、IME、发送数据或单行/大编辑器契约。本地仍仅静态检查，等待停写版本与同一测试云端 GREEN。

## 首轮生产 GREEN 与独立审查追加边界

生产提交：`6d42df0e8451a5307553f32a837d4c9d093ba265`。专职写入者 READY 停写；四个生产文件的原始差异 SHA-256 `50af6c5ad2d7950405e71cbff623863b00ad677699e6ca7b0fcd2f6d149aeb7c`。

[run 39768887370](https://github.com/puzige/vega/actions/runs/39768887370)，[Nextest job 119791131907](https://github.com/puzige/vega/actions/runs/39768887370/job/119791131907)，[Clippy job 119791131313](https://github.com/puzige/vega/actions/runs/39768887370/job/119791131313)，[required check 119793917149](https://github.com/puzige/vega/actions/runs/39768887370/job/119793917149) 全部成功。

- 同一冻结 13 个测试：13 discovered，13 executed，13 passed，0 failed，0 skipped；ignored 未单独提取。Python 三组 5+15+40 全部 OK。
- 主 Agent 对比冻结提交与 GREEN 提交的测试/观察器，diff 为空；未修改断言、数据或跳过用例。
- 原始源码、patch、完整 SHA 和 RED/GREEN 证据在 [PR #539 留档](https://github.com/puzige/vega/pull/539#issuecomment-6339552984)并通过 API 读回来源可达。

独立非实现者审查请求更改：`text_input/element.rs:225–253` 在完整剩余字符串中按下一字形位置选断点，然后独立排版前缀，却不检查最终前缀宽度。上下文字距会使独立前缀比先前位置更宽，存在 A5 未覆盖的裁切/进入滚动条槽风险。主 Agent 已复查实际 `closest_index_for_x` / `x_for_index` / `shape_line` 语义，确认责任层为生产折行逻辑而非 CI 或旧测试错误。

这是遗漏边界，不把旧 GREEN 充作其证据，也不实现后补测。规格 v1.1 追加 A9，以 `6d42df0e8451a5307553f32a837d4c9d093ba265` 为新生产基线重新先测后修；先保留旧 13 个测试并写 A9 及必要阳性对照，再独立审查/云端有效 RED 后修生产。原轮 A1–A8 的 GREEN 结论保留，新轮全部回归仍须 GREEN；现阶段不删测、不转正式、不合并。
