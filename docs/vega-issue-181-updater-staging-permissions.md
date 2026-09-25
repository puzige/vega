# Issue #181 follow-up: secure update staging directory permissions

来源：[Issue #181](https://github.com/puzige/vega/issues/181)、[自动更新规格](vega-issue-181-auto-update.md)、[独立更新签名规格](vega-issue-181-independent-update-signing.md)。本 follow-up 只修复桌面端真实更新时的暂存目录权限错误，不改变更新协议与既有安装安全边界。

## 事实与根因

- 本机 Vega v0.1.9 检查到 v0.1.10 后，下载阶段在 UI 报“更新暂存目录权限无效”，新版本尚未安装。
- `Worker::check_release` 当前使用未显式设置权限的 `tempfile::Builder::tempdir_in(parent)` 创建下载目录，随后调用 `platform::private_dir`。
- `private_dir` 要求目录属于当前 effective user，且 `mode & 0o077 == 0`。创建出来的目录因而必须明确为 owner-only；不显式控制 `tempfile` 权限会使此校验失败。
- 安装 helper 在通过 `private_dir` 校验的下载 staging 内创建 `authenticated-*` 解包目录。该父目录已经不可由 group/world 遍历；follow-up 仍统一显式设定嵌套目录为 owner-only，以保持解包区自身的私有权限契约。

## 冻结范围

- 为 updater 提供唯一私有临时目录构造路径，调用 `tempfile::Builder` 受支持的权限 API 明确设置 Unix mode `0o700`，然后继续执行现有 owner/mode 安全校验。
- 下载暂存目录和安装 helper 的认证解包目录都使用该构造路径。其他 `tempfile` 用途（例如临时文件）不变。
- 保留对符号链接、所有者和 group/world 权限的现有拒绝行为；不放宽 `private_dir`，不提权，不改下载、签名、解包、替换、回滚、启动确认或恢复协议。
- 不新增依赖、公开 API、数据库状态或配置项。

## 验收矩阵

| ID | 前置状态 | 操作 | 预期 | 测试 |
|---|---|---|---|---|
| STG01 | 当前用户拥有的临时父目录 | 经 production staging helper 创建目录 | effective UID 为当前用户，mode 恰为 `0o700`，既有 `private_dir` 校验通过 | updater 定向 Nextest |
| STG02 | 上述私有目录 | 将 mode 改为 `0o755` 后调用 `private_dir` | 保持拒绝，错误为“更新暂存目录权限无效” | updater 定向 Nextest |
| STG03 | 下载与安装流程 | 检查两处临时目录构造调用 | 下载 staging 与认证解包目录均调用 production 私有目录 helper；其余验签、恢复流程不变 | 静态审查 |
| STG04 | 真实桌面端 v0.1.9 → v0.1.10 更新 | 在修复版本发布后用 Vega 设置触发检查/下载 | 不再出现权限错误，下载到 Ready；原安装在用户明确重启前保持不变 | 合并发布后的 Computer Use 验收；本实现阶段 NOT RUN |

## 实现与验证计划

1. 先在 production helper 的回归测试中复现默认权限未满足私有目录契约的失败。
2. 用 `tempfile::Builder` 的权限设置 API 明确请求 `0o700`；下载与嵌套解包目录统一使用 helper。
3. 运行仅匹配 STG01/STG02 的 `cargo nextest run -p vega -E 'test(updater_private_tempdir_)'`；运行 `git diff --check`。
4. 在交付记录保留最初失败和修复后原始测试输出；不运行 workspace 全量测试，不在此 follow-up 提交 push、PR 或 merge。

## 偏离与恢复

- 与父规格无偏离。新增显式 `0o700` 是使现有 owner-only 校验与目录创建契约一致。
- 失败时仍按原路径返回错误并 fail closed；不继续下载或触及已安装 app。代码恢复可通过回退此 follow-up commit 完成。

## 实现交付与验证证据

- verified_at_utc：2026-09-25 13:55 UTC。
- 分支：`feat/181-updater-staging-permissions`；已先 fetch/rebase 到当时最新 `origin/master`。
- OS/架构：Darwin 24.6.0 arm64；Cargo 1.98.0；rustc 1.98.0。
- 受影响 production 文件相对 `origin/master` 的二进制 diff SHA-256：`183b4c6a05634d9ce1ed47bfd7690e2ed6323192fe929dc17c0478526067b1ee`。
- 根因复现：给 helper 暂时使用未配置权限的 Builder 后，helper 按原样调用 `private_dir`，与桌面错误一致地拒绝了默认目录。该初次失败输出保留如下。

```text
        FAIL [   0.014s] (1/2) vega::bin/vega updater::platform::tests::updater_private_tempdir_rejects_group_and_world_permissions
  stdout ───
    running 1 test
    test updater::platform::tests::updater_private_tempdir_rejects_group_and_world_permissions ... FAILED
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 210 filtered out; finished in 0.00s
  stderr ───
    thread 'updater::platform::tests::updater_private_tempdir_rejects_group_and_world_permissions' (78210333) panicked at crates/vega/src/updater/platform.rs:388:76:
    called `Result::unwrap()` on an `Err` value: Custom { kind: Other, error: "更新暂存目录权限无效" }
        FAIL [   0.014s] (2/2) vega::bin/vega updater::platform::tests::updater_private_tempdir_owner_only
  stdout ───
    running 1 test
    test updater::platform::tests::updater_private_tempdir_owner_only ... FAILED
    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 210 filtered out; finished in 0.00s
  stderr ───
    thread 'updater::platform::tests::updater_private_tempdir_owner_only' (78210334) panicked at crates/vega/src/updater/platform.rs:379:76:
    called `Result::unwrap()` on an `Err` value: Custom { kind: Other, error: "更新暂存目录权限无效" }
────────────
     Summary [   0.017s] 2 tests run: 0 passed, 2 failed, 209 skipped
error: test run failed
```

以上为原始失败输出的测试结果摘录；命令退出状态为 100。

- 修复：`private_tempdir_in` 显式调用 Builder `.permissions(Permissions::from_mode(0o700))`；下载和认证解包目录都使用该 helper，创建后仍走现有 `private_dir` 校验。
- 定向验收命令：`cargo nextest run -p vega -E 'test(updater_private_tempdir_)'`。
- 修复后原始输出（已在最新基线 rebase 后重跑）：

```text
   Compiling vega_ui v0.1.0
   Compiling vega v0.1.0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 7.82s
────────────
 Nextest run ID 03a0958b-065c-4803-b1f3-069b268bd981 with nextest profile: default
    Starting 2 tests across 2 binaries (209 tests skipped)
        PASS [   0.012s] (1/2) vega::bin/vega updater::platform::tests::updater_private_tempdir_owner_only
        PASS [   0.012s] (2/2) vega::bin/vega updater::platform::tests::updater_private_tempdir_rejects_group_and_world_permissions
────────────
     Summary [   0.013s] 2 tests run: 2 passed, 209 skipped
```

退出状态：0。

- `git diff --check`：exit 0，无输出。
- 本地未运行全量测试；格式门禁与定向 Clippy 后续修正见下节。真实桌面更新尚未重试，等待修复合并并发布后由 Computer Use 验收。
- 与规格偏离：无。

## PR #202 格式门禁修正

- 2026-09-25 检查 PR #202 的 [run `36142885736`](https://github.com/puzige/vega/actions/runs/36142885736)：Clippy job `108096749133` 中 `Format` 步骤失败，`Clippy (deny warnings)` 因前一步失败而跳过。完整 job 日志现可读取，CI 报错摘录如下（runner 路径简化为仓库相对路径）：

```text
Run cargo fmt --all -- --check
Diff in crates/vega/src/updater/platform.rs:386:
-        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
-            .unwrap();
+        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
Process completed with exit code 1.
```

- 本次 CI 失败仅为格式检查；实际 Clippy 步骤被跳过。下方本地格式命令复现了同一差异。
- 本地用相同格式门禁复现的唯一差异是新测试的 `set_permissions(...).unwrap()` 可合并为一行：

```text
Diff in crates/vega/src/updater/platform.rs:386:
-        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))
-            .unwrap();
+        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
```

- 已执行 `cargo fmt --all`；本次仅格式化了上述表达式。
- `cargo fmt --all -- --check`：exit 0，无输出。
- 格式修正后的定向 lint：`cargo clippy -p vega --all-targets -- -D warnings`，exit 0；`Finished dev profile [unoptimized + debuginfo] target(s) in 2.05s`。只有依赖 `block v0.1.6` 的既有 future-incompatibility 提示，没有 lint warning。
- 格式修正后定向回归：`cargo nextest run -p vega -E 'test(updater_private_tempdir_)'`：exit 0，2 passed、209 skipped；Nextest run ID `0b7ce366-64b4-47e2-9d4d-2dad6b5c7f97`。
- 原 PR run 的 workspace Nextest 最终通过，耗时 6m19s；summary check 因 Clippy job 的格式步骤失败而失败。此次未运行本地 workspace 全量 Nextest；PR 云端 Nextest 门禁负责全量测试。

## PR #199 合并后的基线复验

- PR #199 合并后已 fetch 并 rebase 到最新 `origin/master`，包含 Composer 错误行修复；生产变更 diff hash 相对新基线复算后仍为上方记录值。
- `cargo fmt --all -- --check`：exit 0，无输出。
- `cargo nextest run -p vega -E 'test(updater_private_tempdir_)'`：exit 0；2 passed、209 skipped；Nextest run ID `65956143-dc27-4311-b2b3-9967ae24b66e`。
- `cargo clippy -p vega --all-targets -- -D warnings`：exit 0；`Finished dev profile [unoptimized + debuginfo] target(s) in 3.93s`。保留 `block v0.1.6` 的既有 future-incompatibility 提示。
- 未运行本地 workspace 全量测试；等待更新 PR 后的云端 Nextest 与格式/Clippy 门禁。
