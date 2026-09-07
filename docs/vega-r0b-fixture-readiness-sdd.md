# R0b 测试夹具启动准备规格

版本：v0.1 · 2026-09-05

## 目标

R0a 的完整 workspace 测试保留三项故障：summary deferred overflow 夹具在
750 ms 窗口内没有到达 overflow marker；mutation nonzero 夹具在 1 s 内得到
`TimedOut` 而不是 `GitFailed`；F3 在 500 ms 故障窗口内多次没有写出 descendant
pid。独立诊断显示，新建可执行 `#!/bin/sh` 脚本的首次解释器启动会污染短故障
窗口；readiness-only 预执行后相同脚本的启动延迟明显降低。该诊断支持夹具
启动准备的方向，但不证明所有进程控制错误具有同一根因。

本卡只隔离测试夹具的首次启动准备，保持故障测试对真实 runner/service 入口的
调用和原有边界断言。它不改变 production runner、超时、进程组清理或公开 API。

## 范围与入口边界

只允许修改下列 test-only 内容：

- `artifact/tests::launcher_script` 生成的 `/usr/bin/open` 替身；
- `trusted_git/tests::scripted_mutation` 生成的 mutation 替身；
- `git_workspace/tests::caps_runner::commit_summary_deferred_overflow_never_eof_uses_bounded_timeout`
  中的手写 summary 替身及其必要测试准备。

Artifact 的 production 入口仍为 `ArtifactService::open_in`，其 launcher 默认仍为
`/usr/bin/open`、production deadline 仍为 10 s。Trusted Git 的 production 入口仍
为 `Runner::run_trusted_mutation*` 和 `Runner::run_commit_summary*`，分别继续使用
120 s mutation deadline、10 s read/summary deadline以及既有 collector、TERM/KILL、
drain/reap 逻辑。测试 executable 注入只通过现有 `#[cfg(test)]` constructor seam；
不新增 production seam、依赖或公开类型，也不改变真实 Git delegating E2E 的来源。

## Frozen readiness contract

每个目标脚本在写入并设为 executable 后，使用同一原路径做一次 direct
`Command` readiness invocation，首个参数固定为专用 marker
`--vega-test-readiness`。脚本的 readiness 分支必须位于所有故障体和副作用之前：

1. 识别 marker 后立即以 exit 0 结束；
2. 不写 attempt、argv、stdin、overflow、pid 或其他故障 recorder；
3. 不启动 descendant，不调用 Git，也不执行 body 的任意其他命令；
4. readiness invocation 使用空 stdin/stdout/stderr 和独立 process group；
5. readiness 总 deadline 固定为 5 s，超时、spawn error 或非零退出直接失败，保留
   有界诊断并回收/reap readiness 进程；
6. 不重写脚本、不重复 readiness、不循环重试直到成功。

故障执行仍通过原有 production 路径启动同一已准备脚本，正常参数不含 marker。
因此 readiness 不计入 Artifact `launch_attempts` 或 mutation attempt recorder，实际
调用的 raw argv、stdin、权限、路径和 process lifecycle 仍由原 runner/service 断言。

## 保持的故障断言

Artifact 继续保持六套 exact raw argv、preflight 零调用、success/failure/timeout
各一次调用、1 s nonzero deadline、20 ms timeout、返回 `GitFailed`/`TimedOut` 以及
shell 和 descendant 均已退出的断言。

Trusted mutation low-level tests 继续保持 pre-cancel 零 spawn、nonzero 1 s
`GitFailed`、stdout/stderr cap 的 inclusive 与 `+1`、exact argv/stdin、每个实际
故障恰一次 attempt、F3 500 ms `TimedOut`、cancel 20 s 与 descendant reap。F3
删除原有五次新夹具循环；一次 readiness 后只执行一次 fault run，并直接要求 pid
文件存在、attempt 恰为一个 `x`、返回 `TimedOut`、`kill -0` 失败。缺 pid 是测试
失败，不得作为 inconclusive 跳过或触发额外样本。

Deferred summary 继续使用 750 ms test-only deadline、256 KiB cap 与 deferred
overflow policy，并保持 `TimedOut`、overflow marker、elapsed 至少 700 ms 且小于
3 s、descendant 已退出的断言。任何生产 10 s/120 s deadline 或 cap、argv、attempt、
pid、TERM/KILL/reap 断言均不可放宽。

## 验收证据

在 R0a 最终树上先保留原始三失败和精确独跑结果。实现后依次保留：

- readiness-only 证据：每个目标脚本的 marker invocation 成功，且故障体 recorder、
  descendant 和 Git 副作用均未发生；
- 相关故障单例及完整 relevant suite 的原始 exit/结果；
- 预先固定 8 次、并发度 2 的相关故障用例批次，保存每一次 exit、typed error、
  marker、attempt 和 pid 结果，失败样本不被覆盖，也不增加样本数重试取绿；
- 最终完整 workspace、fmt、clippy、build 门禁由协调 Agent 执行。

该批次是有限功能测试稳定性证据，不是 benchmark 或 soak。若 readiness 后仍有
进程控制、timeout 分类或 pid 失败，保留首次失败与边界证据，回到机制调查；不加
生产时间窗、不增加 fault retry、不删除安全断言。

## v0.2 受限进程控制诊断（仅定位首败）

为定位完整 workspace 中偶发的 `process_control_failed`，增加一个只在
`cfg(test)` 生效的局部诊断。状态属于单次 `collect_child` 调用，不使用全局共享
状态，不改变 reader、process-group、deadline、drain、reap 或错误分类语义。只有
最终错误为 `ProcessControlFailed` 时才向测试输出一条固定字段诊断。

诊断只保留安全的首次阶段枚举（句柄捕获、初次 `try_wait`、初始排空、排空重试、
清理、最终 reap、最终状态）、已收到的 reader 输出数、`status.is_some()`、
`cleanup_failed`、停止错误码和 `try_wait` 的 `raw_os_error`。不得记录 argv、路径、
输出正文、stdin、key、pid 或其他可识别内容。`try_wait` 的错误仍按既有
`ProcessControlFailed` 路径返回；诊断仅用于区分初始排空、poll/reap、清理和最终
状态缺失，后续修复必须基于保留的首个失败阶段，不得靠重跑、放宽安全断言或延长
生产时间窗取绿。

## 变更记录

- v0.1（2026-09-05）：冻结 R0b test-only readiness 边界、5 s 单次准备、既有故障
  时限/断言与固定 8×并发度 2 验收批次。
- v0.2（2026-09-05）：补充单次 `collect_child` 的 cfg(test) 首败诊断字段与安全
  输出边界；不改变任何生产进程控制语义。

## v0.3 已就绪 reader 结果的有界排空

受限修复 `collect_child` 初始排空：调用线程在 grace 边界被抢占后，若两路
reader 结果已经排队，必须先非阻塞收取，再决定是否仍有未完成的 pipe。
固定 500 ms grace 只限制等待；每次阻塞等待不得超过剩余 grace，最多接收
两条结果。到期且仍缺结果继续原有 `ProcessControlFailed`、进程组清理和
reap；cancel、总 deadline、overflow 和 inherited-pipe 拒绝语义不变。

以私有 drain helper 的过期 Instant + 预排队结果构造确定性 UNIT 回归；
另验证到期缺一路且 sender 仍存活立即保留缺失状态，并运行既有 lifecycle
和 mutation 入口测试。这证明局部调度竞态，不把历史 Artifact/Diff 首败
认定为同一根因，也不新增 timeout、依赖、公开 seam 或 DDL。

- v0.3（2026-09-05）：基于可确定性构造的 ready-result 丢弃竞态，授权上述
  最小生产排空修复；历史偶发错误的根因关联仍未证实。

### v0.3 定向验收

- verified_at_utc：2026-09-05T11:23:13Z；local：2026-09-05 19:23:13 +08:00。
- branch：`codex/vega-drain-ready-result`；runner tracked diff SHA-256：
  `aedfcec39e76d6cd294ec893ecc75ca5d63f208a4f8fd6ee9cfee46b49fd3ba5`。
- UNIT 红灯：抽取但保持旧 loop 后 `cargo test -p vega_conversation expired_drain -- --nocapture`，
  `0 passed; 2 failed`；两条已排队结果被返回为空，单条已排队结果亦被返回为空。
- UNIT / 真实 Git / FAULT-INJECTION 定向集：
  `cargo test -p vega_conversation git_workspace:: -- --nocapture`，
  `149 passed; 0 failed; 0 ignored; 115 filtered out; finished in 48.29s`。
  包含三条确定性过期 drain 回归、inherited pipes 拒绝及组回收、read timeout、
  mutation timeout/cancel/双路 drain；集成测试 binaries 被此 filter 全部过滤。
- `cargo clippy -p vega_conversation --all-targets -- -D warnings`：exit 0，3.27s。
- `cargo fmt --all -- --check`、`git diff --check`：exit 0。
- raw 日志文件名 / SHA-256：
  - `vega-drain-red.log`：`973d0a734a2bd1fe33353e69cd92713e9cdb1b3ae37b632dc915d90e6170a6e3`
  - `vega-drain-green.log`：`a239113df0ebd149f89ed2b49a7d498f7131e49de5d173d6b950a0498fd717e2`
  - `vega-drain-clippy.log`：`6d45e4445e2a3c892d10aade82b0f80745c923444b6b2e814694553a0928bda6`
- LIMIT：历史 Artifact/Diff 首败与此竞态的关联尚未证明；未运行 workspace 全量、
  UI、build、bench/soak、真实 provider 或 Keychain；联合树最终门禁由协调 agent 承担。
