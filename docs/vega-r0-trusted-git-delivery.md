# R0a 受信任 Git 来源交付记录

## 交付状态

R0a 的固定 Git 来源、版本准入、文件 identity、环境清洗、production runner 接线、typed 错误和 UI/安装诊断已落地。实现提交为 `425b30daa77bd0447cd682cfe36d12673bf33caf`（`feat(r0a): pin trusted Git source`）；冻结的 `docs/vega-r0-trusted-git-sdd.md` 已随该提交纳入。resolver 源文件 SHA-256 为 `72b08b2f077aae8b188926d1c6c22979740bdb64e78ebdf0c77f935242b78f4a`。

本机 arm64 解析实际选择 Homebrew Git 2.55.0。production 候选固定为 `/opt/homebrew/opt/git/bin/git`、`/usr/bin/git`（x86_64 使用对应的 `/usr/local/opt/git/bin/git` 和系统路径，其他 Unix 只使用系统路径）；不读取 PATH、Codex 缓存、仓库或任意配置来选择 executable。

## 实现边界

- Homebrew candidate 必须 canonicalize 到与 candidate prefix 相同的 `Cellar/git/<single-version>/bin/git`，是 regular executable，且 group/world 不可写；system candidate 必须是 canonical `/usr/bin/git`。
- probe 前捕获 admitted canonical file 的 dev/inode/size/mtime/ctime identity，probe 后重新检查 canonical path、regular/executable/权限和同一 identity。每次 child spawn 前再次 verify；删除、替换、chmod 或 identity 变化返回 `git_executable_changed`，不会 mutation。
- 成功选择由 process-wide cache 共享；取消、缺失、旧版本和 operational failure 不会永久缓存。只有实际不支持的版本继续尝试下一个固定 candidate。
- probe 只使用无 shell 的固定 `--version`，复用 bounded collector 和 caller cancellation；完整 banner 解析要求 `git version X.Y[.Z]`，拒绝额外行、CRLF 和任意尾随文本。准入版本为 Git 2.40+，目标树的 `check-attr --source` 能力检查仍在既有 branch preflight/execute 路径中执行。
- 所有 production read、refresh、diff、branch switch、stage、commit、summary runner 使用同一 resolver 来源。Git 子进程清洗 `GIT_*`、`DEVELOPER_DIR`、`TOOLCHAINS`、`DYLD_*` 和 `LD_*`，并保留既有 argv、root/path/operation/lease/owner 和 bounded output 断言。
- shared types 新增 `git_unavailable`、`git_unsupported`、`git_executable_changed`；branch、diff、artifact、commit UI 映射为可操作短诊断。包装文档和 `xtask` 安装文本说明 Git 2.40+ 与 Homebrew 安装方式，不自动安装或替换系统 Git。
- true delegating test scripts 使用与 production resolver 相同的 executable 和 shell quoting；fixture 初始化仍可用系统 Git，直接 test fault seam 仍是显式 test-only 路径。没有扩大 public API，也没有修改 R1/R4、provider、Keychain 或用户仓库。

## 验证证据

raw logs 均保存于任务专用目录 `/private/tmp/vega-r0a-luna-20260905/`。下表列出命令、结果和日志 SHA-256；仓库中的交付记录不依赖临时文件内容。

| 范围 | 命令/结果 | raw log SHA-256 |
| --- | --- | --- |
| 格式 | `cargo fmt --all -- --check`，PASS | `fmt-check-01.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| workspace clippy | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`，PASS | `clippy-workspace-01.log`: `7e04adf9113fe10eaa376e0eeb63c17c754dfe796298e47de117ec3cd69097ca` |
| workspace build | `cargo build --workspace --all-targets --all-features --locked`，PASS | `build-workspace-01.log`: `0dec4d36184cb3a9f0f0053c4ff8d86aa390560ae0542f8a598910e9f9c9461d` |
| 真实 S6 production E2E | `cargo test -p vega_conversation --test s6_acceptance -- --test-threads=1`，首次和二次均 2/2 PASS | `s6-acceptance-first.log`: `0df1a0e9b374255f26a1d667f13cd3960767798e59d7040f4c26bff76c77fed1`; `s6-acceptance-02.log`: `52963ba4a83b3d1789c067a218ec8f1a7608df8befdf81dfc7cea1ad61bbf610` |
| Git workspace 集成回归 | `cargo test -p vega_conversation --lib git_workspace -- --test-threads=1`，146/146 PASS | `git-workspace-lib-02.log`: `6c204554cbbfe3ee0df7a3a2a23e5aecfe8e207d58c1775967c55f207cf69b0d` |
| resolver 安全测试 | 固定来源、parser、cache retry、identity replacement/deletion/chmod，9/9 PASS | `executable-tests-02.log`: `03140e2fafa43fdc340b07659e1608fb8175e88d9edf03e956261ca930011c8b` |
| environment scrub | `DEVELOPER_DIR`、`TOOLCHAINS`、`DYLD_*`、`LD_*` 与既有 GIT 清洗，1/1 PASS | `environment-scrub-01.log`: `a6988923bbc7969fa764c8cc62fffaedc8af6f64aba161f24f7133468e7d85ad` |
| lease cleanup | production delegating wrapper 精确用例，1/1 PASS | `lease-cleanup-rerun.log`: `0eb939f9fa25b6f348e6021e8c9024aa0e62d2ce7f682bfda0000081a942eac1` |
| UI/安装 | full workspace test 中 UI 115/115 PASS；另有 UI check PASS；`xtask` 36/36 PASS | `test-workspace-01.log`: `7826094bb6e56db707d313f6389cda5f0946ab2a5b2c74e1b58cf63b80fab1a7`; UI check `check-ui-tests-01.log`: `ef2dd036fe62e2228dd42cdfb1606d2cbf435e0a842c8f7904b4af622cad83fe`; `xtask-tests-01.log`: `d0e1eaf1b8224d24197b06ef9d82de1bd3d2ee608bd97f7e7488fd35629974a3` |

## Full test 残余

首轮 `cargo test --workspace --all-features --locked --no-fail-fast` 已完整跑完并保留在 `test-workspace-01.log`，但 `vega_conversation --lib` 有 258 passed / 3 failed：

- `git_workspace::tests::caps_runner::commit_summary_deferred_overflow_never_eof_uses_bounded_timeout`：full-suite run 中 overflow marker 未在断言窗口内出现，原因待归因；
- `git_workspace::trusted_git::tests::runner_mutation::trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit`：full-suite run 中一次返回 `TimedOut` 而非 `GitFailed`，原因待归因；
- `git_workspace::trusted_git::tests::runner_mutation::trusted_mutation_runner_times_out_cancels_and_reaps_process_groups`：full-suite run 中 fixture readiness spawn race 在五次重试内未收敛，原因待归因。

其余 workspace targets（包括真实 S6、UI、xtask 和 doctests）通过。三个失败均为进程/fixture 时序相关待归因项；按裁决没有改 timeout 或断言。三个 exact test 随后分别以单线程重跑，均 1/1 PASS，日志分别为 `f3-rerun-commit-summary-01.log` (`b83f2d6888d5e9df196c4ac63cf7e82684af0586072273ddb9ec49eacea1c4b6`)、`f3-rerun-mutation-caps-01.log` (`b3e37c068dc5a981e22ee43d97a41854bdea6cd0abcad12f96916bbb511e20f7`) 和 `f3-rerun-mutation-timeout-01.log` (`f422f7d847bd13a6c17f2c8eeb85fe0f6a2d47f94075b19da8e4a71461ba3852`)。因此本卡不宣称 full workspace test 全绿，三项交由 R0b/F3 后续处理。

首轮 Git workspace 137 项日志 `git-workspace-lib-first.log` 保留了旧 `/usr/bin/git` delegating wrapper 导致的 136 PASS / 1 FAIL；修正 wrapper 目标后为上表 146/146 PASS。更早的 resolver negative 首次 raw 由协调 Agent 保存在 `/private/tmp/vega-main-review-20260905/r0a-executable-first-root-copy.log`，SHA-256 `b80658f819fc5660198e884df5258bb2d7921ead4a9608540ae6a2577f761c1a`。一个旧 check 日志路径曾被成功 rerun 覆盖，因此不把该路径作为 E0277 首败 raw 证据；这不影响实现/安全失败日志的保留。

## 剩余风险与交接

same-user 在 verify 与 spawn 之间修改 executable、动态 loader 依赖或 system shim 背后的 OS 工具链，仍是 SDD 记录的残余 TOCTOU/信任边界；top-level file identity 检查不声称提供签名验证或原子隔离全部依赖。full-suite 的三项进程/fixture 时序相关待归因项保持为单列 F3 residual；没有在本卡放宽断言、调整 timeout 或运行性能/bench 测试。未执行 push、PR、merge、release、live provider、Keychain 访问或用户仓库 mutation。

实现提交后的工作树应保持 clean；Cargo target 在最终命令结束后无 cargo/rustc/rustdoc 进程，已释放给后续协调工作。最终 local delivery HEAD 由包含本文件的后续本地提交记录；当前实现 commit 的基准 HEAD 如上。

## 变更文件

- `crates/vega_conversation/src/git_workspace/executable.rs`：私有固定来源 resolver、版本 parser、process cache、identity/safety verify 和测试 seam。
- `crates/vega_conversation/src/git_workspace/{mod,runner}.rs`、`branch/mod.rs`、`trusted_git/{mod,service,authority}.rs`：所有 production Git runner 接线、取消/identity verify 和 typed error 映射。
- `crates/vega_conversation/src/git_workspace/**/tests/*.rs`：production delegating wrapper、resolver identity/env/cache 与既有 fault 回归。
- `crates/vega_conversation/src/types/{artifact,workspace}.rs`、`crates/vega/src/commit_controller.rs`：错误码闭合映射。
- `crates/vega_ui/src/{artifact_card,branch_selector,commit_panel/panel,diff_view/render}.rs`：Git 依赖诊断。
- `docs/vega-r0-trusted-git-sdd.md`、`docs/vega-packaging.md`、`docs/vega-s6-tasks.md`、`xtask/src/package.rs`：冻结规格、来源条款和安装说明。
