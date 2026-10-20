# #506 思考档位 Light 改为 Low

## 范围与基线

- Issue：https://github.com/puzige/vega/issues/506
- 方案 v1，2026-10-20。用户本轮明确授权 Vega 协调，覆盖 Codex/Vega 角色分工限制；其余仓库约束不变。
- 生产基线：`e93518cf76a24df921a1af2c2663ebce75f614fd`。
- 分支：`feat/506-thinking-low`；使用本卡独立 worktree，不改主 checkout 的用户改动。
- 目标：将思考档位 `low` 的显示名称从 `Light` 改为 `Low`，沿用其他英文档位首字母大写的风格。
- 非目标：不重命名内部 effort、持久化偏好、请求字段，不迁移数据，不修改浅色主题，不改交互、布局、模型能力或其他档位。

## 根因与调用链

`crates/vega_ui/src/conversation_stream/thinking_slider.rs::tier_display_label` 对 `low` 显式返回 `Light`。`ThinkingSliderModel::label` 调用该映射，composer 模型选择器中的思考滑块视图消费此 label。`tier` / `choice_name` 继续返回内部名称。单点修改映射可修正展示，不需要改变数据或协议。

## 先行验收与测试冻结目标

先写下面两个行为测试，全部进入同一测试先行提交；生产实现必须保持基线。测试放在现有 `thinking_slider.rs` 的独立 `#[cfg(test)]` 模块，不新增依赖、公开 API、fixture 或执行设施。

| 用例 | 前置与操作 | 可观察结果 |
|---|---|---|
| low_thinking_tier_displays_low_without_renaming_choice | 用公开 `ThinkingSliderModel::new` 构造包含 low/medium/high 的模型，当前选择 low | `label()` 为 `Low`；`tier()` 与 `choice_name()` 仍为 `Some("low")` |
| other_thinking_labels_and_empty_states_are_unchanged | 检查其余已知档位、enabled、未知名称与空名称；构造 Off、provider default 和无档位状态 | 原标签保持 None / Minimal / Medium / High / Extra High / Max / Ultra / Persistent / 开启思考；未知名称继续仅首字母大写，空名称为空；Off / Default / 无选择的空标签保持不变 |

这是对真实纯状态模型与显示边界的断言，不检索源码字符串、不 mock 映射。预期 RED 为第一个测试实际得到 `Light` 而非 `Low`；无关编译/依赖/runner 失败不算有效 RED。第二个测试应通过。

冻结后不得补测、改预期或扩大验收；新增需求须独立重开先测后修周期。

## 顺序与允许修改范围

1. 专职子 agent 仅在 `thinking_slider.rs` 添加上述测试，不修改生产代码；主会话审查并提交本规格与测试，建立 draft PR（base master）。
2. 现有 GitHub Actions 给出有效 RED 后，主会话固定测试提交、源码与原始失败链接。
3. 同一子 agent 将 `tier_display_label` 的 `"low" => "Light"` 映射改为 `"low" => "Low"`。只改这一处生产字符串，不重构、不更改冻结测试。
4. 同一 draft PR 验证带相同测试的 GREEN，并固定源码/完整 SHA、run/job 和准确计数。
5. GREEN 后以独立提交删除本轮测试模块；不得改生产实现。最终差异仅包含本规格与上述一行字符串修改。
6. 最新 head 完整门禁成功，确认 Rust 0 发现/0 执行（`no Rust tests`）后转正式 PR、审查并 squash merge。Issue 保持开放，进入 In review 等用户手测，不自动关闭。

本地仅允许低 CPU 差异、格式和静态检查，不运行 Vega、编译、Clippy 或任何测试。自动验证仅使用现有 `.github/workflows/pr-check.yml`；CI 查询间隔至少 5 分钟，收到通知或用户即时请求除外。

## 云端验证命令

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- 现有三组 Python 检查：`python3 scripts/release_tests.py`、`python3 scripts/prepare_computer_use_tests.py`、`python3 -m unittest discover -s scripts/tests -v`。
- `cargo nextest list --workspace --message-format json` 后，非空执行 `cargo nextest run --workspace`；删测后仅在发现数为 0 时执行 `cargo nextest run --workspace --no-tests=pass`。
- required check：`check (fmt, clippy, test)`。

未运行或计数未取得须如实标记。原始测试及完整提交固定在 PR，不能只依赖本机文件或临时 artifact；删测后的空集成功不替代带测试 GREEN。

## 用户手测与回滚

合并后的版本由用户在支持 low 档位的模型上选择该档，确认 composer 模型选择器的思考滑块显示 `Low`；切换其他档位、Default、Off（模型支持时）确认名称与操作不变。浅深主题不因本卡改变。不以本地运行或 agent 截图冒充用户验收。

回滚仅恢复本行显示映射到 `Light`，按正常 PR/CI 流程交付；无需数据回滚或配置迁移。

## 验证记录

- PR：[Draft #510](https://github.com/puzige/vega/pull/510)。用户授权本轮角色限制例外；验收与实现范围无偏离。
- 原始规格与冻结测试提交：`f0a89cab1d8634e4b02551dd420ba413bd9e824b`；生产仍为上述基线。原始测试源码和完整补丁固定在 [PR 评论](https://github.com/puzige/vega/pull/510#issuecomment-6285666842)，删分支或 squash 后仍可找回。
- 有效 RED：[Actions 39386142098](https://github.com/puzige/vega/actions/runs/39386142098)，[Nextest job 118506332465](https://github.com/puzige/vega/actions/runs/39386142098/job/118506332465)。2 发现/2 执行，1 通过、1 失败、0 跳过；目标断言实际 `Light`、预期 `Low`。不是编译、依赖或 runner 失败。其他标签/Off/Default/空态测试通过，fmt/Clippy 与 34 个 Python 测试通过。
- 实现与带同一测试 GREEN 提交：`9fad7f981abf4cc6fd19644cd861c4b5e927fda3`。只改一行映射，冻结测试逐字未变。[Actions 39387192301](https://github.com/puzige/vega/actions/runs/39387192301) / [Nextest job 118509742363](https://github.com/puzige/vega/actions/runs/39387192301/job/118509742363)：2 发现/2 执行/2 通过、0 失败/忽略/跳过；Python 5+15+14=34 通过，fmt/Clippy/required check 成功。
- 带测试 GREEN 已固定在 [PR 评论](https://github.com/puzige/vega/pull/510#issuecomment-6285919573)，随后才授权独立删除本轮两个 Rust 测试和所属 55 行测试模块；无独占依赖、fixture 或其他接线需要清理。生产、Python、CI、benchmark 与打包能力不变。
- 本地仅运行 `cargo fmt --all -- --check`、`git diff --check` 与只读差异/静态比对；未运行 Vega、编译、Clippy 或任何测试。
- 独立删测提交与最终最新 head 的完整门禁结果在 PR 回写；尚待验证，不以空集代替上述带测试 GREEN。用户手测尚未完成，Issue 保持开放。
