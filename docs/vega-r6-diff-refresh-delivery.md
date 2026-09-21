# Vega R6 Diff 刷新呈现交付

- **日期**：2026-09-05
- **执行器**：Luna / max
- **独立源码提交**：`c57a6010852a4fb1fd059b977738edbe351005be`
- **联合树已集成**：`e90c7a9d220a639f8b42a191cd952bb4585b8666`
- **分支**：`codex/vega-diff-refresh-ui`
- **SDD**：[vega-r6-diff-refresh-sdd.md](vega-r6-diff-refresh-sdd.md) v0.1

## 交付结果

Diff 刷新现在区分 Initial、Retry 和 Background intent。首次打开无快照时显示一次加载进度；用户 Retry 显示显式进度；750ms 后台轮询、tool terminal 和 workspace action refresh 保留已有 rows、header 和 stats，不显示周期性 header 标记，也不把 clean 空态替换成 loading。coalesced 请求保留显式 Retry 的进度意图。

刷新失败时保留已有快照或空态，显示 typed error 和 Retry。保留的快照是只读显示：pending projection 会清理，新的 projection request、迟到成功 projection 和迟到 projection error 都被拒绝；fresh snapshot 清除错误并恢复投影能力。真实 `VegaWindow` controller 回归覆盖了非空快照、后台在途、失败后 Retry，以及真实 clean snapshot 的 `0 files +0 -0` 空态 stats。

实现保留 `DIFF_REFRESH_INTERVAL = 750ms`、route/sequence/coalescing、cancel、snapshot-generation、projection fence、ListState 和既有安全断言。未改 provider、store、DDL、依赖、权限逻辑或 Git 安全命令；测试只使用 owned temporary Git fixture。T48 memory/render/UI 性能调优继续延期，本卡没有运行 bench 或 soak。

## 验证命令与原始日志

以下命令在 `/Users/puzige/Workspace/worktrees/vega-diff-refresh-ui` 执行，使用：

```text
env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true
```

原始日志目录：`/private/tmp/vega-r6-diff-refresh-ui-20260905/`

| 日志 | 确切命令 | 结果 |
| --- | --- | --- |
| `00-clean.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo clean -p vega -p vega_ui 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/00-clean.log` | 清理本卡相关 target |
| `10-controller-r6-final.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo test -p vega diff_refresh_intents_keep_content_during_background_and_retry --locked -- --nocapture 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/10-controller-r6-final.log` | **1 passed / 0 failed**，真实 `VegaWindow` controller Initial/Background/failure/Retry/empty stats |
| `11-controller-fences-final.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo test -p vega diff_controller_route_latest_poll_tool_and_cross_project_fences --locked -- --nocapture 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/11-controller-fences-final.log` | **1 passed / 0 failed**，route/projection fence 与 coalesced progress |
| `12-ui-diff-view-final.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo test -p vega_ui diff_view --locked -- --nocapture 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/12-ui-diff-view-final.log` | **16 passed / 0 failed**，diff-view 定向回归 |
| `08-clippy-rerun.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo clippy -p vega -p vega_ui --all-targets --locked -- -D warnings 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/08-clippy-rerun.log` | **passed** |
| `09-check.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo check -p vega -p vega_ui --locked 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/09-check.log` | **passed** |
| `13-build-vega-final.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo build -p vega --locked 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/13-build-vega-final.log` | **passed** |
| `14-fmt-final.log` | `env PATH=/Users/puzige/.cargo/bin:$PATH CARGO_TARGET_DIR=/Users/puzige/Workspace/vega/target CARGO_NET_OFFLINE=true cargo fmt --all -- --check 2>&1 \| tee /private/tmp/vega-r6-diff-refresh-ui-20260905/14-fmt-final.log` | **passed** |

## 首次失败与计数边界

- `/private/tmp/vega-r6-diff-refresh-ui-20260905/01-controller-r6.log` 使用了 `--exact`，GPUI 宏生成的完整测试名不匹配，因此是 **0 tests**；不计入通过证据。随后无 `--exact` 的初次运行在 `02-controller-r6.log` 中为 1/1。
- `/private/tmp/vega-r6-diff-refresh-ui-20260905/06-clippy.log` 首次 clippy 发现 `request_refresh` dead code 与 `let_and_return`。
- `/private/tmp/vega-r6-diff-refresh-ui-20260905/07-clippy-rerun.log` 修复上述问题后发现保留分号导致 `request_seq` 推断为 `()`。
- 上述失败日志均原样保留；`08-clippy-rerun.log` 是修复后的 clippy 通过证据。`03-controller-fences.log`、`04-ui-diff-view.log`、`05-fmt.log` 是修复前的中间日志，最终计数以 `10`–`14` 为准。

## 交接边界

本卡没有进行新的原生 CUA 走查；root 负责联合树的原生复验。联合 workspace 全量测试与最终 integration gate 待 root 在联合树执行。未读取真实用户文件、未访问 Keychain、未发送真实 provider 请求，也未进行 T48 性能或 soak 测试。

## 后续修订：偶发失败稳定化（issue #99，2026-09-21）

本卡交付的 `diff_refresh_intents_keep_content_during_background_and_retry` 自 2026-09-05
起被 18 份交付文档记录为偶发失败（retry 阶段 `refresh_error=Some(GitFailed)`），处置一律是
孤立复跑通过 + `--no-verify` 推送，门禁判定力因此下降。

已确认该测试用虚拟时钟 pump 等待一个跑真 `/usr/bin/git` 的真线程 worker；虚拟时间
对真实子进程调度无效，但 Git 非零退出的底层原因尚未证明。已改为通过生产入口发起请求、由 `finish_diff_refresh` 注入终态，
断言一条未删、未弱化、未加 `#[ignore]`、未放宽超时，生产代码 0 改动。详见
[vega-r6-diff-refresh-sdd.md](vega-r6-diff-refresh-sdd.md) §7 与 issue #99。

上表 `10-controller-r6-final.log` 记录的是修订前的真实 worker 版本，作为历史证据保留。

集成审查：生产入口仍启动真实 worker，本修订只让 UI 状态机断言不依赖其完成时序。
最终门禁按 Issue #107 运行受影响 vega 包与 Diff 回归；最终内容指纹、命令、退出码和计数
保存在本地 `remaining-merges-2026-09-21` 证据目录，并回写 Issue #99。旧全量要求由新门禁政策取代。
