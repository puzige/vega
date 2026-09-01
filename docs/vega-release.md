# Vega 发版指南（tag → GitHub Release）

打一个 `v*` tag，GitHub Actions 自动构建 macOS 安装包并挂到 Release。
流水线定义：[.github/workflows/release.yml](../.github/workflows/release.yml)。

## 发版三步（主人视角）

1. **确认 master 可发**：master 上本地门禁通过（`cargo fmt --all -- --check`
   / `cargo clippy --all-targets -- -D warnings` / `cargo test --workspace`）。
2. **打 tag 并推送**：
   ```sh
   git tag v0.1.0 && git push origin v0.1.0
   ```
3. **等流水线跑完**：Actions → `release`（macos-14/arm64，`cargo xtask
   package` 构建 dist/Vega-macos-arm64.zip，版本号 = tag 去掉 `v`）→
   GitHub Releases 自动出现 v0.1.0，附按提交自动生成的 notes 与 zip。

## 成本提示（重要）

- 仓库是 **private**：macOS runner 按 Linux 分钟 **10x 计费**。一次发版
  冷缓存构建约 25-45 分钟 ≈ 250-450 计费分钟（配额随 GitHub 计划）。
- 流水线只在 `v*` tag push 与手动触发时运行，**不在 push/PR 上跑**；
  单 job、超时 60 分钟上限；tag 构建内不跑全量测试（测试由 PR 门禁承担，
  见 workflow 头注释的取舍说明）。
- rust-cache 按 tag 隔离（`key: v-<tag>`）：手动 re-run 命中缓存很快，
  新 tag 每次冷构建。

## 签名与公证（HUMAN 前置）

当前产物为 **ad-hoc 签名**：目标 Mac 首次启动需右键打开或
`xattr -cr`（zip 内 INSTALL.txt 有说明）。要彻底消除 Gatekeeper 提示：

1. 主人前置：Apple Developer 账号（$99/年）+ 导出 Developer ID
   Application 证书 `.p12`（本仓库不含任何凭据）；
2. 在仓库 Settings → Secrets 添加：
   `APPLE_CERTIFICATE_P12`（.p12 的 base64）、`APPLE_CERTIFICATE_PASSWORD`、
   `KEYCHAIN_PASSWORD`、`APPLE_ID`、`APPLE_TEAM_ID`、
   `APPLE_APP_SPECIFIC_PASSWORD`（appleid.apple.com 生成）；
3. 解开 workflow 中「Signing / notarization (HUMAN PENDING)」注释块
   （步骤模板同 [vega-packaging.md](vega-packaging.md) 末节）。

## 失败怎么办

- **构建失败**：修复后直接重跑——Actions 页面对该 run 点
  「Re-run jobs」（命中 tag 缓存，很快）；
- **Release 已建但资产缺失**：`gh release upload <tag> dist/Vega-macos-arm64.zip`
  本地补传，或 `workflow_dispatch` 手动触发流水线验证构建后再补；
- **误打了 tag**：`git push origin :refs/tags/vX.Y.Z` 删除远端 tag 并在
  Releases 页删除对应 release，修好重新打 tag（concurrency group 相同，
  重推 tag 会自动取消进行中的旧 run）。

## 首次运行验证点

`cargo xtask package` 依赖 `qlmanage`（WebKit 栅格化图标）。CI runner 无
桌面会话，理论可跑（QuickLook satellite 进程），但首次 tag 发布时留意该
步骤日志；若失败可临时降级为 PNG→icns 路径（见
[vega-packaging.md](vega-packaging.md) §2 的图标链路说明）。
