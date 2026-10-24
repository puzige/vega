# Issue #589：轻量 i18n（en / zh-CN），用户可见文案英文化

- Issue：[#589](https://github.com/puzige/vega/issues/589)
- 看板：Vega Desktop，Status=Backlog，Size=XL
- 基线：`a100f730`（origin/master）
- 规则：[AGENTS](../../AGENTS.md)、[执行总纲](../vega-exec-guide.md)、[设计守则](../vega-design-guidelines.md)
- 状态：规格已冻结，待实现。本地不编译、不测试。

## 1. 决策

- 方式：**轻量 i18n 框架，中英双语**（`en` 默认 + `zh-CN`），资源随二进制内嵌。
- 范围：**仅用户可见 UI 文案**。代码注释、`tracing` 日志、内部 `Debug` 输出不在范围。
- 标题口径：**「置顶 / 项目 / 最近任务」三个栏目标题只做翻译，不改名**（2026-10-24 用户确认）。

| key | en | zh-CN |
|---|---|---|
| `sidebar.pinned` | `Pinned` | `已固定` |
| `sidebar.projects` | `Projects` | `项目` |
| `sidebar.recents` | `Recent tasks` | `最近任务` |

## 2. 现状盘点（静态）

无 i18n 设施：`crates/` 下无 `locales` / `i18n` / `translations` 目录，依赖中无 `rust-i18n` / `fluent`。

中文字面量 **1536** 处 / 126 个文件：

| crate | 数量 |
|---|---:|
| vega_ui | 991 |
| vega | 325 |
| vega_conversation | 134 |
| vega_runtime | 56 |
| vega_store | 22 |
| vega_theme | 5 |
| vega_tools | 3 |

热点文件：`settings/mcp.rs` 151、`settings/render_impl.rs` 70、`settings/provider_management.rs` 68、`settings/skills.rs` 55、`settings/agents.rs` 45、`settings/model_catalog.rs` 43、`sidebar/threads_block.rs` 34。

含插值文案 87 处，例如 `format!("正在重新连接… {attempt}/{max_retries}")`、`format!("预览 {}", state.name)`。

**非 UI 文案**：`conversation_stream/bench.rs`、`render_rows.rs` 中的中文是 benchmark/fixture 数据，不迁移。

关键接入点：

- 配置：`vega_store/src/config.rs:142` `UiPrefs { theme, sidebar_collapsed, sidebar_width, projects_collapsed, sessions_collapsed }`，需新增 `language`。
- 设置页：`vega_ui/src/settings/render_impl.rs:601`「外观与布局」组，含「主题」「侧栏」，语言项并入该组。
- 侧边栏标签：`sidebar/projects_block.rs:446`、`organization/render.rs:484`、`:544`、`:625`、`:651`。

## 3. 方案

1. 新增 `vega_i18n`（或并入 `vega_theme` 同级的轻量 crate）：
   - 资源文件 `locales/en.toml`、`locales/zh-CN.toml`，`include_str!` 内嵌。
   - API：`t(key) -> &'static str` 与 `t_args(key, &[(&str, &str)])`；缺失 key 回退 en 并 debug 断言。
   - 当前语言来自全局，切换时触发重绘。
2. `UiPrefs.language: String`，`#[serde(default)]`，默认 `"en"`；旧配置缺字段可正常加载。
3. 设置页「外观与布局」新增语言选择器（`English` / `中文`），切换即时生效并持久化。
4. 按 crate 分批迁移，每批后跑静态盘点脚本输出剩余中文字面量。

## 4. 验收矩阵

| ID | 前置 | 必须观察 |
|---|---|---|
| A01 | 全新配置启动 | 全 UI 英文；侧边栏 `Projects` / `Recent tasks` / `Pinned` |
| A02 | 切中文 | 立即重绘为中文；不可译项（品牌名、模型名、路径）保持原样；重启保持 |
| A03 | 切回英文 | 立即恢复；重启保持 |
| A04 | 旧 config（无 `language`） | 正常加载，落默认 `en`，不报错、不重置其他偏好 |
| A05 | 触发含参数提示 | 参数正确替换，语序符合当前语言，无 `{0}` 裸露 |
| A06 | 遍历侧边栏 / 设置 / 会话流 / 错误提示 | 用户可见区无遗留中文；benchmark 数据不计入 |
| A07 | 键盘遍历主要控件 | `aria_label` 随语言切换，读屏可读 |

## 5. 迁移顺序

1. 框架 + 语言设置 + 侧边栏三个标签（可见收益，风险最低）。
2. `vega_ui`（991）→ `vega`（325）→ `vega_conversation`（134）→ `vega_runtime` / `vega_store` / `vega_theme` / `vega_tools`。
3. 每批完成后核对剩余清单，不机械全局替换。

## 6. 非目标

- 不改注释、日志、内部错误枚举输出。
- 不迁移 benchmark/fixture 样例数据。
- 不引入第三方 i18n 运行时或远程翻译服务。
- 不改布局尺寸、排版 token、主题。

## 7. 约束

- 本地不编译、不测试、不打包，仅静态检查；验证交云端 PR Check，语言切换观感交用户手测。
- 代码改动走 worktree + PR。
- 逐条甄别「用户可见」与「测试数据」，禁止全局替换。
