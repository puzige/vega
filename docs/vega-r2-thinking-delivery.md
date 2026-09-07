# Vega R2 Thinking 交付记录

**日期**：2026-09-05

**分支**：`codex/vega-r2-thinking`

**实现基线**：`81a4419`
**状态**：R2 实现与本地验证完成，待主 Agent 在专用整合树复核

本文记录 R2 专用树的实现范围、验证证据和首轮失败。它与
[vega-r2-thinking-sdd.md](vega-r2-thinking-sdd.md) 配套，不能替代 SDD。

## 交付内容

- `vega_store` 新增独立、带版本的 `reasoning.toml` 表示、严格校验、精确
  `(provider, model)` profile、进程内共享保存协调器、字段级三方合并、临时文件
  flush/fsync、rename 前复查和真实磁盘 readback reconcile。reasoning 文件不覆盖
  `AppConfig`，不新增 DDL、依赖或 Keychain 读取。
- `vega_conversation::types` 提供跨 crate 的 typed reasoning 边界；runtime 保持
  headless 和既有依赖方向。`FrozenReasoning` 在 run 开始时生成，工具轮和 retry
  复用同一快照，并校验 profile 的 provider/model 与请求一致。
- OpenAI-style 请求只发送 profile 声明的合法 effort（含显式支持的 `xhigh`）；
  GLM-5.3/GLM-5.3-FLASH 标准 API 仅允许 `low`/`high`/`max`，不能 disabled。
  Unknown/未声明协议只使用提供方默认并省略控制字段。按 review 裁决，同 run
  内 reasoning 回传不自动发送 `clear_thinking=false`。
- 工具轮原始 GLM `reasoning_content` 仅在内存中有界保留：单 delta 64 KiB、单逻辑
  轮 256 KiB、单 run 1 MiB；超限返回 typed failure，不截断、不把空值伪装成内容。
  既有 `ThinkingDelta` 事件与 redaction 契约保留，原文不进入正文、Debug、日志、
  成本估算或持久化消息。
- Settings 提供可编辑的提供方/model 能力模板和偏好、保存 pending/ack/reload
  投影、精确 owner 路由及 Tab/Enter/Space 焦点动作。controller 对无效声明、保存
  不确定/失败和过期 ack fail-closed；合法缺失 profile 仍显示“使用提供方默认”。
  app 验收使用 owned `SettingsView::from_config`，不读取宿主用户配置。
- Git service/test 中的 `reasoning: None` 仅为请求字段新增后的编译适配；没有改变
  Git 行为、权限、`@file` 或引用解析逻辑。

实现提交为：

- `dc903a8`：R2 thinking SDD v0.1；
- `d4e08cd`：R2 frozen reasoning settings/wire 实现；
- 本交付记录及 SDD v0.2 的提交：见本文件所在提交。

## 验证证据

以下命令均在本树执行，并使用本地 target
`/Users/puzige/Workspace/worktrees/vega-r2-thinking/target`；没有使用共享 target。

| 检查 | 结果 | 原始日志 |
| --- | --- | --- |
| `cargo check --all-targets --locked` | 通过 | `/tmp/vega-r2-thinking-cargo-check-all-targets-7.log` |
| `cargo fmt --all -- --check` | 通过 | `/tmp/vega-r2-thinking-fmt-check-final.log` |
| `cargo clippy --all-targets --locked -- -D warnings` | 通过 | `/tmp/vega-r2-thinking-clippy-dwarnings-final.log` |
| `cargo test -p vega_store --lib` | 92 passed, 1 ignored（真实 Keychain） | `/tmp/vega-r2-thinking-store-full-final.log` |
| `cargo test -p vega_runtime --lib` | 100 passed | `/tmp/vega-r2-thinking-runtime-full-final.log` |
| `cargo test -p vega_conversation --lib` | 261 passed | `/tmp/vega-r2-thinking-conversation-full-final.log` |
| `cargo test -p vega_ui --lib` | 118 passed | `/tmp/vega-r2-thinking-ui-full-final.log` |
| `cargo test -p vega --bin vega` | 49 passed, 0 failed | `/tmp/vega-r2-thinking-app-full-final-2.log` |
| store reasoning targeted | 7 passed | `/tmp/vega-r2-thinking-store-reasoning-full-3.log` |
| runtime agent targeted | 39 passed | `/tmp/vega-r2-thinking-runtime-agent-tests-4.log` |
| runtime OpenAI/loopback targeted | 31 passed | `/tmp/vega-r2-thinking-runtime-openai-tests-3.log` |
| app model selection serial | 6 passed | `/tmp/vega-r2-thinking-app-model-selection-serial-3.log` |
| rendered Settings targeted | 9 passed | `/tmp/vega-r2-thinking-settings-tests-5.log` |

这些检查使用 mock、owned temporary state 或 loopback；没有真实 provider 请求、网络
API、Keychain 读取、性能 bench 或 soak。UI 的 rendered Settings 测试覆盖焦点键盘
动作；app 测试覆盖真实 controller/run 入口和 wire body 断言，二者的边界在测试注释
中分别说明。

## 首轮失败与修复

首两次 all-targets 检查保留在以下日志中：

- `/tmp/vega-r2-thinking-cargo-check-all-targets.log`
- `/tmp/vega-r2-thinking-cargo-check-all-targets-2.log`

失败原因是新增 `window/reasoning.rs` 测试触发 GPUI `#[test]` 宏递归深度限制，另有
已移除的 dead `prepare_run` wrapper 和 lint。随后调整测试入口、移除无调用 wrapper
并修复 lint，最终 check 与 `-D warnings` clippy 通过。

首次完整 app 测试保留在 `/tmp/vega-r2-thinking-app-full-final.log`，结果为
48 passed、1 failed。失败来自测试 fixture：原本标为非法的样例实际上是合法的
Unknown profile，因此没有触发 fail-closed；产品逻辑未因此放宽。样例改为真正非法
effort 后，`/tmp/vega-r2-thinking-app-full-final-2.log` 为 49 passed、0 failed。

首次 clippy 警告日志 `/tmp/vega-r2-thinking-clippy-1.log` 保留了
`Result<(), ()>`、needless borrow 和 type complexity 警告；实现改用 typed
`ReasoningSubmitError`、修正借用并抽取局部类型别名，最终门禁使用
`-D warnings` 通过。

## 整合边界

本树尚未 push、merge 或修改主 Agent 联合树。主 Agent 应在
`vega-thinking-integration` 基于 `c09ded4` 依次 cherry-pick SDD、实现和本记录，
并重新验证 R1/R4b/R5/R6/R7。整合时需保留 R5 的原子
`Finished { reference_failure }` 终态、引用失败零 provider 请求和 FileIndex
subscriptions，以及 R7 的 Provider 键盘、`SettingsSaved` candidate/catalog worker
链；R2 reasoning 保存/reload/ack 与 catalog pending guard 需独立存在。

保存协议明确是 fingerprint 检查加 rename，不是对非合作外部写者的原子 CAS；检查至
rename 之间仍有竞态窗口。rename 成功后父目录 fsync 若报错，磁盘结果不确定，失败
路径必须重新读取真实 authority 再决定可提交状态，不能用旧值盲目覆盖。

## 日志校验摘要

主要最终日志的 SHA-256：

```text
e3666d3cc8388241bd89e21639336af1221c8bfaf6a067f6ef983e2e712ef36d  /tmp/vega-r2-thinking-cargo-check-all-targets-7.log
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /tmp/vega-r2-thinking-fmt-check-final.log
db5528586c335f0918962d01ddc3590169cb445aa122af36ac7e313b64d49b53  /tmp/vega-r2-thinking-clippy-dwarnings-final.log
8d1690061609bcb5d307566952906d46ed01f46437a6c473670369c7c381a79f  /tmp/vega-r2-thinking-store-full-final.log
d4d3cd1438bcc31dd14b33ed02aa45ef49fd946a7e4ec1db5e6c689e24a4e865  /tmp/vega-r2-thinking-runtime-full-final.log
ce7d122ff6072941411dc484b0cf2b732b75f13ca0d903253efa6c803d311563  /tmp/vega-r2-thinking-conversation-full-final.log
a946b7523fd9d246231b0dccbd80dc7158fd92d34490ccff40976961caf9e34e  /tmp/vega-r2-thinking-ui-full-final.log
50e1164c4f484797bb748ae6703fe8f8d52d787a424689ae2a1ef43f7cd2c3b7  /tmp/vega-r2-thinking-app-full-final-2.log
```

## 专用整合复核

主 Agent 在 `vega-thinking-integration` 从 `c09ded4` 建立独立整合树，依次保留
SDD、实现和交付记录提交，并合入 R1/R4b/R5/R6/R7。整合阶段补齐了两项跨卡边界：

- run worker 接收 controller 的 owned config path，在 provider construction、Keychain
  或 HTTP 之前同时校验 frozen `(provider, model)` 与真实唯一 `(provider, model)`；
  A→B 同模型的 owner 变化在真实 worker 路径上 fail-closed，测试不读取宿主配置。
- Provider `SettingsSaved` 在 reasoning save pending 时只合并一次 refresh intent；
  reasoning ack 后再启动 catalog reload。失败 ack 的 typed error 和精确 owner draft
  穿过 fresh catalog，Loading/Ready 投影不会把失败编辑清掉；reasoning 生成号在发布
  Ready 前推进，连续 Settings 保存均可 ack。

整合树的首轮失败仍保留：

- `/tmp/vega-thinking-integration-check-1.log`：app 测试调用点尚未补新增 reasoning
  参数，4 个 test 编译错误；
- `/tmp/vega-thinking-integration-check-3.log`：首次补测试时缺少测试可见性/import，6
  个 test 编译错误；
- `/tmp/vega-thinking-integration-fmt-check-1.log`：新增测试的一处 rustfmt 差异；
- `/tmp/vega-thinking-integration-clippy-1.log`：`collapsible_if` 在 `-D warnings` 下
  失败。上述问题均已修复，未用 lint 抑制掩盖。

整合后的关键证据（均使用
`CARGO_TARGET_DIR=/Users/puzige/Workspace/worktrees/vega-r2-thinking/target`）：

| 检查 | 结果 | 原始日志 |
| --- | --- | --- |
| app model-selection/controller（拆分前） | 9 passed | `/tmp/vega-thinking-integration-model-selection-3.log` |
| app 全量 | 58 passed | `/tmp/vega-thinking-integration-app-full-1.log` |
| UI reasoning | 3 passed | `/tmp/vega-thinking-integration-ui-reasoning-1.log` |
| UI 全量 | 135 passed | `/tmp/vega-thinking-integration-ui-full-1.log` |
| store reasoning | 7 passed | `/tmp/vega-thinking-integration-store-reasoning-1.log` |
| runtime agent/reasoning | 39 passed | `/tmp/vega-thinking-integration-runtime-agent-1.log` |
| runtime OpenAI/loopback | 31 passed | `/tmp/vega-thinking-integration-runtime-openai-1.log` |
| `cargo fmt --all -- --check` | 通过 | `/tmp/vega-thinking-integration-fmt-final-3.log` |
| `cargo clippy --all-targets --locked -- -D warnings` | 通过 | `/tmp/vega-thinking-integration-clippy-3.log` |
| `cargo check --all-targets --locked` | 通过 | `/tmp/vega-thinking-integration-check-final-1.log` |

这些 app/UI 验收均通过 owned temporary config/data/DB 和 MockProvider；没有真实
provider、Keychain、网络 API、性能 bench 或 soak。Git service/test 中新增的
`reasoning: None` 仍只是请求字段的编译适配，不改变 Git 行为。

## Stale-generation race follow-up

在 `57f9d8d` 后的补丁中，app tests 按职责拆分：纯 model-selection 文件为 579 行，
reasoning/catalog controller 文件为 893 行，均低于仓库的 1000 行限制。新增回归覆盖
两个 gated worker 未返回窗口：Provider `SettingsSaved` 的非 pending refresh，以及
失败 reasoning ack 触发的 fresh catalog refresh。两者都在真实 Settings action 路径
上确认 Loading 期间不会创建 stale Saving；worker 返回后使用新 generation 完成保存。
失败路径同时确认 typed Conflict 在 fresh catalog 后仍可见，并能用同一精确 owner
重试成功；UI 单元测试确认 Loading 不清除 draft。

| 检查 | 结果 | 原始日志 |
| --- | --- | --- |
| app reasoning/controller race suite | 8 passed | `/tmp/vega-thinking-integration-race-reasoning-1.log` |
| app test compile after module split | 通过 | `/tmp/vega-thinking-integration-race-check-2.log` |

本轮仍只使用
`CARGO_TARGET_DIR=/Users/puzige/Workspace/worktrees/vega-r2-thinking/target`；没有
性能测试、真实 provider、Keychain 或网络请求。

## Root integration sync

本整合分支随后合入 root 联合树 `050d94a`（其祖先包含 `260cf47`、`da983bf` 和
R7 原生 Models 2–4 行布局修复），并保留权限入口、FileIndex、R5 原子 Finished
终态以及 R2 reasoning guards。合并后的真实测试结果为 app 60/60、UI 136/136；
UI 结果包含 `provider_models_frame_reserves_rows_and_keeps_tail_visible`，app 结果
包含 `production_agent_start_entry_surfaces_write_permission_and_continues`。

合并后严格门禁日志：

- `/tmp/vega-thinking-integration-post-merge-reasoning-1.log`：reasoning/controller 8/8；
- `/tmp/vega-thinking-integration-post-merge-app-full-1.log`：app 60/60；
- `/tmp/vega-thinking-integration-post-merge-ui-full-1.log`：UI 136/136；
- `/tmp/vega-thinking-integration-post-merge-fmt-1.log`：fmt 通过；
- `/tmp/vega-thinking-integration-post-merge-clippy-2.log`：`clippy --all-targets --locked -- -D warnings` 通过。

post-merge clippy 的首轮 `collapsible_if` 失败保留在
`/tmp/vega-thinking-integration-post-merge-clippy-1.log`，已按建议收敛条件并重新
通过。整合树仍未 push 或修改 root 联合 checkout。

## 联合首轮计数污染与修复

root 联合 workspace 首轮全量日志
`/private/tmp/vega-main-review-20260905/final-combined-workspace-1.log` 保留了唯一
失败：`pricing_settings_and_agent_preflight_production_e2e` 的全局
`AGENT_WORKER_STARTS` 断言观察到 `left 16, right 15`。复核整合树确认该计数在
`run_agent_worker` entry 使用单一进程级 atomic；R2/R5 新增的真实入口测试与 gpui
并发执行会改变它，pricing 的 provider request 计数虽仍是该测试自己的 MockProvider，
却不能修复 worker 计数污染。model-selection 同样使用了该全局计数。

本轮修复将计数替换为 `cfg(test)` 的 `VegaWindow` owned
`AgentWorkerStartProbe`，由实际 spawned worker 传到 `run_agent_worker`，并在 worker
entry 处递增；pricing/model-selection 断言读取自己的 probe。原有拒绝路径仍断言
零 worker 与 MockProvider 零请求，成功路径仍断言一次 worker 与一次请求；没有重置
全局计数、串行化 suite 或删除断言。独立 target 验证：

- `/tmp/vega-thinking-integration-worker-probe-pricing-1.log`：pricing E2E 1 passed；
- `/tmp/vega-thinking-integration-worker-probe-model-selection-1.log`：model-selection
  E2E 1 passed。

本轮同一独立 target 的 app 全量为 60 passed；fmt、`cargo check --all-targets
--locked` 与 `cargo clippy --all-targets --locked -- -D warnings` 均通过，日志分别为
`/tmp/vega-thinking-integration-worker-probe-app-full-1.log`、
`/tmp/vega-thinking-integration-worker-probe-fmt-1.log`、
`/tmp/vega-thinking-integration-worker-probe-check-1.log` 和
`/tmp/vega-thinking-integration-worker-probe-clippy-1.log`。

本轮日志 SHA-256：

```text
3e76adf5ed8d660916c4b81a8887c7d8f5815c07991f507d8fc1fc489fb673c1  /tmp/vega-thinking-integration-worker-probe-pricing-1.log
62876b8b06ee3a1edf8269f623a96eb1fb77217b81e399dc0917556d394ca2e7  /tmp/vega-thinking-integration-worker-probe-model-selection-1.log
52d6bd0d1edbd31b5d452edce69233e27e6a3db909f99965dcab12b8ca15ff4a  /tmp/vega-thinking-integration-worker-probe-app-full-1.log
cd8e9edb81d61165ace399931cc204f5d04cc3095abdf7f709ceae122e2c012e  /tmp/vega-thinking-integration-worker-probe-clippy-1.log
a327b73f9eff942b935929075186a5b5f081cb6012a9265226c0690caad93caf  /tmp/vega-thinking-integration-worker-probe-check-1.log
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /tmp/vega-thinking-integration-worker-probe-fmt-1.log
```
