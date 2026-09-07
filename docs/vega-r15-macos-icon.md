# R15 — macOS Dock 与启动台图标尺寸

Type: Implementation
Status: Delivered locally — [主 Agent 验收](vega-r15-acceptance.md)
Owner: 主 Agent review/integration；Astra medium 专属 executor 实现

2026-09-06 用户指出 macOS Dock 和 Launchpad 中 Vega 图标偏大。主 Agent 从正在运行的 R14 bundle 解出实际 icns：32、128、1024 三个尺寸 alpha 全为1，外围没有透明像素；本机 ZCode 1024 图标的半透明阈值主体范围为96…927（832px），32/128尺寸也有同比留白。现有 SVG 主底板为64…960（896px），打包使用 qlmanage 的缩略图后再缩放，实际产物存在不透明白色画布。

## 交付要求

1. 修复生产 `cargo xtask package` 的图标导出流程，使画布外围真正透明；标准1024画布的可见主底板约832px、居中（四边约96px留白），与已观察的本机参考比例协调。保留现有白底祖母绿终端/星芒图案、色彩和形状关系，不引入新的品牌设计；允许均匀缩放既有矢量图组以适配底板。
2. 16/32/128/256/512及其@2x所有icns表示均正确透明、居中、无实心白框，低分辨率清楚。不要只修一个原生验收副本；未来正式打包必须稳定生成相同结果。
3. 仅修改图标 SVG/生产导出实现和必要的打包辅助脚本/资源说明。零新增第三方依赖；允许macOS已有AppKit/CoreGraphics/WebKit与swift、sips、iconutil等系统构建工具。实现应从矢量源可靠导出，不能根据白色像素做色键抠图而破坏白底/抗锯齿。选定方案须实际运行生产入口生成icns并解码核对透明像素，不以源码注释宣称透明。
4. 本卡是低风险打包外观修正，不写镜像实现的尺寸单测；做一次真正的生产导出、解包像素/尺寸核对和视觉检查。现有packaging测试保持；确需更新已有源选择/工具假设断言时必须说明原因，业务与签名断言不能放松。标准fmt/clippy/workspace/build由root统一执行；性能bench/soak延期。
5. 主Agent负责验证现有Dock/Launchpad的应用路径、原生启动及系统展示。为兑现本次图标修复，允许备份并只更新 `/Applications/Vega.app` 的图标资源与重新签名；不替换其应用功能代码、不改全局Dock尺寸或Launchpad行列数，不重置整个图标数据库/布局。仅在确有缓存问题时针对Vega做最小刷新，保留回退资料。

## 执行与证据

专属 sibling `vega-r15-macos-icon` / `codex/r15-macos-icon`，从本spec提交建立。先读AGENTS.md、docs/vega-exec-guide.md并fetch/rebase。executor仅负责assets/logo、xtask图标导出、必要构建脚本与`docs/vega-r15-icon-delivery.md`，不得操作用户应用、原生UI、Dock或Launchpad、不可改integration工作树。交付记录精确命令、耗时、首次失败、最终icns/像素证据与偏离。本轮不访问真实密钥/Keychain/provider，不改数据库或产品UI。

## 验收补充：已有 HTTP 测试夹具的有界收尾

最终全量在既有 `provider_settings::tests::production_failures_are_bounded_content_free_and_never_retry` 挂起。主Agent取得当前owned进程1秒sample：测试线程位于 `tests.rs:262` 的同步 `http.join()`，server线程位于 `tests.rs:115` 的无超时 `socket.write_all()`；生产service已经完成并通过结果断言。Content-Length超限提前拒绝后，当前线程Tokio runtime被join堵住，连接清理无法继续，fixture仍阻塞写入。首次未完成gate与sample必须保留，不能记为通过。

主Agent批准一个仅测试夹具的后续实现切片：专属 `vega-r15-fixture-bounds` sibling，Astra medium；仅修改 `crates/vega_conversation/src/provider_settings/tests.rs` 的fixture收尾及对应交付说明。server写入设置合理有限超时，async测试通过 `spawn_blocking` 等待server线程完成并保留其panic/结果传播，避免阻塞Tokio线程。保留全部请求/结果/数据断言，不改生产client、连接/总时限、body上限、runtime或model功能。无需增加镜像测试，复验现有provider_settings测试并由root再跑统一门禁。
