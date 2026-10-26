# #505 Composer 模型列表展示 Provider 分组 — 实现规格

## 基线

- Issue：<https://github.com/puzige/vega/issues/505>（`[Bug] 模型列表没有展示providers`）
- 基线：`410758d3`（`origin/master`）
- 分支 / worktree：`feat/505-model-providers` / `worktrees/vega-505-model-providers`

## 问题与根因

Composer 模型选择器的候选来自 `crates/vega/src/window/session.rs` 的 worker 读取：
遍历 `config.providers`（仅 `enabled`）的 `models`，把模型 ID 推入 `Vec<String>`。
provider 名称在该投影中被丢弃，`ConversationStream` 只拿到扁平的模型字符串，因此列表无法显示来源。

同一模型 ID 若在多个 enabled provider 中出现，`unique_provider_for_model` 返回 `None`，
该模型不会进入候选列表（既有歧义保护，本卡不改）。所以列表里每个模型都能唯一反推一家 provider，
分组不会引入新的 provider 选择歧义。

## 展示契约（2026-10-26 用户确认）

```
选择模型                        12px text_tertiary

OpenRouter                     组头 12px text_tertiary，不可点
Claude Sonnet 4.5              13px text_secondary
GPT-5                          13px text_secondary

DeepSeek                       组头
DeepSeek V3.2                  13px brand_primary            ✓
DeepSeek R1                    13px text_secondary
```

- **组头与模型行同一条左基线，不缩进**：两级靠字号（12 / 13）与颜色（tertiary / secondary）区分，不靠位移。
- 组头与模型行左内距一致（现有行 `px_2` = 8）。
- 组头高度 24，模型行高度沿用 32。
- 相邻组之间 4 顶部间隔；第一组不额外加间隔（复用面板标题的 4 底部内距）。
- 组头随列表滚动，不做 sticky；不显示模型数量，不可点，无 hover，不进入键盘序列。
- 组顺序等于 `config.providers` 声明顺序，不排序、不按字母重排。
- 只有一个 provider 时同样显示组头（本卡诉求就是让来源可见）。
- 无可用模型的 provider 不渲染空组头。
- 当前模型行右侧显示 ✓（固定标记列，未选中行同样占位，标签不位移）。
- 底栏胶囊不显示 provider，仍只显示模型名。

新增 token：`Layout::COMPOSER_PICKER_GROUP_HEIGHT = 24.0`、
`Layout::COMPOSER_PICKER_GROUP_GAP = 4.0`；标记列复用 `vega_ui::menu_list::selection_marker`
（宽 `Typography::SIDEBAR` = 13）。

## 数据契约

- 新增跨 crate 共享类型 `vega_conversation::types::ModelOption { provider: String, model: String }`。
- `VegaWindow::configured_models`、`model_options_for_pricing()`、
  `ConversationStream::model_options`、`apply_model_options()` 由 `Vec<String>` 改为 `Vec<ModelOption>`。
- worker 的过滤、去重与顺序规则不变：去重仍按模型 ID（`option.model`），
  仍受 `unique_provider_for_model` 约束，仍只在 `enabled` provider 中收集。
- 键盘高亮仍是模型扁平等值下标；组头不占下标，`on_model_previous/next`、
  `open_model_picker_list`、`select_model_option` 语义不变。
- 列表滚动使用子元素下标，组头会占一个子元素位置；因此 `scroll_to_item` 的入参
  由 `model_picker_row_index` 统一换算（组头数 + 模型下标），渲染与滚动共用
  `model_picker_starts_group`，避免两处分组规则漂移。
- 选中请求仍只发模型 ID；provider 解析、保存链路、运行态互斥、reasoning 投影一律不改。

## 非目标

- 不支持同一模型按不同 provider 分别选择（会改变保存与请求语义）。
- 不改 Settings 的 Provider 列表与模型目录（`model-catalog`）能力。
- 不改底栏胶囊、思考档位面板、权限面板。
- 不新增依赖、DDL、IO、网络调用或注释。

## 验收矩阵

| # | 场景 | 可观察结果 |
|---|---|---|
| A1 | 两个 provider 各有模型 | 列表按声明顺序出现两个组头，组头在其模型之上 |
| A2 | 组头位置 | 组头与模型行左边界相同，无缩进；字号 12 / 13 |
| A3 | 当前模型 | 该行文字为 brand_primary，行尾 ✓；其他行无 ✓ 但保留同宽标记列 |
| A4 | 键盘导航 | ↑/↓ 只在模型间移动，跳过组头；高亮背景正确；Enter 选中高亮模型 |
| A5 | 选择保存 | 请求仍为模型 ID，成功 ack 后胶囊与高亮更新，列表关闭 |
| A6 | 单 provider | 仍显示一个组头 |
| A7 | 空 provider / 全禁用 / 配置读取失败 | 无空组头；列表为空时不渲染浮层 |
| A8 | 同名模型跨两个 provider | 沿用既有歧义保护，该模型不出现，不新增可选路径 |
| A9 | 滚动 | 组头与模型行一起滚动，标题固定；最大高 320 不变 |

## 验证方式

按仓库规则：本地只做静态检查（格式、差异、符号清点）；编译、Clippy 与 nextest 由
GitHub Actions `pr-check` 执行；原生 UI 由用户在合并后的 master 手测。
