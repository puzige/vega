# Issue #81：移除 Pi Agent 相关内容

状态：实现规格；当前产品行为以本文件为准。

## 目标与范围

移除 Settings → Providers 中从 Pi Agent 导入凭据的功能，包括入口、命令分发、运行状态文案、Pi `models.json` 路径解析与读取逻辑，以及专用错误类型。普通 Provider 设置继续支持手动新增/编辑、在 Vega keystore 中保存和读取凭据、查看已存储状态、显式启用/禁用、模型发现和连接测试。

已有凭据没有导入来源标记，因此本任务不清除凭据、不自动启用或禁用 Provider、不修改配置格式或数据库。Provider 用户设置与 keystore 内容在更新后保持不变；用户仍可从 Settings → Providers 编辑和重新输入 API Key。

Settings → Skills 的通用外部目录导入保持不变：仅通过用户选择的精确目录预览并经显式确认关联，不递归探测相邻客户端目录。`.pi/agent/skills` 等未选择的目录仍不得自动扫描。保留相关安全测试和历史 A7-02 交付证据；历史证据描述的是旧版本行为，不作为当前验收契约。

## 验收矩阵

| ID | 需求/风险 | 前置状态 | 实际操作 | 预期可观察结果 | 测试层级 | 证据 | 状态 |
|---|---|---|---|---|---|---|---|
| I81-1 | Pi 导入入口和状态不存在 | Settings → Providers 已选中 Provider | 检查详情页控件与状态 | 没有 Pi 导入按钮、Pi 导入状态或对应命令分发路径 | GPUI / 代码 | 定向 Provider UI 回归与源码检查 | PASS |
| I81-2 | 普通 Provider 设置仍可操作 | 已配置 Provider | 查看详情，切换启用状态，发现模型并测试 | 标准控件和操作仍可用，启用状态只因用户显式操作改变 | GPUI / service | Provider UI 与 mock-network 回归 | PASS |
| I81-3 | 已存储或手动输入的凭据保留 | 有 owner-only keystore 凭据的 Provider | 打开设置、编辑并重新保存 API Key，再切换启用状态 | 打开设置不改变状态；手动保存的 key 可用；已存储 key 不被清除；Provider 仅按用户操作启用/禁用 | GPUI / service | #81 定向保留边界测试及现有凭据恢复测试 | PASS |
| I81-4 | Provider 配置的既有数据不迁移 | 配置中含 enabled/disabled Provider 和 key_ref | 用 Vega 加载配置并编辑普通字段 | enabled、key_ref、模型和 keystore 内容保持原样；不创建导入迁移 | service | #81 定向服务测试 | PASS |
| I81-5 | 通用外部 Skills 目录导入保留 | Settings → Skills 已打开 | 使用原生目录选择器选目录、预览精确根并确认 | 仍能显式导入所选目录；不读取目录外内容 | GPUI / service | `issue74_native_folder_picker_previews_exact_root_before_link` | PASS |
| I81-6 | 未授权客户端目录不自动扫描 | 未链接外部 Skills 源 | 运行 Agent Skills 回归 | 未选择的外部根不会进入 catalog、请求或日志；`.pi/agent/skills` decoy 不进入 model catalog 或技能正文 | production / security | `issue74_vega_owned_global_requires_exact_ui_link_and_uses_config_dir_root` | PASS |

## 实现计划

1. 移除 Provider Settings UI 的 Pi 命令、异步处理、入口与专用状态选择器；移除仅为该测试注入路径的 Settings 状态。
2. 移除 `ProviderSettingsService` 中 Pi 源路径、解析/校验/读取与导入方法及 `PiSource` 错误；保留通用 Provider 表单、keystore 和启用状态接口。
3. 删除只覆盖已移除 Pi 凭据导入功能的服务/UI 测试，改为 Provider 移除/保留边界的定向回归；保留 Skills 外部目录导入回归，并明确保留 `.pi/agent/skills` 未扫描的安全断言。
4. 更新当前规格索引与仍用于未来验收的 A7/#82 契约；不改写已完成 A7/#82 交付记录或原生验收结果。
5. 只运行本卡的 Provider 与 Skills 定向 nextest；检查格式、差异和代码引用。云端 `pr-check` 承担完整门禁。

## 交付证据

测试命令、结果与残余见 [交付记录](vega-issue-81-remove-pi-agent-delivery.md)。
