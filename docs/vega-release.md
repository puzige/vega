# Vega 发版指南（tag → GitHub Release）

打一个 `vMAJOR.MINOR.PATCH` stable tag，GitHub Actions 自动构建 macOS 安装包并挂到 Release。
流水线定义：[.github/workflows/release.yml](../.github/workflows/release.yml)。

## 发版三步（主人视角）

1. **确认 master 可发**：合并的 PR 已通过云端 `check`（`cargo fmt --all --
   --check` / `cargo clippy --workspace --all-targets -- -D warnings` /
   `cargo test --workspace --no-fail-fast -- --test-threads=1`（含单元、集成和文档测试，单 job、无分片），见 [.github/workflows/pr-check.yml](../.github/workflows/pr-check.yml)）。
2. **打 tag 并推送**：
   ```sh
   git tag v0.1.0 && git push origin v0.1.0
   ```
3. **等流水线跑完**：Actions → `release`（macos-latest/arm64，`cargo xtask
   package` 构建 dist/Vega-macos-arm64.zip，版本号 = tag 去掉 `v`）→
   GitHub Releases 自动出现 v0.1.0，附按提交自动生成的 notes、zip 与 SHA-256 摘要。

## 成本提示

- 仓库是 **public**（2026-09-22 核实）：GitHub 托管 runner（含 `macos-latest`）
  免费、不计分钟配额。
- 发布流水线只在 `v*` tag push 与手动触发时运行；单 job、超时 60 分钟上限；
  tag 构建内不跑全量测试（测试由 PR check 承担，见 [pr-check.yml](../.github/workflows/pr-check.yml)）。
- rust-cache 按 tag 隔离（`key: v-<tag>`）：手动 re-run 命中缓存很快，
  新 tag 每次冷构建。
- 每次 PR merge 后 master 会跑 [cicd.yml](../.github/workflows/cicd.yml)（GitHub Actions 名称 `master`）
  的 `build` job 打包上传 artifact，但**不发 Release**；发布仍只由 tag 触发。

## 签名与公证

[自动更新实现规格](vega-issue-181-auto-update.md) 对正式更新要求 Developer ID
Application 签名、公证和 staple。workflow 已实现该路径；只需仓库管理员配置凭据，
无需再编辑流水线。

在仓库 Settings → Secrets 配置全部六项：

| Secret | 内容 |
|---|---|
| `APPLE_CERTIFICATE_P12` | Developer ID Application 证书与私钥导出的 `.p12`，base64 编码 |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` 密码 |
| `KEYCHAIN_PASSWORD` | CI 临时钥匙串密码 |
| `APPLE_ID` | 公证 Apple 账号 |
| `APPLE_TEAM_ID` | 十位签名 Team ID |
| `APPLE_APP_SPECIFIC_PASSWORD` | 公证账号的 App 专用密码 |

六项全部为空时继续产出 ad-hoc 包，仅支持检查更新与人工下载；部分配置会立即失败，
不允许签名配置失误退回 ad-hoc。凭据只注入 release job 的对应步骤，不注入 PR。

完整配置时，临时钥匙串仅导入该证书，选择配置团队唯一的 Developer ID Application
身份，使用 hardened runtime 和安全时间戳签名，校验 Team ID 与 `ai.vega`，提交
公证并确认 `Accepted`，staple 后重打 zip。再从最终 zip 解压验证 ticket 与签名，
最后计算 `Vega-macos-arm64.zip.sha256`。任何步骤失败均不上传 Release；退出时清理
临时证书与钥匙串。摘要防止传输损坏；更新信任以当前正式 app 的已验证签名团队为准。

归档只包含 `Vega.app/` 与 `INSTALL.txt`，使用 `zip -r -q -X`，不生成
`__MACOSX` 资源叉目录。最终归档的 stapler 校验保证 ticket 没有在重打包时丢失。

安装到 `~/Documents/Vega/Vega.app`；首次从 ad-hoc 迁移到正式包需人工安装。
正式包不要求移除 quarantine 或绕过 Gatekeeper。若系统拒绝，应检查发布签名与公证。

## 发布限制

仅接受三段数字 stable tag；各段不允许前导零（单独 `0` 除外），且不超过
`18446744073709551615`，与应用的版本解析契约一致。手动触发必须选择已有 tag，不能把分支名当版本。
本卡没有配置凭据或发布实际 tag；正式签名、公证及真实更新仍需具备凭据后的发版验收。

## 失败怎么办

- **构建失败**：修复后直接重跑——Actions 页面对该 run 点
  「Re-run jobs」（命中 tag 缓存，很快）；
- **Release 已建但资产缺失**：`gh release upload <tag> dist/Vega-macos-arm64.zip dist/Vega-macos-arm64.zip.sha256`
  本地补传，或 `workflow_dispatch` 手动触发流水线验证构建后再补；
- **误打了 tag**：`git push origin :refs/tags/vX.Y.Z` 删除远端 tag 并在
  Releases 页删除对应 release，修好重新打 tag（concurrency group 相同，
  重推 tag 会自动取消进行中的旧 run）。

## 首次运行验证点

`cargo xtask package` 依赖 macOS `swift`/AppKit（SVG 栅格化图标）。CI runner
无桌面会话，但 AppKit 位图渲染不依赖 QuickLook satellite；首次 tag 发布时
仍应留意该步骤日志（见
[vega-packaging.md](vega-packaging.md) §2 的图标链路说明）。
