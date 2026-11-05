# ✦ Vega

**Native AI Agent Desktop** — Rust + GPUI 原生 Agent 工作台

<p>
  <img src="assets/logo/vega-icon-smile-light.svg" width="128" alt="Vega logo">
</p>

Vega 将模型对话、编码任务、工具执行和改动审阅放进同一个 macOS 原生工作台。界面使用 Rust + [GPUI](https://www.gpui.rs/) 和 Metal 渲染，不依赖 Electron；自研 Vega Runtime 直连模型 API，也支持通过 ACP 接入 Codex。

- **围绕项目工作**：管理项目与会话，引用文件，查看工具活动、Git Diff 和生成的产物。
- **自主选择模型与扩展**：配置 Provider、模型与推理偏好，通过 MCP 和 Skills 扩展任务能力。
- **掌握执行边界**：区分询问、规划与执行，选择权限策略，查看 Token 用量与费用信息。

[安装与上手](#安装与上手) · [配置与扩展](#配置与扩展) · [开发](#开发) · [文档导航](#文档导航)

## 状态

Vega 正在持续开发。以下能力概览描述当前 `master` 源码，不表示任意旧版安装均已具备；可下载版本及变更记录以 [GitHub Releases](https://github.com/puzige/vega/releases) 为准。

目前发布面向 **macOS Apple Silicon（arm64）**，不在此承诺 Intel Mac、Windows 或 Linux 桌面版支持。原生渲染是技术选择，不代表已经达到特定帧率、启动耗时或相对内存指标；历史 Sprint 报告和测试数字也不代表当前版本的验收结果。

## 核心能力

| 能力 | 使用方式与边界 |
|---|---|
| 多模型对话 | 在设置中管理 Provider、模型、推理偏好与能力配置；Native 运行时适配模型 API 差异。实际能力取决于所选模型与服务。 |
| 项目与编码任务 | 引用项目文件和图片，读写文件、执行命令，结合工具活动、Diff 与产物检查结果。工具执行受当前模式和权限约束。 |
| 长任务与上下文 | 流式 Markdown、代码高亮和可展开的执行过程；支持上下文压缩、停止任务及中断后的显式续接，不因重启自动恢复执行。 |
| 会话分支 | 从符合条件的 Native 历史回复创建新分支，保留原会话；分支不继承活跃执行权或旧权限批准。 |
| Skills 与 MCP | 管理技能并在单条消息中引用；连接 MCP 服务扩展工具。服务启用、认证和执行授权是不同步骤。 |
| 桌面操作 | 观察 macOS 窗口、截图及受控操作，支持暂停和用户接管；需要相应系统权限，不等于可绕过应用或系统限制。 |
| Codex 接入 | 通过 ACP 在项目中使用外部 Codex Agent；需单独配置和认证，其模型、模式与审批遵循外部 Agent 的能力，不与 Native 设置混用。 |
| 用量与日常体验 | 查看 Token 用量与费用信息，使用中英文界面、主题和应用更新。费用取决于可用的用量与定价数据，不是已核验的服务商账单。 |

## 安装与上手

1. 在 [Releases](https://github.com/puzige/vega/releases/latest) 下载 `Vega-macos-arm64.zip` 和同版 `.sha256`，核对下载完整性。正式发布的更新清单另有独立 Ed25519 签名；SHA-256 摘要本身不证明发布者身份。
2. 解压后将 `Vega.app` 放到 `/Applications/Vega.app` 或 `~/Applications/Vega.app`，从安装位置打开，不把构建目录作为日常安装位置。
3. 在设置中添加 Provider，保存 API Key 并选择模型；密钥通过界面保存，不写进项目文件或 YAML。
4. 选择项目或创建独立任务，确认模型、任务模式和权限后发送需求。需要 Codex 时先在 Agents 设置中完成外部 Agent 配置与认证。

**从旧版升级前请注意**：当前版本使用 `~/.vega`，不会自动迁移旧目录中的历史、凭据或配置。启动后看不到旧历史不代表旧数据已删除；需要保留数据时，先阅读[配置与手动转移说明](docs/820/configuration-and-manual-transfer.md)，不要直接改配置后缀或复制运行中的 SQLite 主文件。

应用更新支持签名校验和确认后重启安装。独立更新签名与 Apple Developer ID 签名、公证不是一回事；若 macOS 拒绝打开，先检查该版本的签名、公证与发布说明，**不要关闭 Gatekeeper 或移除 quarantine 来绕过保护**。详见[安装与分发](docs/vega-packaging.md)和[更新机制](docs/181/vega-issue-181-independent-update-signing.md)。

## 配置与扩展

### 模型与凭据

优先通过设置界面管理 Provider、模型、推理偏好和模型能力。Native 使用 Vega 配置；ACP 任务使用外部 Agent 返回的配置和认证流程。不要把“可配置”理解为所有模型都支持图片、推理或工具调用。

默认目录布局：

```text
~/.vega/
├── config.yaml                   # Provider、默认模型、界面、模型能力与 MCP 配置
├── credentials/credentials.toml  # 独立凭据文件，不纳入项目版本管理
├── data/vega.db                  # 会话与任务数据库
├── data/scratch/                 # 独立任务文件
└── skills/                       # 技能目录
```

可用非空绝对路径 `VEGA_HOME` 隔离另一套数据；无效路径会报错，不静默回退。凭据文件为受文件权限保护的独立明文存储，不是加密保险库。YAML 格式、凭据权限与手动转移步骤见[配置说明](docs/820/configuration-and-manual-transfer.md)。

### Skills、MCP 与执行权限

- **Skills**：在设置中管理，在消息输入中引用所需技能；引用作用于该条消息，不等于永久固定到会话。外部链接目录的授权与恢复需显式检查。
- **MCP**：在设置中添加、编辑和启用服务，按需完成认证。配置文件声明不自动授予执行权；工具仍受当前运行的权限策略约束。
- **模式与权限**：Ask / Plan / Execute 描述任务方式，ReadOnly / Confirm / Auto / Full access 描述权限策略。Full access 需明确授权，具有更高风险；Ask / Plan 不因选中 Full access 就变成可写执行模式。运行中的权限快照不会因设置改变而立即收紧，需要立即中止时使用 Stop。

具体边界见[权限与沙箱](docs/624/vega-permission-sandbox-alignment.md)、[Skills](docs/74/vega-issue-74-skills.md)和 [MCP](docs/73/vega-issue-73-mcp.md) 规格。

## 开发

参与前先读 [`AGENTS.md`](AGENTS.md) 和[执行指南](docs/vega-exec-guide.md)。项目采用 SDD，行为改动需对应具体规格；使用独立分支与 worktree，通过 PR 交付，不直接向 `master` push。

### 前置要求（macOS）

- Apple Silicon Mac 与完整 Xcode；GPUI 的 Metal 着色器工具链不能仅由 Command Line Tools 替代。
- 通过 [rustup](https://rustup.rs/) 管理 Rust，工具链版本以 [`rust-toolchain.toml`](rust-toolchain.toml) 为准。

**当前仓库规则禁止 Agent 在本地运行 Vega 代码、编译、构建、打包或测试**，包括 `cargo check`、Clippy 和定向 test / nextest。本地只做低 CPU、快速完成的差异、格式与静态检查；自动测试交 GitHub Actions，打包走云端发布。不因历史文档含本地命令而视为已获执行授权。

### 质量门禁（云端 CI）

| Workflow | 当前职责 |
|---|---|
| [`pr-check.yml`](.github/workflows/pr-check.yml) | PR 面向 `master` 时，并行运行 Clippy 与 Nextest 两个 job：前者检查格式和全目标 Clippy，后者运行三组 Python 检查及 workspace Nextest；汇总为 required check `check (fmt, clippy, test)`。 |
| [`cicd.yml`](.github/workflows/cicd.yml) | `master` push 后调用发布 workflow，为合并版本自动预留 patch 版本并发布。 |
| [`release.yml`](.github/workflows/release.yml) | 统一处理自动发布、`v*` tag 和手动发布，构建 arm64 包、签署更新资产并发布 GitHub Release；Apple 签名与公证取决于发布凭据配置。 |

Rust 测试仅供开发辅助和留痕，不得进入 `master`；现存 Python 测试保留。最终 Rust 空集必须如实记录为 **0 发现 / 0 执行**，不能写成“Rust 测试通过”。详情见[验证与交付政策](docs/vega-rust-test-delivery-lifecycle.md)。

合并前需完成非实现者独立审查并通过最终 head 的 required CI；CI 不能替代原生界面和真实系统权限的用户验收。发布流程见[发版指南](docs/vega-release.md)。

## 文档导航

README 保留上手与关键入口；Ticket 规格、交接和验收记录在 [`docs/<ticket>/`](docs/README.md)，历史报告继续保留在 `docs/`，不作为当前完成度声明。

| 主题 | 入口 |
|---|---|
| 协作与交付 | [仓库规则](AGENTS.md) · [执行指南](docs/vega-exec-guide.md) · [Issue 工作流](docs/vega-issue-workflow.md) |
| 产品与设计 | [设计守则](docs/vega-design-guidelines.md) · [UI v3](docs/296/vega-issue-296-ui-redesign-v3.md) |
| 模型兼容 | [统一适配](docs/494/vega-issue-494-chat-compat-v3.md) · [兼容基线与能力配置](docs/494/vega-issue-494-model-compat-baseline.md) |
| 上下文与分支 | [上下文压缩](docs/667/vega-issue-667-claude-compaction.md) · [Native 会话分支](docs/851/validation.md) |
| 外部 Agent | [ACP / Codex](docs/vega-acp-codex-v1-spec.md) |
| 扩展与桌面操作 | [Skills](docs/74/vega-issue-74-skills.md) · [MCP](docs/73/vega-issue-73-mcp.md) · [桌面权限引导](docs/307/vega-issue-307-cua-onboarding.md) |
| 配置与发布 | [配置及手动转移](docs/820/configuration-and-manual-transfer.md) · [打包与安装](docs/vega-packaging.md) · [发版指南](docs/vega-release.md) |

## License

TBD（私有开发中）
