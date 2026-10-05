# Issue #181 交付与手测记录

关联：[实现规格](vega-issue-181-auto-update.md) · [实现卡](https://github.com/puzige/vega/issues/181)。

## 当前证据边界

既有实现记录未新增或运行本地测试；本次补充的定向忙碌保护回归单独记录如下。静态审查、编译、云端 check 与真实签名安装分别报告；任何一个不替代其他证据。真实 Developer ID、公证与 UI 手测尚未执行。仓库凭据查询为空，本轮不配置凭据、不发 tag、不更改日常应用。

## 安装忙碌保护定向回归

- 测试提交：`8c488552f92de098d0016e3ec193b4d8e0a1a8d3`。
- 命令：`cargo nextest run -p vega issue181_install_refuses_each_active_owner_without_dispatching`；exit 0。Nextest run ID：`1ae3fb81-ad2d-4005-b64d-2007c6f32fbe`；`PASS [0.097s] vega::bin/vega tests::updater::issue181_install_refuses_each_active_owner_without_dispatching`；结果：1 passed，213 skipped。
- 此回归覆盖生产窗口的活动任务安装拒绝门禁；真实签名更新、原生安装替换、安装失败回滚及启动恢复仍为 NOT RUN。

## UPD06 / UPD09 owned-temp 定向回归（2026-10-05）

- UPD06：使用隔离 `TempDir` 和真实 ZIP fixture，验证归档父路径穿越与 symlink 条目均被拒绝，已有假 app 的 executable 与 bundle 数据保持不变。
- UPD09：`updater_rollback_helper_restores_old_bundle_and_preserves_failed_candidate` 直接对 rollback helper 注入失败，验证旧 executable 与资源恢复、新候选保留为 `failed.app`。这是 helper 级 fault-injection 回归，不覆盖 `replace_and_launch` 内的启动错误分支或完整安装集成路径。
- 命令：`cargo nextest run -p vega updater::`；exit 0；Nextest run ID：`ad221517-3e19-4a20-ab63-3436247517c3`。

```text
Finished `test` profile [unoptimized + debuginfo] target(s) in 4.71s
Starting 6 tests across 2 binaries (211 tests skipped)
PASS [0.018s] vega::bin/vega updater::platform::tests::updater_private_tempdir_rejects_group_and_world_permissions
PASS [0.019s] vega::bin/vega updater::platform::tests::updater_private_tempdir_owner_only
PASS [0.023s] vega::bin/vega updater::platform::tests::updater_archive_symlink_is_rejected_without_modifying_existing_bundle
PASS [0.023s] vega::bin/vega updater::platform::tests::updater_archive_parent_path_is_rejected_without_modifying_existing_bundle
PASS [0.024s] vega::bin/vega updater::install::tests::updater_rollback_helper_restores_old_bundle_and_preserves_failed_candidate
PASS [0.047s] vega::bin/vega tests::updater::issue181_install_refuses_each_active_owner_without_dispatching
Summary [0.048s] 6 tests run: 6 passed, 211 skipped
```

- 首次 symlink fixture 尝试失败：`unix_permissions(0o120777)` 被当前 zip crate 编码为普通文件，导致拒绝断言不成立；改用 `ZipWriter::add_symlink` 后上述定向命令通过。
- Developer ID / 公证签名、macOS 原生替换安装、真实安装失败回滚及启动失败恢复仍为 NOT RUN；本地只验证上述 owned-temp helper。

## 用户手测步骤（合并后）

1. 从固定 `~/Documents/Vega/Vega.app` 打开应用，在设置中找到更新区域，确认版本与实际 bundle 的 `CFBundleShortVersionString` 一致。
2. 关闭自动检查并重新打开应用，确认选项保留；手动检查仍可用。断网时手动检查显示错误与重试入口，不影响现有会话。
3. 当当前版不低于最新 stable 时显示已是最新；当前为 ad-hoc 且存在新版时提供官方发布页入口，不替换应用。开发实例应明确显示开发版本。
4. 首次正式签名包需手动安装。由负责人完整配置发布凭据并发布后续较新版本，再验证自动下载、版本说明、稍后和重启安装。
5. 运行任务或准备启动任务期间，安装应被拒绝；稍后回到更新入口时已准备版本仍可安装。没有任务后主动重启，确认新版本号和原有配置/会话。
6. 使用受控异常包验证摘要、身份、Team ID、版本、签名、公证失败均拒绝；仅在专用副本验证替换失败和启动失败恢复，不对日常应用制造故障。

## 恢复语义

安装在目标 app 的父目录内创建独占 `.vega-update-*` 暂存目录，旧包保存为其内部 `previous.app`。正常启动确认后才清理。启动失败恢复旧包；新版仍存活但未及时确认时不强杀，保留旧包与记录。断电或强杀可能中断两次目录重命名；不宣称跨断电原子性。

若发生中断，先确认没有相关 Vega 进程/任务正在运行，核对暂存目录的 `install.json` 与旧包身份，再将对应 `previous.app` 恢复为原 `Vega.app`。不要批量删除 `.vega-update-*`，不要删除配置或 `ai.vega` 数据目录。恢复后保留该次记录供定位。

## 状态

- 实现：代码完成，主 agent 与独立审查完成。
- 编译：`cargo check -p vega -p xtask --bins`，exit 0，3.41s；日志 SHA-256：`66ab1b97530a826e4f94a7b597e843e2f96e727eb359d0014260659cc0fe4319`。
- 本地测试：忙碌保护与 UPD06/UPD09 定向 updater 回归合计 6 passed、211 skipped；其他本地测试未运行，详见上节。
- 云端 check / PR / merge：[#256](https://github.com/puzige/vega/pull/256) 于 2026-10-05 合并，PR head `8587774f73c228cbf502dc3c7eec068204b7ce5d`；CI run `37262763011` 的 Clippy（2m51s）、Nextest（4m57s）及汇总 `check (fmt, clippy, test)` 均通过。合并提交 `f87479f5e6f7468e6dfe18d5acc15b53ef9a8f51` 对应版本 v0.1.45。
- 真实签名安装与用户手测：NOT RUN。
- 日常安装：未更新。

## 编译输出（有界原文）

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.41s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

## 已知限制

- 自动安装只允许固定 `~/Documents/Vega/Vega.app`；其他 bundle 保留版本检查与发布页。
- “稍后”保留当前会话中的已下载包，不承诺跨退出恢复下载。普通退出或崩溃可能留下未安装的 `.vega-update-*`；本期不扫描删除历史目录，避免误删仍有 helper/恢复责任的备份。需要清理时逐个核实，不自动批量删除。
- 发布凭据未配置，签名成功路径、Gatekeeper 实机表现与更新后的首次启动均为 NOT RUN。

## 受影响 binary 静态 lint

`cargo clippy -p vega -p xtask --bins -- -D warnings`：exit 0，6.88s。未包含 test targets 或运行测试。日志 SHA-256：`7b1b4cc1ad034a5396eb1871b340d702a2984a352343348032c595b97bebb3b2`。

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.88s
```

## master 自动发版追加与基线同步

2026-09-25 用户确认每次合入 master 后 patch +1 自动发布。发布流程改为共用版本分配、签名打包和 draft→publish；同 SHA 重跑复用版本。此前仅 tag 发布的约定已被取代。

同步 #149/#183 基线后：`cargo check -p vega -p xtask --bins` exit 0（4.94s）；同范围 `cargo clippy -p vega -p xtask --bins -- -D warnings` exit 101，报基线 `mcp_registry.rs` 的 `large_enum_variant`。本卡未改该运行时文件；保留失败记录，实际合并门禁使用仓库云端全目标 check。上述早期 lint PASS 仅对应同步前代码，不能当作同步后通过。

推送认证：HTTPS 的 gh OAuth token 缺 workflow scope；本机现有 SSH 认证已由 GitHub 确认身份 puzige，后续使用现有 SSH 推送，不更改或扩展凭据权限。
