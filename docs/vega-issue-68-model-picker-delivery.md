# Issue #68 · 无思考档位模型选择入口交付记录

> 状态：自动化验收通过；打包与原生窗口验收待完成。
> 规格：[vega-issue-68-model-picker.md](vega-issue-68-model-picker.md)。
> 原始日志与截图保存在 worktree 外的本地证据包 `vega-issue68-evidence`；仓库仅记录脱敏摘要和 SHA-256。

## Freeze

- 验证日期：2026-09-21（Asia/Shanghai）；最终原生验收时间待补。
- 基线：`origin/master @ 8c1d874`；任务分支：`feat/issue68-model-picker`。
- 代码与测试 diff SHA-256（不含文档）：`2174141c692ba9a8883336578aa7f17eb8454147dc06747be4a7eef43ae96ea7`。
- 环境：Darwin arm64；Rust 1.98.0；Cargo 1.98.0；Git 2.55.0。
- 任务契约：有档位保留 Slider → List；无可渲染档位直接进入 List；能力刷新关闭失效 Slider；选择保存、键盘操作和现有保护保持不变。

## 测试先行与实现

先在真实 `ConversationStream` Composer 触发器上增加 I68-01 回归测试。修复前测试得到 `Slider`，预期为 `List`，退出码 101，见 `i68-01-red.log`。原 R59 无档位断言改为新规格后也先失败，见 `i68-r59-replaced-red.log`。两份红测均保留。

修复让模型触发器根据当前 slider 投影的 `has_tiers()` 选择打开 Slider 或 List，并复用现有模型高亮和滚动初始化。能力刷新若使已打开的 Slider 无效，会关闭该层。模型列表、选择保存、思考档位和 provider 请求路径没有改变。相对冻结规格无偏离。

## Results

以下 Cargo 命令均经过仓库的 `scripts/cargo-lock.sh`。日志文件名均指向上述本地证据包；括号内为退出码。

| 需求/门禁 | 证据级别 | 实际命令 | 结果与日志 |
|---|---|---|---|
| I68-01 原缺陷红测 | GPUI 生产触发器 | `scripts/cargo-lock.sh test -p vega_ui i68_no_tier_trigger_mounts_model_list -- --nocapture` | 预期失败，0 通过、1 失败（101），`i68-01-red.log` |
| R59 旧断言红测 | GPUI 生产触发器 | `scripts/cargo-lock.sh test -p vega_ui r59_a_model_without_tiers_mounts_no_layer -- --nocapture` | 预期失败，0 通过、1 失败（101），`i68-r59-replaced-red.log` |
| I68-01–05、07 及 R59/R57 回归 | GPUI 生产路径 | `scripts/cargo-lock.sh --wait test -p vega_ui model_picker_levels -- --nocapture` | 26 通过、0 失败（0），`model-picker-final.log` |
| I68-06 模型保存与运行 | E2E-REAL：真实 app handler + owned store | `scripts/cargo-lock.sh --wait test -p vega model_selection_app_handler_persists_and_runs_exact_model -- --nocapture` | 1 通过、0 失败（0），`model-selection-controller-e2e.log` |
| 格式 | 仓库门禁 | `scripts/cargo-lock.sh fmt --all -- --check` | 通过（0），`fmt-check.log` |
| 严格 Clippy | 仓库门禁 | `scripts/cargo-lock.sh clippy --all-targets -- -D warnings` | 通过（0），`clippy.log` |
| 首次全量 | 仓库门禁 | `scripts/cargo-lock.sh --wait test --workspace` | 现有 Diff 用例失败，Vega 包 180 通过、1 失败（101），`workspace-test.log` |
| 首次失败项单独复跑 | 生产测试 | `scripts/cargo-lock.sh --wait test -p vega diff_refresh_intents_keep_content_during_background_and_retry -- --nocapture` | 1 通过、0 失败（0），`diff-retry-isolation.log` |
| I68-09 串行全量 | 仓库门禁 | `scripts/cargo-lock.sh --wait test --workspace -- --test-threads=1` | 35 组共 1646 通过、0 失败、9 忽略（0），`workspace-test-serial.log` |
| I68-08 原生窗口；I68-09 打包 | 原生 E2E / 仓库门禁 | 待运行 | 待验收；不得据此关闭卡片 |

首次全量失败的用例是 `diff_refresh_intents_keep_content_during_background_and_retry`，错误为 `GitFailed`。该用例与模型菜单无关；单独复跑及随后的串行全量均通过。首次失败保留为历史结果，不计为通过。最终改动只有注释在通过上述门禁后作了更新；没有生产逻辑或测试行为变更。

| 用例 | 当前结论 | 对应证据 |
|---|---|---|
| I68-01 | 通过 | 红测、修复后 26/26 定向测试 |
| I68-02 | 通过 | 缺失、不可用、无可用档位的 GPUI 回归 |
| I68-03 | 通过 | 有档位模型的原有两级菜单回归 |
| I68-04 | 通过 | 空列表、保存中和可信操作忙的 GPUI 回归 |
| I68-05 | 通过 | 能力变更关闭失效 Slider 及恢复回归 |
| I68-06 | 通过 | 模型确认后的菜单重开与 owned-store controller E2E |
| I68-07 | 通过 | 真实 GPUI 焦点下 Enter、上下移动和 Esc 回归 |
| I68-08 | 待验收 | 候选构建的浅色、深色原生截图 |
| I68-09 | 部分通过 | fmt、Clippy、串行 workspace 测试通过；打包待验收 |

## 证据校验

| 原始日志 | SHA-256 |
|---|---|
| `i68-01-red.log` | `00c66ba1b4725fc72a17a67a91fac64ff85468042187e9dcff5e0936dfeefe69` |
| `model-picker-final.log` | `b8fbe6387b9707528eac118a36339328a5e7a6b8f8e5713f8e3bafbe01bd6694` |
| `model-selection-controller-e2e.log` | `423a3efd1e05b871e72a617c062ec99fe8bd41872814125982ddcd697fb18904` |
| `clippy.log` | `85e5f2a773c262debb0312bcfeb2b048bb5de1ad2b618da7dd6f3e4c69ac2f0d` |
| `workspace-test.log` | `2309dcfc77152e25b31a779137c7db4d14126df1bec81fecc97645d39484594d` |
| `workspace-test-serial.log` | `adf6c5f50f9767ff44218c11a217dd36abb6345b9dc4ad47273a10765921eecd` |

## Residuals

- **NOT RUN**：候选 App 打包、原生浅色/深色窗口操作与持久截图；完成后补充被测构建身份、截图 SHA-256 和 manifest。
- **ACCEPTED**：仓库原有 9 个 ignored 测试未在本卡启用；模型入口没有新增持久化状态或数据迁移。
- 回滚方式：撤销本卡的 squash commit；用户现有模型和思考偏好数据不需迁移。
