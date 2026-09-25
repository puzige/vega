# Issue #178：Vega 自动更新方案

- **状态**：历史调研草案；实现已转入 [Issue #181 冻结规格](vega-issue-181-auto-update.md)，以下建议与待决策项按 #181 的裁决更新。
- **核实日期**：2026-09-24
- **Issue**：[puzige/vega#178](https://github.com/puzige/vega/issues/178)
- **目标平台**：当前已发布的 macOS Apple Silicon `.app`

## 1. 结论与建议

建议采用 Vega 自有的 Rust 更新流程，参考 Zed 的版本检查、下载状态和分发链路，参考 Codex 的自动更新提醒与手动检查入口。第一版只支持 stable release 和当前 macOS arm64 包，不引入新的更新服务器。

更新 UX 建议如下：

1. 默认开启后台版本检查，应用启动后执行一次，之后最多每 24 小时检查一次；设置中可以关闭自动检查，菜单中始终保留“检查更新”。
2. 发现新版本后，在满足签名校验条件时后台下载到临时目录，并显示版本、发行说明与下载状态。
3. 更新包准备完成后显示“重启以安装”与“稍后”选项。不得自动关闭 Vega 或中断正在运行的 Agent 任务；用户稍后可以从更新入口继续安装。
4. 如果发布包尚未通过 Developer ID 签名和公证，应用只提示新版本并打开官方 GitHub Release 页面，不在应用内替换 `.app`。

Vega 当前产物是 ad-hoc 签名且未公证。因此自动检查可以先实现，安全的应用内下载与替换必须等签名、公证和发布凭据配置完成后启用。Issue #178 作为方案任务完成前，需由产品负责人确认是否接受“自动下载、显式重启安装”的默认行为，以及 Developer ID 证书投入。

## 2. 范围与非目标

### 本期范围

- 查询当前 stable GitHub Release 的版本与发行说明。
- 对比当前应用版本，区分无更新、有更新、检查中、下载中、待安装和失败状态。
- 提供自动检查与手动检查入口；自动网络失败保持安静，手动检查展示可理解的错误。
- 在签名链路启用后下载、校验并暂存 macOS arm64 `.app` 更新；由用户决定何时重启安装。
- 仅替换当前正在运行的 `Vega.app`，保留 `CFBundleIdentifier = ai.vega`、用户配置和会话数据库。

### 非目标

- Windows、Linux、Intel Mac、nightly/preview 渠道。
- 更新 Vega 的 Git、系统依赖或用户工作区。
- 强制重启、后台退出应用、修改 Dock/Launchpad 或创建新的安装位置。
- 更新从源码 checkout、`target/` 或其他非正式 `.app` bundle 启动的开发实例。
- 在没有代码签名身份验证的情况下静默执行下载包中的程序或覆盖应用。

## 3. Zed 与 Codex 参考

| 项目 | 已核实行为 | 对 Vega 的启示 |
|---|---|---|
| Zed | 设置文档中 `auto_update` 默认开启，文字称自动检查更新。当前 `auto_update` crate 会按 release channel 周期轮询；发现较新版本后自动下载和安装，并进入 `Updated` 状态、设置重启路径。它还公开 `Checking`、`Downloading`、`Installing`、`Updated`、`Errored` 等状态，并提供手动检查 action。 | 状态模型和单一更新服务值得参考。Vega 的设置文字应准确区分自动检查、自动下载与安装，避免只写“自动更新”而隐藏网络和重启行为。 |
| ChatGPT 桌面端中的 Codex | OpenAI 帮助文档提到桌面应用会显示自动更新通知，并提供侧栏更新图标和菜单 `Check for Updates` 手动入口。当前 Codex 是该桌面应用中的独立视图；公开资料没有说明下载器、包格式或替换应用的内部实现。 | 保留用户可见的更新提示、手动复查和稍后操作；不推断或照搬未公开的实现细节。 |
| Vega | 发版由 `v*` tag 触发 GitHub Actions，上传 `Vega-macos-arm64.zip`。`.app` 使用 ad-hoc 签名，Developer ID 与公证仍是 `HUMAN PENDING`，目前没有版本检查或 updater。 | GitHub Release 可以先作为唯一版本源；自动替换必须依赖已配置并可验证的发布者签名。 |

参考资料：

- [Zed 自动更新设置](https://zed.dev/docs/reference/all-settings#auto-update)
- [Zed `auto_update` 实现](https://github.com/zed-industries/zed/blob/main/crates/auto_update/src/auto_update.rs)
- [OpenAI：ChatGPT Work and Codex](https://help.openai.com/en/articles/20001275-chatgpt-work-and-codex)
- [OpenAI：macOS 应用更新与签名证书说明](https://openai.com/index/axios-developer-tool-compromise/)

## 4. Vega 当前约束

- `.github/workflows/release.yml` 在推送 `v*` tag 后运行 `cargo xtask package`，并发布 `Vega-macos-arm64.zip`。
- `xtask/src/package.rs` 生成 `CFBundleIdentifier = ai.vega` 的 `.app`；当前通过 `codesign --force --deep --sign -` ad-hoc 签名。
- [打包文档](vega-packaging.md) 和 [发版文档](vega-release.md) 已保留 Developer ID、hardened runtime、notarization 与 staple 的人工配置步骤，但尚未配置。
- Release workflow 会用 tag 覆盖 `.app/Contents/Info.plist` 的 `CFBundleShortVersionString` / `CFBundleVersion`；Vega binary 目前没有读取运行中 app bundle 版本的 updater API。检查必须读取当前 `.app` 的 bundle 版本，不能用编译时固定的 workspace `CARGO_PKG_VERSION` 比较。
- [AGENTS.md](../AGENTS.md) 将用户日常应用固定在 `~/Documents/Vega/Vega.app`。更新逻辑应从运行中的 bundle 取得目标路径并验证 bundle ID，不要把单台机器的绝对路径写入产品逻辑。
- `reqwest`、Serde、`sha2` 已在 workspace 白名单中；引入 Sparkle、其他 Rust updater crate 或新的密码学依赖需要先走依赖审批。
- `vega_runtime` 保持 headless。更新检查与安装属于桌面应用层，不应加入 Runtime 的模型请求路径。

## 5. 建议的实现结构

### 5.1 版本源与检查

- 只请求 `https://api.github.com/repos/puzige/vega/releases/latest`，读取 stable release 的 `tag_name`、发行说明和 `Vega-macos-arm64.zip` 资产地址。
- 应用版本按 `v` + 三段式发布号与现行 app version 比较；暂不支持 prerelease 标签。若后续需要完整 SemVer 语义，再按执行宪法申请直接依赖。
- 当前版本取自运行中 bundle 的 `CFBundleShortVersionString`；候选版本取自 Release `tag_name` 去掉前缀 `v`。二者必须以同一规则解析，避免 release tag 覆盖了 Info.plist、binary 却一直报告 Cargo workspace 版本的错配。
- 请求不携带账号、机器 ID、会话 ID或工作区路径；只使用公开 Release API。自动检查建议启动后稍作延迟，之后最多每 24 小时一次，并为手动检查复用同一请求状态，避免并发重复下载。
- 从开发 checkout、非 `.app` bundle 或不受 Vega 控制的安装来源启动时，不尝试自替换；手动检查可打开官方 Release 页面。

### 5.2 状态与界面

更新服务建议使用以下可观察状态：

```text
Idle → Checking → UpToDate
                 → UpdateAvailable
UpdateAvailable → Downloading → ReadyToInstall
任意网络/校验/安装阶段 → Error（保留当前可运行版本）
ReadyToInstall → 用户选择重启 → Installing → Restarting
```

- 自动检查失败或离线时不弹窗、不打断会话；手动检查展示错误和重试入口。
- “稍后”只关闭提醒，不丢失已下载版本状态。
- 有 Agent 任务运行时禁用或延迟重启安装；不得为了更新强制停止任务。
- UI 入口放在 Vega 主菜单或设置页的 About/Updates 区域，并提供版本号、发行说明、检查时间和手动检查按钮。具体视觉按 `vega-design-guidelines.md` 冻结。

### 5.3 下载、验证与替换

1. 下载到应用数据目录下的专用 staging 子目录，限制响应体大小，使用临时文件名，下载中断时不触碰现有应用。
2. 解压时拒绝绝对路径、`..` 路径和不符合预期的目录结构；确认只有一个 `Vega.app`，bundle ID 为 `ai.vega`，版本与元数据一致。
3. 只有启用正式签名的 Release 才能进入安装阶段：验证 bundle 的 Developer ID 身份与当前应用预期 Team ID 相符，并检查代码签名完整性及 notarization。ad-hoc 签名不能证明发布者身份，必须 fail closed。
4. 用户选择“重启以安装”后，才允许替换当前运行 bundle。替换前先在同一卷暂存现有 `.app`，新包完整就位并通过验证后再切换；任何一步失败都恢复旧目录，且不删除 `~/Library/Application Support/ai.vega`。
5. 首次成功启动新版后再清理旧 bundle 与 staging 文件。安装权限不足时保留旧版本，提示通过官方 Release 页面手动升级；不提权、不调用 shell 拼接下载参数。

### 5.4 发布链路

- 先由负责人配置 Developer ID Application 证书与 notarization 凭据，再在 `.github/workflows/release.yml` 中启用签名、公证和 staple 步骤。
- 签名和公证必须发生在 zip 上传到 GitHub Release 之前；PR workflow 不持有发布凭据。
- Release 资产应保持 `Vega-macos-arm64.zip` 稳定命名，并附版本、说明和 SHA-256。下载端校验 SHA-256 与签名身份；摘要用于传输完整性，签名身份才用于发布者认证。
- 更新器的签名预期值必须由构建/配置确定，不从下载到的 manifest 自己读取并信任。

## 6. 验收矩阵

这些是后续实现需要先落地的用例。没有真实签名材料时，签名更新与安装验收标为 `NOT RUN`，不得用测试替身冒充真实签名验收。

| ID | 场景 | 预期 |
|---|---|---|
| UPD-01 | 从当前 bundle 读取 tag-stamped 版本；latest 等于或低于该版本 | 显示最新；不下载、不改 app bundle。 |
| UPD-02 | stable 版本高于当前版本 | 识别可用版本并显示版本号和发行说明；不把 prerelease 当作 stable。 |
| UPD-03 | 自动检查关闭 | 自动启动检查不发请求；用户手动检查仍可用。 |
| UPD-04 | 自动检查遇到离线、超时或 GitHub 限流 | 不打断任务；手动检查显示清晰错误和重试入口。 |
| UPD-05 | 响应格式错误、缺少 arm64 资产或版本标签非法 | fail closed；不下载、不修改安装目录。 |
| UPD-06 | 下载被截断、超限、摘要不符或 zip 路径穿越 | 拒绝解压/安装；旧 `.app` 保持可启动。 |
| UPD-07 | bundle ID、版本、Team ID、签名或 notarization 不匹配 | 拒绝安装，显示官方手动下载入口；绝不以 ad-hoc 验证结果放行。 |
| UPD-08 | 更新已暂存且用户选择重启安装 | 新版本仅在用户主动选择后安装；无运行任务被静默中断。 |
| UPD-09 | 目标目录不可写或替换过程失败 | 回滚到旧 `.app`；保留用户数据和可重试状态。 |
| UPD-10 | 从源码 checkout、非 app bundle 或其他路径启动 | 不执行自替换；不触碰当前执行文件与工作区。 |
| UPD-11 | 更新成功后检查应用身份和数据路径 | `CFBundleIdentifier` 仍为 `ai.vega`；配置、数据库和会话数据均可读取。 |

实现阶段只跑与本卡相关的定向测试。真实签名包的 Gatekeeper/首次启动验收由用户在合并后的 master 上手测。

## 7. 实施顺序

1. **决策与发布前置**：确认默认 UX；配置并验证 Developer ID、notarization 和受限 CI secrets。完成后才允许启用自替换功能。
2. **更新域与检查**：在 app 层实现 release 元数据、从运行 bundle 读取版本、版本比较、限频检查和状态机；为同一检查入口覆盖最新/非最新、错误响应、离线和限流。
3. **更新入口与提醒**：添加设置项、手动检查、无更新/有更新/错误反馈和稍后状态；覆盖自动检查关闭、重复请求和任务运行状态。
4. **安全下载与安装**：实现大小上限、归档路径校验、签名校验、staging、替换与回滚；使用临时 app bundle 验证失败路径。
5. **Release 工作流**：启用签名与公证，发布 arm64 zip 和摘要；验证 GitHub Release 的真实资产可被正式安装器消费。
6. **手测**：在固定日常安装路径验证检查、下载、稍后、用户主动重启、签名拒绝和数据保留。

## 8. 待产品负责人确认

| 决策 | 建议 | 阻塞内容 |
|---|---|---|
| 是否接受后台自动下载、由用户选择重启安装 | 接受；设置文案明确为“自动检查并下载”，并提供关闭选项。 | 决定默认开关与通知 UX。 |
| 是否采购/启用 Apple Developer Program 与 Developer ID | 是；不具备发布者签名时仅启用版本检查和官方 Release 链接。 | 阻塞应用内安全替换与静默安装能力。 |
| 是否保持仅支持 stable + macOS arm64 | 是；与当前唯一 Release 资产一致。 | 若需其他平台/渠道，需先扩展打包和 Release 资产规格。 |

## 9. 实施约束

- 本文是基于公开参考与当前仓库状态的规格建议，不代表真实代码、签名配置或安装验收已经完成。
- Issue #178 已在 GitHub Project 中为 `In progress`。本文完成调研与方案草案；实现范围及验收条件确认后，可在同一卡内继续实现，或在范围扩大时另开实现卡。代码实现按 Vega 交付流程委派给专用 subagent。
- 上游公开实现与产品策略会变化；实现开工前重新核实 Zed/Codex 当前行为及 Vega Release 工作流。
