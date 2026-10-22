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
