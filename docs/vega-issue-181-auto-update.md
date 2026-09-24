# Issue #181：macOS 自动更新实现

来源：[方案 #178](vega-issue-178-auto-update.md)；[实现卡 #181](https://github.com/puzige/vega/issues/181)。2026-09-25 冻结。

## 契约

本卡采用 #178 §2、§4–6 的实现契约；其“建议/待确认”由本节取代。默认后台检查并下载，用户明确重启才安装；stable、macOS arm64。无正式签名的运行应用仅检查并打开官方发布页。签名配置是生产启用前置，不阻止实现受门禁保护的完整代码路径。

设置包含自动检查并下载开关（默认 true）、当前版本、状态、最后检查时间、发行说明、检查更新、发布页、稍后、重启安装。布局遵循设计指南，使用现有 token。自动检查延迟启动并持久化最近尝试时间，间隔 24h；手动检查绕过时间限制，但复用 single-flight。关闭自动选项不影响手动操作。开发实例明确显示开发版本，不以 Cargo 固定版本假装 bundle 版本。

网络仅公开 GitHub Release API 与该仓库资产；限定 HTTPS 与 GitHub 官方资产重定向域，限制元数据、下载、解压总大小及条目数量。版本只接受三段非负数字 stable，不接受 prerelease/draft。摘要文件固定 Vega-macos-arm64.zip.sha256；SHA-256 仅完整性验证。正式身份从已验证当前 app 的 Developer ID Team ID 取得；候选必须相同 Team ID、ai.vega、预期版本，并通过 codesign 与 Gatekeeper/公证检查。不能信任下载元数据提供的 Team ID。

归档解压拒绝绝对路径、父路径、链接逃逸和特殊文件；不得跟随归档 symlink 写出 staging。安装由独立、可信 helper 或同一受信任 binary helper 模式执行，等待旧进程退出；验证目标仍为原 bundle，使用同卷暂存与备份、失败回滚。helper 不使用 shell 插值，不提权；重启失败保留恢复证据/旧 app。仅新版本正常启动后清理自身 updater 备份。运行任务检查必须覆盖所有会话，安装动作不得有任务启动竞态。不可满足安全条件就 fail closed 并显示人工下载入口。

release workflow 支持正式签名、公证、staple、之后压缩与摘要；凭据全缺保持 ad-hoc 发布，部分配置必须报错。不打印 secrets。PR 不持有发布 secrets。本卡不配置或采购 Apple 凭据，不发布实际 tag，不替换用户日常应用。

## 允许依赖与实现边界

优先复用 workspace 的 reqwest/serde/serde_json/sha2/tokio/futures/tempfile/libc；新增外部 crate 先上报主 agent。桌面 updater 留在 app 层，跨 crate DTO 放 conversation::types，UI 不直接 SQLite。不改变 runtime headless。代码不加注释（unsafe SAFETY 例外），非测试不 unwrap/expect。

## 验收矩阵（先于实现）

| ID | 前置/操作 | 预期 | 层级与证据 | 状态 |
|---|---|---|---|---|
| UPD01 | bundle 版本等于/高于 release | 不下载，版本来自 plist | production metadata + parser 定向测试 | 待实现 |
| UPD02 | 较新 stable 元数据 | 可见版本、说明、状态 | controller/metadata 定向测试；UI 用户手测 | 待实现 |
| UPD03 | 关闭自动/重启/手动/重复点击 | 持久化、24h 限频、手动可用、single-flight | 设置与 controller 定向测试 | 待实现 |
| UPD04 | 离线、超时、限流、空响应 | 自动安静、手动错误可重试 | network boundary fault injection | 待实现 |
| UPD05 | 非法版本/缺资产/外部地址 | 不下载不改 app | production metadata 定向测试 | 待实现 |
| UPD06 | 截断/大小/摘要/路径/链接异常 | 拒绝并保留旧 app | owned temp filesystem + parser 测试 | 待实现 |
| UPD07 | 身份/版本/Team ID/签名/公证错 | fail closed | 真实 ad-hoc 拒绝；签名失败边界测试 | 待实现 |
| UPD08 | 已下载、稍后、显式安装、有活动任务 | 状态保留；无任务才重启；无静默中断 | controller 定向测试；用户手测 | 待实现 |
| UPD09 | 目标不可写/替换或启动失败 | 回滚/保留旧包、错误可解释 | owned temp filesystem + fault injection | 待实现 |
| UPD10 | 非 bundle/开发环境 | 禁止自替换，官方页入口 | production discovery 定向测试 | 待实现 |
| UPD11 | 正常替换/启动清理 | ai.vega、数据保留、仅清理自有备份 | owned temp filesystem；真实签名用户手测 | 待实现 |

## 实现计划

1. 专用实现 agent 阅读仓库规范、设计指南，落地 updater service/测试（先实现可证伪的关键用例）。
2. 接入 app 生命周期、设置和 action；补发布与打包签名路径及文档。
3. 只运行本卡定向测试，原始日志留临时目录，提交证据记录；不跑本地全 workspace 测试。
4. 主 agent 审查，PR 云端 check 绿后 squash 合并，卡置 In review 等用户手测。

## 兼容与回滚

只增设置，旧配置默认 true；不更改会话数据库 schema。代码回滚用后续 PR revert；应用失败恢复旧 bundle，不改用户配置/数据。真实 Developer ID 成功安装在凭据未配置时标 NOT RUN，不将替身测试称真实验收。

## 架构裁决记录

- 2026-09-25：批准 `zip = "=8.6.0"`，关闭默认 features，只启用 deflate。使用维护中的 ZIP reader 逐项有界解压，避免自写 ZIP parser。依据：[官方 API](https://docs.rs/zip/8.6.0/zip/read/struct.ZipArchive.html)。不能仅依赖 enclosed_name：额外拒绝原始父路径组件、所有符号链接与特殊文件，限制实际解压字节。已存在 workspace 依赖继续复用。
- 实现与验收分离：当前会话更高优先级约束禁止在用户未明确要求测试/验证时新增或运行测试。因此保留上述验收矩阵作为待验收清单，本轮不新增/执行测试，不将静态审查或编译报告成测试通过。既有云端 PR check 保持原样。
- 安装启动超时裁决：若新版进程已经退出或无法启动，恢复旧版；若新版仍存活但未及时确认启动，不强杀，保留备份与安装记录，交由用户恢复。启动确认前不清理旧包。目录重命名能在普通失败时回滚，但不宣称两个目录切换具有跨断电原子性；断电/强杀后的自有备份位置必须在交付文档说明。

- 安装路径按仓库约定限制为当前用户 `~/Documents/Vega/Vega.app`；其他位置只检查与打开发布页。稍后仅保证当前进程内保留下载；未安装 staging 在退出/崩溃后可能残留，本卡不自动扫描历史目录，以免删除安装恢复证据，列入交付限制。
