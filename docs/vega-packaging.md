# Vega macOS 打包与分发（cargo xtask package）

本文覆盖 Vega 桌面端在 macOS 上的构建、打包（.app bundle）与分发。
打包入口是 xtask 的 `package` 子命令（零新增第三方依赖，全部使用 macOS
自带工具：swift/AppKit / sips / iconutil / codesign / zip / shasum）。

## 1. 构建

前置要求与 GPUI 日常构建一致（见 [README](../README.md#前置要求macos)）：

- 完整 Xcode（Metal 着色器编译需要 `metal` 工具）；
- Rust（rustup），进入仓库后自动使用 `rust-toolchain.toml` 锁定工具链。

`cargo xtask package` 每次都会先无条件重建 release 二进制（与 bench 共用
`xtask/src/provenance.rs::rebuild_release`），无需手动 `cargo build --release`。
产物：`target/release/vega`（arm64）。

## 2. 打包

```sh
cargo xtask package
```

一条命令完成（`xtask/src/package.rs`）：

1. release 构建；
2. 图标：以 `assets/logo/vega-icon-r17-smile-light.svg`（R17 蓝色单星笑脸浅色主标）为源，经
   `swift`/AppKit 画到透明 1024px RGBA 位图 → `sips` 生成
   Apple 标准 iconset（16/32/128/256/512 + @2x，至 1024px）→
   `iconutil -c icns` 得到 `Resources/Vega.icns`。
   **为什么不直接用 raster PNG**：`raster/vega-icon-f*-original.png` 右下角
   带 AI 生成水印（LOGO.md 明示 SVG 矢量版为定稿）；历史 F1/F3 变体与
   R16 探索稿继续归档——`.icns` 无自动明暗外观切换机制，因此使用 R17
   浅色主标作为确定性生产源。
3. 组装 `dist/Vega.app`：

   ```
   Vega.app/
   └── Contents/
       ├── Info.plist          # 12 项元数据
       ├── PkgInfo             # APPL????
       ├── MacOS/vega          # 可执行（0755）
       ├── Resources/Vega.icns # 图标
       └── _CodeSignature/     # ad-hoc 签名
   ```

4. 写 `Info.plist`。**`CFBundleIdentifier = ai.vega` 是兼容性红线**：它就是
   macOS 数据根 `~/Library/Application Support/ai.vega` 的命名空间（权威定义见
   `crates/vega_store/src/paths.rs`）；改动会让本机已有 dogfood 数据全部
   孤儿化。其余键：CFBundleName/DisplayName=Vega、
   CFBundleExecutable=vega、CFBundleVersion 与
   CFBundleShortVersionString=发布版本（优先 `--version`，其次 `VEGA_RELEASE_VERSION`，默认 workspace 版本）、CFBundleIconFile=Vega、
   CFBundlePackageType=APPL、LSMinimumSystemVersion=11.0（arm64 构建，
   Apple Silicon 起点）、NSHighResolutionCapable、
   LSApplicationCategoryType=public.app-category.developer-tools。
5. ad-hoc 签名并自检：`codesign --force --deep --sign -` +
   `codesign --verify --strict` + `plutil -lint`（任一失败即打包失败）；
6. 产出 `dist/Vega-macos-arm64.zip`（Vega.app + INSTALL.txt）以及
   `dist/Vega-macos-arm64.zip.sha256`。本地 package 默认保持 ad-hoc；正式签名由 release workflow 完成。

`dist/` 中的产物全部不入库（.gitignore）；icns 可由入库的 SVG 确定性
重建，故 iconset/icns 均不备库。单测覆盖 Info.plist 必备键与 iconset
尺寸表（`cargo test -p xtask`）。

## 3.1 Git 运行时要求

Vega 的 workspace、branch 和 trusted commit 操作只使用固定的 canonical
Git 来源：Apple Silicon 按顺序检查
`/opt/homebrew/opt/git/bin/git` 与 `/usr/bin/git`，Intel Mac 按顺序检查
`/usr/local/opt/git/bin/git` 与 `/usr/bin/git`。解析成功后，进程会固定该
来源并在每次启动子进程前复核文件身份、canonical 路径和权限；`PATH`、仓库
配置以及任意用户指定的 executable 不参与选择。

运行时需要 Git **2.40 或更新版本**（用于安全的 `check-attr --source`
查询）。如果 Vega 显示 Git 不可用或版本过旧，请在对应 Mac 上安装或升级
Homebrew Git，例如执行 `brew install git`，然后重启 Vega。Vega 不会自动
联网安装、替换 `/usr/bin/git` 或修改用户仓库。

## 3. 分发（其他 Mac 安装）

把 `dist/Vega-macos-arm64.zip` 和同版 `.sha256` 下载到目标 Mac，要求
macOS 11.0+ Apple Silicon。摘要仅验证完整性，不替代发布者签名验证。

1. 确认旧 Vega 中没有运行任务并正常退出；
2. 解压，将 `Vega.app` 放到 `~/Documents/Vega/Vega.app`；
3. 从该明确路径打开。更新只替换 app，保留 `ai.vega` 数据根与配置。

`dist/Vega.app` 仅是打包产物，不作为日常入口；不安装到 Applications，
不主动注册 Launchpad 或刷新 Dock。需要备份时用 zip 保存构建身份，避免散落 app 副本。

本地包与缺少全部发布凭据的 Release 为 ad-hoc 签名，禁止自动替换自身。
首次从 ad-hoc 升级到 Developer ID 正式签名包必须人工安装；之后才可使用自动安装。
正式签名包应正常通过系统验证；如被阻止，应核对来源与签名，不移除安全属性。
zip 内附相同安装入口说明的 `INSTALL.txt`。

## 4. Developer ID 签名与公证

完整配置 [发布指南](vega-release.md#签名与公证) 列出的六项 secrets 后，
release workflow 自动完成临时钥匙串导入、Developer ID Team ID 校验、
hardened runtime + timestamp 签名、公证 Accepted 检查、staple、Gatekeeper 检查。
随后重新打包、解压最终 zip 验证 ticket 与签名，并更新 SHA-256 sidecar。
部分 secrets 配置会失败，不回退到 ad-hoc。

归档保持普通 zip 的 `Vega.app/` 与 `INSTALL.txt` 布局，没有 `__MACOSX`
资源叉条目。staple 必须在最终压缩之前完成；发布流水线验证压缩后的 ticket。
不手动重签或修改已发布包；需要变更时发布新版本。
