# Vega macOS 打包与分发（cargo xtask package）

本文覆盖 Vega 桌面端在 macOS 上的构建、打包（.app bundle）与分发。
打包入口是 xtask 的 `package` 子命令（零新增第三方依赖，全部使用 macOS
自带工具：qlmanage / sips / iconutil / codesign / zip）。

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
2. 图标：以 `assets/logo/vega-icon-f1-light.svg`（F1 浅色主标）为源，经
   `qlmanage -t -s 1024`（WebKit 栅格化，squircle 外透明）→ `sips` 生成
   Apple 标准 iconset（16/32/128/256/512 + @2x，至 1024px）→
   `iconutil -c icns` 得到 `Resources/Vega.icns`。
   **为什么不直接用 raster PNG**：`raster/vega-icon-f*-original.png` 右下角
   带 AI 生成水印（LOGO.md 明示 SVG 矢量版为定稿）；F3 深色变体留作营销
   素材——`.icns` 无自动明暗外观切换机制，F1 白色 squircle 在明暗 Dock
   均适用（LOGO.md 的经典扁平第三方图标风格决策）。
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
   macOS 数据根 `~/Library/Application Support/ai.vega` 与 Keychain 服务
   `ai.vega` 的命名空间（权威定义见
   `crates/vega_store/src/paths.rs`）；改动会让本机已有 dogfood 数据全部
   孤儿化。其余键：CFBundleName/DisplayName=Vega、
   CFBundleExecutable=vega、CFBundleVersion 与
   CFBundleShortVersionString=workspace 版本（0.1.0）、CFBundleIconFile=Vega、
   CFBundlePackageType=APPL、LSMinimumSystemVersion=11.0（arm64 构建，
   Apple Silicon 起点）、NSHighResolutionCapable、
   LSApplicationCategoryType=public.app-category.developer-tools。
5. ad-hoc 签名并自检：`codesign --force --deep --sign -` +
   `codesign --verify --strict` + `plutil -lint`（任一失败即打包失败）；
6. 产出 `dist/Vega-macos-arm64.zip`（Vega.app + INSTALL.txt）。

`dist/` 中的产物全部不入库（.gitignore）；icns 可由入库的 SVG 确定性
重建，故 iconset/icns 均不备库。单测覆盖 Info.plist 必备键与 iconset
尺寸表（`cargo test -p xtask`）。

## 3. 分发（其他 Mac 安装）

把 `dist/Vega-macos-arm64.zip`（约 9 MB）拷到目标 Mac（AirDrop/微信/U 盘
均可），要求 macOS 11.0+ Apple Silicon：

1. 解压，将 `Vega.app` 拖入「应用程序」（/Applications）；
2. 首次启动会遇 Gatekeeper（「无法打开，因为无法验证开发者」），任选其一：
   - 在 /Applications 中**右键点 Vega.app → 打开 → 再点「打开」**；或
   - 终端执行 `xattr -cr /Applications/Vega.app`；
3. 之后可正常双击启动，图标、名称、元数据齐全。

zip 内附同样说明的 `INSTALL.txt`。

## HUMAN PENDING：Developer ID 签名 + 公证（notarization）

ad-hoc 签名在其他 Mac 上需要上面的 Gatekeeper 放行步骤。要彻底消除提示，
需要主人的 Apple Developer 账号（$99/年）与 Developer ID Application
证书（本仓库不含任何凭据）。步骤模板：

```sh
# 1. 导入 .p12 证书（钥匙串访问导出的 Developer ID Application 证书）
security import DeveloperID_Application.p12 -k ~/Library/Keychains/login.keychain-db

# 2. 以 Developer ID 重新签名（hardened runtime + 时间戳）
codesign --force --deep --options runtime --timestamp \
  --sign "Developer ID Application: <团队名> (<TEAM_ID>)" dist/Vega.app

# 3. 打 zip（ditto 保留扩展属性）
ditto -c -k --keepParent --sequesterRsrc dist/Vega.app Vega-macos-arm64.zip

# 4. 提交公证（APP 专用密码在 appleid.apple.com 生成，勿入库）
xcrun notarytool submit Vega-macos-arm64.zip \
  --apple-id <APPLE_ID> --team-id <TEAM_ID> --password <APP_SPECIFIC_PASSWORD> --wait

# 5. 回签 staple，离线校验也通过
xcrun stapler staple dist/Vega.app
codesign --verify --strict --verbose=2 dist/Vega.app
```

公证通过后分发的 zip 在目标 Mac 上双击即启，无 Gatekeeper 提示。
