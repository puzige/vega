# Vega #75 UI/store 交付记录

日期：2026-10-12。范围：Native 设置、运行控制 UI、worker 生命周期接入。运行时、MCP、conversation 由独立 core 提交提供；本分支仅临时应用 `/tmp/vega-75-core-api.patch` 及其中 `tool_calls.rs` 的遗漏 match 修正用于编译，未将这些依赖文件纳入 UI 提交。正式 core 现已集成，整合后的 30 项定向验证已通过，见[主线集成验证](vega-issue-75-computer-browser-delivery.md#主线集成验证)。下文保留 UI 分支的独立验证证据。

## 实现

- `[agent] desktop_use_enabled/browser_use_enabled` 默认 false，旧配置兼容；Settings CAS 只更新这两个明确字段，保留磁盘上的其他设置并拒绝同字段冲突。
- Settings → Agents 的 Native 开关仅用于新运行。随包组件检测调用 `ManagedComponents::detect()`；系统权限与扩展连通性在首次连接检查。辅助功能、屏幕录制和固定上游版本的扩展安装说明可直接打开。
- Worker 从本次加载的配置创建唯一 `ComputerUseRun`，通过有 generation/thread/stream 栅栏的 Ready 事件将同一 control 注册到 UI，两条 Native Actor 路径接收同一 run。停止早于 Ready 时立即撤权；终态执行异步 shutdown。
- 主内容区域固定底部控制条持续显示真正的 owner 和目标；切换任务、设置页保持可见。授权、接管、继续、停止回调核验 generation、epoch、phase 和 capability，浏览器与窗口审批还由 core 核验目标。Pausing 阶段无继续按钮。
- Stop、Plan review 的取消入口、后台任务删除确认、终态和窗口释放均先撤销 control，再取消运行。Codex 路径不获得此能力。
- 最新观察按需展开并标注来源。预览最多保留两张；替换、epoch 变化、折叠、撤权、终态和 Entity 释放时清理 GPUI ImageDecoder 缓存与已解码的 RenderImage 纹理。工具卡只展示安全动作摘要，不持久化截图或 AX。

## 自动验证

这是进程内 store/GPUI 行为验证，不是已控制用户桌面或浏览器的验收，也不是像素截图验收。没有启动执行器、模型、真实浏览器或操作用户窗口。

```sh
cargo check -p vega --tests
cargo nextest run -p vega_store -p vega_ui -p vega -E 'test(issue75) | test(issue296_h3a_agents_argv_has_input_surface)'
```

配置字段测试的 red 输出保留在 `/tmp/vega-75-ui-store-red.log`（新增字段前编译失败）。缓存测试曾发现释放需等待 GPUI effect flush；原始失败保留在 `/tmp/vega-75-ui-tests-cache-green.log`，修正测试调度后验证 release 回调确实移除 App asset。最终输出（工作树绝对路径已脱敏；其余内容保留原样）：

```text
   Compiling vega v0.1.0 (<ui-worktree>/crates/vega)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 9.17s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID 5bd1a9e7-9684-4c08-95e7-d14ea9a14a47 with nextest profile: default
    Starting 8 tests across 4 binaries (1065 tests skipped)
        PASS [   0.023s] (1/8) vega_store config::tests::issue75_computer_use_settings_default_off_and_round_trip
        PASS [   0.029s] (2/8) vega::bin/vega window::computer_use::tests::issue75_observation_previews_evict_app_assets_on_replace_clear_and_release
        PASS [   0.048s] (3/8) vega::bin/vega tests::computer_use::issue75_window_cleanup_revokes_every_control_before_cancelling
        PASS [   0.051s] (4/8) vega::bin/vega tests::computer_use::issue75_late_ready_after_stop_is_revoked_and_stale_generation_is_ignored
        PASS [   0.083s] (5/8) vega::bin/vega tests::computer_use::issue75_confirming_background_task_deletion_revokes_its_control
        PASS [   0.237s] (6/8) vega_ui settings::tests::issue296::issue296_h3a_agents_argv_has_input_surface
        PASS [   0.290s] (7/8) vega_ui settings::tests::issue296::issue75_native_toggles_preserve_other_disk_settings_and_reject_conflict
        PASS [   0.435s] (8/8) vega::bin/vega tests::computer_use::issue75_control_bar_survives_settings_and_stop_has_epoch_and_owner_fences
────────────
     Summary [   0.436s] 8 tests run: 8 passed, 1065 skipped
```

## 仍需用户手测

在正式整合并签名的 Vega.app 中授予 TCC 权限、安装并连接 Playwright 扩展，选择一个窗口/标签页，核对授权、真实观察、接管、继续、停止和应用关闭。此 UI 提交没有将这些外部集成声明为已通过。

最后一轮缓存审查将首次解码读取改为 `use_render_image`，确保异步解码完成通知重绘。该修正后的定向命令与输出（工作树绝对路径已脱敏）：

```sh
cargo nextest run -p vega -E 'test(issue75_observation_previews)'
```

```text
   Compiling vega_ui v0.1.0 (<ui-worktree>/crates/vega_ui)
   Compiling vega v0.1.0 (<ui-worktree>/crates/vega)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 8.88s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
────────────
 Nextest run ID e54f7efc-6daf-4f29-9720-d5f7e6929d48 with nextest profile: default
    Starting 1 test across 2 binaries (255 tests skipped)
        PASS [   0.033s] (1/1) vega::bin/vega window::computer_use::tests::issue75_observation_previews_evict_app_assets_on_replace_clear_and_release
────────────
     Summary [   0.033s] 1 test run: 1 passed, 255 skipped
```
