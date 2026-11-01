# Vega 配置与手动转移

## 目录

默认根为原生用户目录的 `.vega`；设置非空绝对路径 `VEGA_HOME` 可隔离另一套安装。无效覆盖会报错，不会回退到默认目录。配置在 `config.yaml`，数据库在 `data/vega.db`，凭据仍为独立的 `credentials/credentials.toml`。Skills 在 `skills/`，独立任务文件在 `data/scratch/`。

新版本**不自动迁移、发现、备份或删除**旧 XDG/macOS 目录。启动后看不到旧历史不等于旧数据被清空。用户可以全新开始，也可以在重启前自行转移需要的内容。不要同时使用新旧版本写两套数据，也不要让多台机器并发写云盘中的 SQLite。

## YAML

最小配置：

```yaml
schema_version: 1
providers: []
defaults:
  model: ""
  permission_mode: confirm
  permission_revision: 0
ui:
  theme: dark
  language: zh-CN
agent:
  composer_predictions_enabled: true
  running_message_behavior: queue
  turn_limit: 0
```

Provider 示例（不填写密钥值）：

```yaml
providers:
  - name: example
    api: chat_completions
    enabled: true
    base_url: https://example.invalid/v1
    models: [example-model]
    key_ref: example
```

`key_ref` 指向凭据文件中的原始引用名。通过设置界面保存密钥；不要在 YAML、命令参数、日志或截图中粘贴密钥。凭据目录和文件分别保持 0700/0600；不支持的平台明确报错，不代表跨平台凭据存储已完成。

模型偏好也在同一文件中：

```yaml
reasoning:
  version: 1
  preferences:
    - provider: example
      model: example-model
      preference: high
model_capabilities:
  version: 1
  models:
    - provider: example
      model: example-model
      allowed_efforts: [low, medium, high]
      image_input: true
```

MCP 每项使用固定 `id`；显示名和排列顺序不是身份。请优先通过界面新增/编辑/启用服务。手工修改配置不代表程序已获激活、OAuth 授权或任务执行权限；旧任务不会因编辑 YAML 自动重放。

配置仅支持单文档、版本 1、无重复键的有界 YAML。不支持 anchors/aliases、merge keys、自定义 tags、插值或 include。格式/版本错误时保留原文件并阻止相关操作；先修复再重试。界面保存会检测已观察到的外部编辑冲突，冲突时重新加载后再编辑，不强行覆盖。

## 重启前的手动准备

1. 停止旧版 Vega、相关 helpers 和任务；自行保存旧安装完整离线备份。旧 macOS 数据通常位于 `~/Library/Application Support/ai.vega`，配置通常位于 `${XDG_CONFIG_HOME:-~/.config}/vega`；其他平台数据通常位于 `${XDG_DATA_HOME:-~/.local/share}/vega`。自定义路径以旧安装实际配置为准。
2. 决定全新开始还是保留数据。新版本不会为你执行复制或转换。没有明确数据路径和操作授权时，Agent 不会操作真实数据。
3. 若保留 SQLite，转移已停止写入的完整一致性数据库集合，包含需要的 WAL；不要只复制运行中的 `vega.db`。目标文件名为 `<新根>/data/vega.db`。完整历史图片保存在数据库 BLOB 中，不是另一个附件目录。
4. 将希望保留的 owned scratch/checkpoints/skills 与凭据按新布局放置并检查权限。外部项目不搬动。数据库中旧绝对 scratch/skill 引用可能需要手工处理；本版本不改写历史路径或承诺手动复制的完整性。
5. 旧 `config.toml`、`reasoning-preferences.toml`、`model-capabilities.toml` 的值须按新 YAML 结构转换或在 UI 中重设；**改后缀不是转换**。MCP YAML 声明与手动复制数据库的稳定 ID 应匹配，否则当前声明与历史关联可能不一致。不要伪造 grant/revision 来绕过权限。
6. 保留原备份，启动后核对所需对话、项目、任务文件、配置和凭据引用。缺少凭据时重新录入；程序未发起认证请求不代表凭据有效。恢复/回退前再次保存新版本产生的数据，避免覆盖新增历史。

这是一份手动准备说明，不是已经执行迁移、验证用户数据或授权清空的记录。
