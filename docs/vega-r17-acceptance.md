# R17 — 蓝色单星笑脸 Logo 主 Agent 验收

## Freeze

2026-09-08。按用户定稿，由 Luna max 实现，root 审查并集成本地分支。规格见 [R17](vega-r17-smile-logo-production.md)，实现与源文件哈希见 [交付报告](vega-r17-logo-delivery.md)。正式图形为连接终端 `>`、单颗修长四角星和微笑光标；浅深版同几何，生产 ICNS 使用浅色。历史探索保留。

验证实现提交为 `3edb3ac`；验证期间只有 R16/current-status 两份 root 状态文档未提交，代码及资源与该提交一致。环境为 macOS 15.7.9 arm64；完整命令与日志保存于外部验收目录 `native-r17/`，下表 `$R17_EVIDENCE` 指该目录。

## Results

| exact command | result | duration | raw log / SHA256 |
| --- | --- | --- | --- |
| `cargo fmt --all -- --check` | PASS | 1.23s | `fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --all-targets -- -D warnings` | PASS | 54.75s | `clippy.log` / `f7aea12810bb749cf96582dabfc58b73e2286aa4cc38b82d9f63b1559e0434b7` |
| `cargo test --workspace` | PASS | 155.63s | `test.log` / `a79f32e8a3bc1bc568781cbe4319a8fbfad51b8160ce0ca2a5b6e8f8751fbfa1` |
| `cargo build --workspace` | PASS | 20.16s | `build.log` / `253abd5f8e62d07e1de79e9b01771b06fb93a8c59cdeaaaa18393ae3155ab625` |
| `cargo xtask package-icon "$R17_EVIDENCE/Vega.icns"` | PASS | 2.42s | `icon.log` / `ea7eedc2f9df62d0a0e92ddc50622a9276e6df3afaa42eb0e22d8ed526681753` |

完整 workspace 测试：**988 passed / 0 failed / 0 ignored**。全部首次通过；strict lint 只有既有依赖 `block v0.1.6` future-incompatibility 提示。无新增依赖，生产改动仅图标源引用及注释，既有测试未修改。

主 Agent 从集成源通过真实 `cargo xtask package-icon` 导出，ICNS SHA256 为 `1446a2b79dff877c95e26eb7c08fb370f15e47bd5dbdcc866dbab680dd0ad685`，与 Luna 两次导出字节一致。使用 `iconutil -c iconset` 解码十档图标，AppKit 逐像素检查全部外边缘 alpha=0；1024px 半透明阈值主体边界为96…927，保留832底板，其他尺寸对称缩放。原始检查在 `alpha.log`，真实解包 PNG 在 `Vega.iconset/`。

## 原生验收

- 先备份已安装应用到 `native-r17/rollback-installed/Vega.app`，对已安装应用与当前验收副本替换 ICNS 并重新 ad-hoc 签名。两份应用严格签名校验均通过；各自 Mach-O UUID 与 `__TEXT,__text` SHA256 前后不变。保留各自功能版本，详见 `bundles-before.json`、`applied-icons.json`。
- 首次仅更新资源并注册时系统缓存未变；更新 bundle 修改时间并只对安装副本 `lsregister -f` 后，系统自行刷新。没有重启整个 Dock 或改写 Launchpad 数据库。
- Finder 信息窗口实际截图已目视确认蓝色笑脸；`dock-after-detail.png` 为原生屏幕区域截图，确认与相邻 ZCode 图标尺寸协调。不是设计稿或解包图冒充实机。
- Launchpad 采用真实系统缓存 PNG 验证：`launchpad-after-big.png` 已为蓝色笑脸；`system-after.json` 记录176条布局及逐行哈希与基线完全一致，Vega仍只有原item186。Dock tilesize68、largesize128、magnification1及autohide状态均未变。
- 已安装旧功能版先完成窗口启动检查，随后正常退出；当前运行单实例为 `native-r17/Vega.app`，复用R14已验收功能代码和自有测试配置，当前主界面经 CUA 实际截图确认正常。未发供应商请求。

## 清理与 Residuals

`Workspace` 与 `Workspace/worktrees` 共移除42个闲置旧副本，原目录分配空间合计约1.37GiB（不是APFS精确空间增量），所有分支头保留；修复12处旧账号路径链接。32个已整合或补丁等价，10个为已交付/被集成版本替代的 Vega 执行树，原提交仍可恢复。8个未整合或原仓库缺失副本保留，主仓库及当前集成树保留。新建的本轮 Luna 执行树也在集成后另行移除，分支保留，不计入42个旧副本。恢复说明见外部 `cleanup-20260908/README.md`。

- ACCEPTED：受控矢量解释，非生成参考图自动像素描摹；高光受 AppKit 支持范围约束。
- LIMIT：Dock AX 读取超时，使用实际区域截图验收；Launchpad 本轮为系统缓存及布局验证，未把它写成 Launchpad 窗口截图验收。
- NOT RUN：跨 macOS 版本、真实供应商请求、性能专项。性能测试继续按用户要求降级。
- 未 push、合并 master 或发布。规格偏离：无。
