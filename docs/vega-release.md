# Vega 发版指南（master 自动 patch → GitHub Release）

每次 PR 合入 master 后，[master workflow](../.github/workflows/cicd.yml) 固定事件 commit SHA，
直接调用 [共用发布 workflow](../.github/workflows/release.yml)，自动选择 patch 版本、
打包、可选 Apple 签名公证、独立 Ed25519 签名并发布四项资产。人工 `vMAJOR.MINOR.PATCH` tag 与手动重跑也走同一实现。
版本与幂等处理见 [scripts/release.py](../scripts/release.py)。

## 发版流程

1. PR 通过云端 required check 并合并 master；不直接 push master。
2. Actions → master → publish merged master 分配版本并构建；不再重复独立 master 打包。
3. 等正式 Release 可见后，应用更新器才可获取该版本。Apple 凭据可选；独立更新签名必需，ad-hoc 包也可在内置公钥的新应用中更新。

GITHUB_TOKEN 创建 tag 不会再次触发 tag workflow，所以 master 直接调用 reusable workflow，
不依赖第二次事件。发布 job 持有 `contents: write`，PR 检查仍只读且不持有发布凭据。

## 缓存与门禁

共用 `vega-master-build` Cargo 缓存；仅 master 自动发布写入缓存，手动 tag 发布只恢复。
PR required check 与 nextest 保持原样；发布不重复跑测试。job 超时 60 分钟，包含签名与公证。

## 独立更新签名（必需）

[独立签名规格](vega-issue-181-independent-update-signing.md) 规定仓库公钥
`assets/update-public-key.hex` 编译进应用。管理员配置 `VEGA_UPDATE_PRIVATE_KEY` secret，
内容为 Ed25519 PKCS8 DER 的 base64。私钥只注入最终 ZIP 完成后的签名步骤；
不写仓库、命令参数或日志，不给 PR 构建使用。密钥丢失或轮换必须单独规划客户端信任迁移。

`cargo xtask sign-update --version <MAJOR.MINOR.PATCH>` 从环境读取私钥，先核对导出的
公钥与仓库 pin，再计算 ZIP 精确长度与 SHA-256，生成 `Vega-update.json` 和
`Vega-update.json.sig`。签名消息为 `Vega update manifest v1\n` 的字节前缀加原始 JSON，
签名文件为 base64 Ed25519 签名。缺 secret、错误 key、非法版本或超大 ZIP 会阻止发布。

新 draft 必须含 ZIP、`.sha256`、manifest 与 `.sig` 四项资产才可公开。
历史 tag 树未包含 `assets/update-public-key.hex` 时，两项资产即可判为完整；
带公钥的新 tag 即使已经发布，也必须四项完整才能幂等结束。历史同 SHA 重跑只读结束，
不补写或覆盖正式资产。

## 签名与公证（Apple 可选）

Developer ID Application 签名、公证和 staple 是额外系统身份保障；
已有正式 Apple 身份的客户端还会要求候选保持同 Team ID 与公证。

在仓库 Settings → Secrets 配置全部六项：

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE_P12` | Developer ID Application 证书与私钥导出的 `.p12`，base64 编码 |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` 密码 |
| `KEYCHAIN_PASSWORD` | CI 临时钥匙串密码 |
| `APPLE_ID` | 公证 Apple 账号 |
| `APPLE_TEAM_ID` | 十位签名 Team ID |
| `APPLE_APP_SPECIFIC_PASSWORD` | 公证账号的 App 专用密码 |

六项全部为空时产出 ad-hoc 包，仍必须独立签名更新 manifest；部分配置会立即失败，
不允许签名配置失误退回 ad-hoc。凭据只注入 release job 的对应步骤，不注入 PR。

完整配置时，临时钥匙串仅导入该证书，选择配置团队唯一的 Developer ID Application
身份，使用 hardened runtime 和安全时间戳签名，校验 Team ID 与 `ai.vega`，提交
公证并确认 `Accepted`，staple 后重打 zip。再从最终 zip 解压验证 ticket 与签名，
最后计算 `Vega-macos-arm64.zip.sha256`。任何步骤失败均不上传 Release；退出时清理
临时证书与钥匙串。独立 manifest 签名认证发布者及 ZIP；当前正式 app 的 Apple 签名团队作为额外约束。

归档只包含 `Vega.app/` 与 `INSTALL.txt`，使用 `zip -r -q -X`，不生成
`__MACOSX` 资源叉目录。最终归档的 stapler 校验保证 ticket 没有在重打包时丢失。

支持 `/Applications/Vega.app`、`~/Applications/Vega.app` 与兼容的
`~/Documents/Vega/Vega.app`，只更新实际运行副本。旧 0.1.1/0.1.2 尚无公钥更新逻辑，
必须先手动安装含公钥的新版本；不能远程为旧 binary 添加更新能力。
正式包不要求移除 quarantine 或绕过 Gatekeeper。若系统拒绝，应检查发布签名与公证。

## 重跑与失败恢复

- 版本先保留为指向本次事件 SHA 的 tag；构建失败后在 Actions 重跑相同任务，复用该版本。
- 发布先建立 draft，四项资产均上传成功才转正式并设为 latest。失败的 draft 不会成为更新源。
- 同 SHA 已有完整正式 Release 时直接结束，不重新构建、不再 bump、不重写正式资产。
- draft 上传失败可重跑，仅允许替换仍为 draft 的资产；上传前后均复核 draft 状态。正式版本缺资产（包括历史版本没有 SHA-256 sidecar）则报错，既不再 bump，也禁止用重跑覆盖；管理员应调查后以新版本修复。
- 若较新代码已发布，旧 SHA 的未完成发布会被拒绝；同 SHA 已完整发布的重跑仍是无副作用完成。
- 验证所有已发布 stable 版本的代码祖先关系与版本大小，避免乱序排队把 latest 倒退。已发布 tag 丢失或指向不可解析提交时失败。
- 权限不足、部分签名凭据、非法版本、patch 溢出都直接失败，保留排查证据；不自动删 tag、改 tag 或跳过门禁。

## 发布队列与限制

共享发布 job 使用 `vega-stable-publication` 并发组，`queue: max`、不取消正在运行的发布。
master 调用方没有同组锁，避免 reusable workflow 等待自身。GitHub 最多保留 100 个 pending
任务，超额会取消；FIFO 按进入等待队列时间，不保证事件时间顺序，代码祖先检查负责拒绝倒序。
参见 [GitHub concurrency 文档](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency)。

版本必须三段数字，各段不允许前导零（单独 `0` 除外），不超过 `18446744073709551615`。
首次无 stable tag 使用 workspace version；后续对已有 stable tag 最大版本 patch +1。
不会修改或回推 Cargo.toml；bundle 的版本由此次 tag 注入。一个 SHA 若已对应多个 stable tag 则失败，交给管理员消除歧义。
人工 tag 也必须指向 master 历史中的 commit，并遵循相同资产与逆序保护。手动 workflow dispatch
必须选择 tag。不得将失败运行改为另一个 SHA 重跑。

真实发版由合并后的 workflow 执行；更新签名私钥 secret 由管理员安全配置，不写入仓库或日志。

## 首次运行验证点

`cargo xtask package` 依赖 macOS `swift`/AppKit（SVG 栅格化图标）。CI runner
无桌面会话，但 AppKit 位图渲染不依赖 QuickLook satellite；首次 tag 发布时
仍应留意该步骤日志（见
[vega-packaging.md](vega-packaging.md) §2 的图标链路说明）。
