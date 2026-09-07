# R15 — Dock 与 Launchpad 图标主 Agent 验收

2026-09-06。用户指出 macOS Dock 和 Launchpad 中 Vega 图标偏大。本轮修复图标透明背景与留白，保留原白底祖母绿终端/星芒设计。图标生产代码 `9b8bb5e`；[规格](vega-r15-macos-icon.md)、[实现证据](vega-r15-icon-delivery.md)记录 Astra medium 实现和主 Agent 独立验证。

## 原因与实际修复

旧 SVG 底板为896/1024，Quick Look 缩略图导出又将外围变成不透明白底。主 Agent 从实际 R14 bundle 解包32/128/1024表示，全部像素 alpha=1。Dock 区域截图中 Vega 显示为撑满68px格子的白方块，旁边标准应用主体约56px。

生产打包现在由 AppKit 直接读取 SVG 并绘制到 RGBA bitmap，随后仍通过 sips/iconutil 输出全尺寸 icns；整体图形绕中心等比缩小13/14。1024画布的实际底板为96…927，即832px，与实测本机 ZCode 主体范围一致。10个表示均有透明边缘和抗锯齿，32主体3…28、128主体12…115；没有通过白色抠图破坏底板，也没有重新生成品牌图案。

`cargo xtask package-icon <output.icns>` 与完整 `package` 共用同一个生产 `render_icon`。主 Agent 从集成分支实际导出用时3.787s，ICNS SHA256为 `a64c929c36b0ef19994963dd02d289944717e62dff87d1e67c9c97954af07cd6`，与 executor 两次独立导出字节一致。未来打包也会使用这个修复。

## 两个系统入口

- 先退出旧 Vega 并核实0实例；目前唯一运行副本为外部 `native-r15/Vega.app`。应用功能二进制仍为R14验收代码，R15仅更新图标并重新签名。
- 通过 Launchpad 的现存 Vega bookmark 只读解析，确认实际入口是 `/Applications/Vega.app`。先完整备份，再更新该 bundle 的 `Vega.icns` 并重新 ad-hoc 签名；没有替换其功能代码。两副本重新签名前后的 Mach-O UUID 和 `__TEXT,__text` SHA256均相等，严格签名校验均通过。
- 仅对安装副本执行 `lsregister -f`，系统已自行更新 Vega 图标缓存。Launchpad仍只有原 Vega item186，175条布局记录的SHA256逐行保持相同；没有重建数据库、重排应用或重启整个 Dock。Dock tilesize68、largesize128及放大开关均保持原值。
- 真实屏幕区域截图确认 Dock 白色方框消失，圆角底板与系统设置/Chrome尺寸协调；实际打开 Launchpad 并搜索 Vega，图标也为正常透明留白/圆角尺寸。CUA 对 Dock/Launchpad 返回无普通窗口的 timeout；采用本机区域截图与 System Events 向已打开的 Launchpad 搜索框输入，随后退出搜索，恢复 Vega 工作区。没有将解包图片冒充系统截图。

外部证据根为 `/Users/puzige/Workspace/vega-review-20260905/native-r15`。`icon-before-analysis.json`、`dock-before.png`是原始错误；`applied-icons.json`记录两bundle与代码段不变证明；`dock-after-detail.png`、`launchpad-after-detail.png`为实屏结果；`native-final.json`、`launchpad-cache-{before,after-first}.json`记录单实例、缓存更新与布局不变。回退副本位于 `rollback-installed/Vega.app`。已安装应用仍保留其原功能版本，最新功能验收副本保持R14自有测试配置。

## 验收与旧测试夹具修复

**最终完整 workspace：988 passed / 0 failed / 0 ignored；fmt、strict all-target clippy 与 build 均通过。** 验证HEAD `4e8e61de8627cc4be001378ea18d59309d1ace02`，tree `1f8cfed07f5d7d51897f7fd15a251100d0df560d`，tracked diff为空；本报告为之后纯文档记录。

| 命令 | 结果 | 耗时 | 原始日志 / SHA256 |
|---|---|---|---|
| `cargo fmt --all -- --check` | PASS | 1.061s | `/private/tmp/vega-r15-root-2-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS | 1.713s | `/private/tmp/vega-r15-root-2-clippy.log` / `533bfa1222b00d7b5fb6881b22daed79827668f1eb5af5c52680d0760666e37c` |
| `cargo test --workspace --locked --no-fail-fast` | 988/0/0 | 89.178s | `/private/tmp/vega-r15-root-2-workspace-tests.log` / `fef2ecdce2577e108214f30feef49ef66ab333f181c898be88f0dd08cd030975` |
| `cargo build --workspace --locked` | PASS | 0.367s | `/private/tmp/vega-r15-root-2-build.log` / `3fe99d6b49536662aac1f40d24aa8a1da5db57b37b4f5b8ead877a9482d2ad89` |

[测试夹具修复交付](vega-r15-fixture-delivery.md)保留11项定向回归与代码范围；主Agent确认所有请求/结果断言原样，最终完整gate包含该修复。

首次全量在既有模型服务测试收尾挂起：1秒进程sample显示测试线程同步 `http.join()`，server线程阻塞无写超时的 `socket.write_all()`；生产结果已经返回并通过断言，超大body的提前拒绝令当前线程Tokio运行器与socket清理相互等待。主Agent仅终止该owned测试进程，保留其他target完成。该轮exit101、420.915s、已完成target汇总688通过；conversation target没有完整summary，不能写成688/0全量通过。

首次日志 `/private/tmp/vega-r15-root-workspace-tests.log` SHA256 `d82afa6d6512d8add5752f3197e6ba09d4082d181cb73785b40d03f97720e3bb`，`first-gates.json`及`first-gate-interruption.json`保留中止原因；sample SHA256 `8b024f419131d9c4488e75426e56874729894f6ee0b68cdda3919230a1e7972f`。已另写窄规格交给 Astra medium，仅为测试 server 增加有限写超时并用 `spawn_blocking` 异步等待收尾，原请求/结果断言和生产客户端、超时、上限保持。该修复不改变安装应用功能。

## 边界

本轮无新依赖、数据迁移、应用UI/业务改动、Keychain或真实供应商请求。完整release打包未重复构建；共享生产图标导出、签名和真实系统展示已验证。跨macOS版本的渲染未逐版验证。性能bench/soak继续延期；未push或合并master。后续仍按103项矩阵推进外观字号/代码显示与通知设置。
