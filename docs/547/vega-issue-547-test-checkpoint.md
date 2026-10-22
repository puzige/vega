# 先行测试准备检查点

## 2026-10-22 / 生产未改

- 基线 `871f91f246e90753743a256cee1eec315e74effc`。
- 规格 v1.1，`vega-issue-547-image-context-budget.md`，SHA256 `9322777d2c531f54668b7f38a5901353e120f3b540f5b72ab5d43ba814bf36d0`。
- 首位测试写入者在完成前中断，已确认停止。唯一测试产物为 `crates/vega_runtime/tests/image_context_budget.rs`，905 行，SHA256 `c3b6327c29241ca05aea4d4209c45581aba85ee2d169ce27208c585f16921084`。文件保留，不重启已完成工作。
- 未执行本地测试/编译/应用，未提交云端 CI；没有有效 RED，没有生产修复，也没有冻结测试。

## 主 Agent 检查与责任层

1. 规格在测试准备期间由 v1 收紧为 v1.1：仅精确 `[System, User]` 且无工具 metadata 属于本卡初始形态，空历史不纳入。现草稿仍包含 system-only 断言，须在冻结前按该验收调整收敛；不是为了实现转绿删断言。
2. 两图片发送用例将 provider 输出 cap 断言为 128000，但 request fixture 明确设为 128，生产 `request_output_cap` 使用 `min(cap, reserve)`。这是测试接线错误，正确值为 128，不改产品 cap 语义。
3. v1.1 明确的 preflight sink 期间取消、无工具 metadata 判定边界尚需核对；A10 错误提示测试尚无产物。
4. A11 历史图片/v2 checkpoint 已由另一写入者独立处理，仅允许 pipeline 测试接线与新测试文件，不与 runtime/error-message 范围重叠。

## 固定 runtime 草稿的独立审查

上述 `c3b6327c...` 草稿静态审查尚不能冻结。除主 Agent 已记录项外，需补齐带 hook 的触发阈值正常发送、User 工具 metadata 排除、已进入 forced recovery 后的可恢复失败仍终止/不减小或高于 target 的结果拒绝、无 hook 及已有流进展不恢复等边界。现有 Ask-mode 预算、几何值与 replay fixture 未发现其他确定的 API 不匹配；编译与行为未执行。

接替的专职写入者从该草稿继续，仅允许 runtime 测试和 A10 错误映射测试，不改 A11 或生产行为。A11 独立写入者已完成并停写。

## A11 READY 与独立静态审查

- `crates/vega_conversation/src/agent/pipeline.rs` SHA256 `0db48c373dd4719a0d61cf79068f8609db18454782ea8178f7f0b2f8130a83ca`：仅追加测试模块声明。
- `crates/vega_conversation/src/agent/pipeline_image_tests.rs` SHA256 `ee42b938990d51977c63f3fa5b74da2c061d88fe57051e5896d2d96b68b98bc4`：3 个测试。
- 写入者完成 scoped rustfmt、静态/差异检查；非写入者核对上述精确内容，无 must-fix。
- 编码体积不同但像素相同的保留行为预期 baseline RED；连续最新完整组/顺序与 v2 SQLite 读回为正向对照。确实经现有 API 安装/加载 checkpoint、运行真实 projection 再读回完整 checkpoint/ContextSource；不冒充进程重启或全库所有表无变更验证。
- 未编译、未执行；这是测试审查 READY，不是 GREEN。

## runtime/A10 修正 READY

写入者已停止，主 Agent 读回哈希并再次执行 scoped rustfmt `--check`、`git diff --check`，均无错误。新增测试无本地执行。

| 文件 | SHA256 | 静态测试入口数 |
|---|---|---:|
| `crates/vega_runtime/tests/image_context_budget.rs` | `cd341ae8023aa5b349a3434f26ebcb630f71d901b0b30181767fc4db33936abd` | 36 |
| `crates/vega_conversation/tests/context_over_limit_message.rs` | `3c76c9cf630cf37f97ea6b371ded641cb1793a39a134f279535edf1d57cf8bce` | 1 |

移除 2 个 system-only 测试及循环中的空历史形态，与 v1.1 收紧的范围一致；cap 期望修为 128。保留原有单 User/历史/图片校验用例，新增 8 个 runtime 测试函数覆盖审查要求。forced-recovery fixture 使用 `ReplayFrame::new(2, ...)` 对应 hook 的 history-only `Some(1)` 加 system 项，确保 unchanged 投影的冷/热边界一致。A10 通过公开 `RunFailureKind::from_runtime(...).message()` 验证，无生产接线改动。

合计 40 个静态测试入口，非 Nextest 发现/执行数；实际运行计数待 Actions。

## 下一步

对修正后固定版本做独立增量审查，A11 未变结论复用。所有先行测试完成/停写、审查无缺口后才提交 draft PR，用现有 Actions 取得有效行为 RED。不得把编译/接线问题当成 RED；任何测试改动均在冻结前留证。
