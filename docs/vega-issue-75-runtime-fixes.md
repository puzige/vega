# Issue 75 — 真机验收缺陷修复

## 范围
用户于 2026-10-12 授权新建工作树修复。延续 #75 的桌面及浏览器执行合同；#307 首次启用引导仍单独跟踪。

## 已确认问题
成功 ComputerUse 历史回读遗漏 truncated=false，导致安全摘要误判为损坏。执行器 MCP isError 的原因被统一错误覆盖，无法区分扩展发现、窗口观察、权限和协议失败。浏览器首次 browser_tabs 失败及桌面选窗后观察失败的具体根因尚未确定，禁止据猜测更改权限或绕过目标授权。

## 要求
1. 成功 ComputerUse 历史重载恢复合法结果；失败与拒绝结果保持原语义，不恢复临时截图或页面内容。
2. 受管调用提供明确 executor、stage、有限 error_class 和可操作的安全提示。不得持久化或向模型输出原始执行器错误中的页面、输入、令牌、用户路径及其他敏感正文；未知错误归类为 unknown，不能误称权限问题。
3. 保持目标授权、租约、接管撤权、结果未知禁止重试等既有安全合同。
4. 根据固定上游源码及新的安全诊断证据修复实际适配失败；新增明确根因时先更新本规格再实现，不猜测性扩权。

## 验收矩阵
- 成功、失败、拒绝 ComputerUse 的持久化历史回读与工具卡投影；成功场景先证明旧代码失败。
- 浏览器连接与桌面观察的分阶段错误映射；恶意或含敏感数据的错误正文不得进入安全结果。
- 动作 outcome_unknown、撤权及不自动重试回归。
- 只跑相关进程内测试，全量门禁由云端 PR check 执行。
- 发布后真实桌面追加文本、浏览器表单填入与核对、接管恢复分别验证；不可用 mock 结果冒充真机通过。

## 实施
专用子任务实现历史回读及安全诊断，主控审查集成。实际适配缺陷在证据充分后更新规格并修复。沿用现有依赖，不新增真实外部 E2E 门禁。

## 实施计划与安全诊断合同

先用真实 Store 写入并关闭重开数据库，验证 ComputerObserve 的 success/failed/rejected 历史投影，再修复成功结果的 truncated 元数据恢复；不恢复图像或观察授权。受管调用按 Cua/Playwright 与 initialize、permissions、discovery、target_list、observe、screenshot、action 阶段分类。错误仅持有有限枚举，Display/Debug/UI/模型/持久化只输出固定代码和固定建议，原始 MCP 文本与 structured 私有字段不进入错误对象。已知上游 structured code 或精确消息格式可映射；其他消息统一 unknown。取消仍沿用既有 epoch 合同；动作调用一旦进入 dispatch 路径，无论后端拒绝、取消或观察失败，均保留 outcome_unknown 与禁止自动重试。

| ID | 风险/操作 | 预期 | 自动验证层级 | 状态 |
|---|---|---|---|---|
| RF01 | 成功、失败、拒绝摘要落库后重开 | 合法 ComputerUse 历史结果，成功不再损坏 | Store + conversation | PASS（进程内） |
| RF02 | 含路径、页面、输入、令牌的未知错误 | 固定 unknown；Display/Debug 均无正文 | runtime 结果分类 | PASS（进程内） |
| RF03 | 固定上游已知错误/传输错误 | executor/stage/class 可区分，权限不误判 | runtime fixture | PASS（进程内） |
| RF04 | 动作失败、撤权、旧观察与租约 | unknown 不重试；既有收口合同不变 | runtime | PASS（进程内） |

定向命令：cargo nextest run -p vega_conversation -p vega_runtime issue75。测试只替换外部执行边界，不启动真实浏览器/桌面驱动。云端门禁与发布后真机验收由主控推进。

## 本轮交付证据

- verified_at_utc: 2026-10-12T03:12:53.182Z
- branch: feat/75-runtime-fixes；基线为已合入 #306 的 master。
- Rust 文件顺序：crates/vega_conversation/src/agent/tests/mod.rs、crates/vega_conversation/src/history.rs、crates/vega_runtime/src/computer_use/backend.rs、crates/vega_runtime/src/computer_use/diagnostics.rs、crates/vega_runtime/src/computer_use/mod.rs。
- implementation_content_sha256: 36be43f55fc4376209338e67095d917eefdfe7854410e631502a18782583a90c（按本文所列五个 Rust 文件顺序串联相对路径与文件字节）
- Rust/Cargo: 1.98.0；自动测试只覆盖进程内合同。
- 代码范围：conversation 历史投影及持久化回归；runtime computer_use 的有限诊断和实际调用路径；无 UI、配置、组件版本、执行器 argv、权限范围变更。

| 要求 | 精确命令 | 原始结果 | 日志 SHA-256 |
|---|---|---|---|
| 首次缺陷复现 | cargo nextest run -p vega_conversation -p vega_runtime issue75_runtime_fixes | exit 100；2 tests run: 0 passed, 2 failed, 872 skipped；失败分别为 Corrupt 和缺少阶段诊断 | f88a2ca940adcf85398955320553d55934384a5393ab99a7a834d854bbce163d |
| 最终本卡回归 | cargo nextest run -p vega_conversation -p vega_runtime issue75 | exit 0；Summary [2.417s] 25 tests run: 25 passed, 853 skipped | 60242a25f0aa34d6bb9bb650663ea0a3be6da63ac2d18c2a658c6711ac05b4e5 |
| 文本检查 | git diff --check | exit 0；无新增代码注释 | 无 |
| 定向格式 | rustfmt --check --edition 2024（上述五个 Rust 文件） | exit 0 | 无 |

原始日志保留在本次会话的 /tmp/vega-75-runtime-fixes-red.log 和 /tmp/vega-75-runtime-fixes-final.log。全量 fmt/clippy/nextest 未在本地运行，交由云端 PR check。只读独立审查未发现新增 P1/P2。

固定来源：[Playwright extensionContextFactory](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/mcp/extensionContextFactory.ts)、[Cua get_window_state](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs)、[Cua refusal](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs)、[Cua background input](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/background_input.rs)。只解析固定 code 或精确消息格式，不复制原始建议，不自动切换输入方式。

### 尚未验证

NOT RUN：本修复版真实桌面追加文本、浏览器表单输入、接管后继续。此前真实 browser_tabs/get_window_state 失败的确切原因尚未确定；本补丁先修复历史显示和诊断丢失，不能据此声明实际适配成功。待发布后用同一授权目标复测；按新 error_class 确定根因后先修订规格，再实施必要适配。
