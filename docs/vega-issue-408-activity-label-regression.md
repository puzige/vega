# Issue #408 大小写修复回归方案

## 现状与根因

用户提供的截图显示运行活动阶段标题为 `run commands`。该内容对应 `RunActivityGroup` 阶段标题，不是单张工具卡摘要。

已合入的 #408 修复曾在 `853aaee1` 中为阶段标题与工具组聚合行增加首位短语首字符大写，并更新相关回归断言。当前安装包版本为 0.1.137，仓库 `origin/master` 对应提交 `e168653b`。对比提交证明 #429 在合入时将这几处生产逻辑及大小写断言改回小写，并移除了共享 helper 和 #408 文档。因此截图所见是当前代码回归，不是仅凭现有证据推测的旧安装问题。

## 冻结契约

- `ToolActivityCategory::action_phrase()` 保持小写原值。
- 在会话阶段标题与工具组聚合摘要中，首位动作短语只将首字符做 Unicode 大写转换；该字符之后内容逐字保留。后续动作短语继续小写。
- 阶段标题的长文案截断保持 96 字符上限和原有省略格式；截断后的首位短语仍大写。
- `ToolActivityGroup::aggregate_summary()` 的 `Not run after takeover` 分支、状态前缀、类别顺序、去重、图标与计数行为保持不变。
- `#[cfg(test)]` 的 `RunActivityGroup::activity_presentation` 镜像采用同一大小写规则，避免测试投影与生产文案分叉。
- 不修改单卡摘要、命令标题、详情、权限卡、模型/用户内容、命令文本或工具输出。摘要暂时覆盖阶段标题时仍显示摘要原文。

## 修改与验收矩阵

| 用例 | 修改/预期 | 验证 |
|---|---|---|
| 单类别阶段标题 | `run commands` → `Run commands`；`read files` → `Read files` | 现有阶段标题 GPUI 回归断言 |
| 多类别阶段标题 | `Read files, run commands`，仅首位大写 | 现有多类别阶段标题回归断言 |
| 工具组运行/完成聚合 | `In progress: Run commands, read files`、`Completed: Run commands` | 现有工具组摘要断言 |
| 单活动测试镜像 | `In progress: Run commands, read files`，后续类别小写 | 现有单活动标签断言 |
| Unicode/边界 | 只大写首字符，空串保持空串，类别原文其余字符不变 | `sentence_initial` 单元断言 |
| 截断 | 首位大写仍保留 96 字符上限与 `, …` 结尾行为 | 阶段标题 helper 单元断言 |
| 摘要标题覆盖 | 模型摘要显示时不强制替换成工具类别短语 | 既有 #421 阶段标题断言 |

实现限于共享首字符 helper、上述三条显示调用路径，以及必要的既有测试期望。不得顺带改变其他工具文案或状态语义。

## 验证边界

不在本地编译、构建、运行 Vega 或执行测试。仅做快速差异、格式和静态文本检查；生产回归由最新提交的 GitHub Actions PR Check 执行。合并后仍需用户手测确认截图场景显示 `Run commands`。
