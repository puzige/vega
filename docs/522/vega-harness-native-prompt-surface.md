# 原生 Harness² 提示模板兼容修复

## 基线、授权与失败证据

本修复属于仍在 In progress 的 #522，不改变日常 Vega 策略或 App。2026-10-21 用户明确本卡研究工具及实验可以本地运行，并要求清理服务器；服务器临时构建资源和两把临时 CPA key 已清理，保留原服务和统计历史。模型固定 `gpt-6.1-sol / max`，用户授权 Agent 决定费用；当前采用 $10 观测停止线，无货币硬配额。现有独立审查和完整 PR Check 保持不变。

生产未改基线为已合并 `e484e3a02d8111b3a651ab33f896a31a74cf94ab`。本地四个 native optimization 样本通过，CPA 25 次请求与原生请求、输入/输出 token 完全一致，费用按 CPA 配置核算 $0.076632；独立 OpenCode 探针 38.8 秒、4 次请求，全为指定模型/max，CPA $0.0199016。两者分别证明 worker 和通道可用，没有证明完整搜索可用或取得优化收益。

第一次真实搜索在 `run_sweep → recursion._run_stage → prompts.propose → _st(surface)` 失败：`no prompt surface for 'vega_native'; known: ['codex', 'opencode']`。发现后协作取消，保留四次原生 trial（2 End、2 Cancelled）和全部失败证据；CPA 本轮 22 次记录含 2 次失败，观测费用 $0.0622044。取消 trial 原生费用未知仍保留未知，不把取消当作完成。

固定 Harness² `1cfe254e8d25b15f37048c3730134686abe60335` 的 `SurfaceSpec` 定义文件与触发边界，`BenchPrompts` 不提供新的 surface 注册点；`prompts.py` 静态只认识两种内置 surface。已有 controller CI、native smoke 与通道探针没有覆盖这条真实提示构造路径。

## 验收行为与证据

| 前提与操作 | 可观察结果 | 验证 |
|---|---|---|
| 未安装原生提示兼容补丁，执行搜索 preflight | 在任何 native/model session 之前明确拒绝；无收费调用，不返回 ready | 先行 Python 行为测试，现有 Actions RED/GREEN |
| 精确固定上游与经审查的原生模板补丁，执行 preflight | INV、DOM、aggregate、compose 的真实模板均可零调用渲染，明确 `vega_native` 和实际允许的文件/工具 | CI 中隔离外部依赖的契约测试；本地固定上游零模型研究检查，分别标注证据 |
| 上游存在其他 tracked/staged/untracked importable 修改，或兼容补丁字节改变 | 拒绝；不以“允许脏源码”绕过原有来源保护 | Python 边界测试，Actions |
| 合法补丁下启动实际搜索 | 原生 worker identity 不变，仍用真实 Vega Runtime；只允许 strategy.md 改动，三份已批准技能字节固定；上游递归/选择/反思算法不变 | 真实研究原始 manifest、trial、channel audit、CPA 对账 |
| 记录研究来源 | 显式记录上游基准 SHA、兼容补丁 SHA 与变更后源码身份；不声称上游完全未修改 | preflight/report 来源字段及静态审查 |

测试仅隔离外部 Git、进程与模型依赖，不伪造真实上游已渲染或真实模型收益。新增 Python 测试保留；本次无 Rust 实现/测试、App UI 或 workflow 修改，不新增真实外部 E2E 自动门禁。

## 最小实现计划

1. 生产未改时先写齐上述 Python 行为/边界测试，由非实现者审查后，现有 Draft PR Check 取得有效 RED，再冻结目标与预期。
2. 在 `research/harness2/` 保存可审查的精确上游兼容补丁，仅新增 `vega_native` 的提示数据（surface_md、inv、dom、tts_surface）。准确说明 strategy.md 为唯一可编辑 always-on；技能经原生 load_skill 激活，三份技能本轮不可改；不支持脚本、插件、hooks 或子 Agent。不得借用 OpenCode/Codex 名称、偷偷修改私有全局表，或引入通用注册平台。
3. 在 `scripts/harness2_vega_adapter.py` 仅允许该固定上游的精确补丁差异，保留对其他源码、阶段区和可导入未跟踪文件的拒绝；不自动安装或执行外部框架。导入真实 prompt 模块并在 preflight 提前渲染所需模板，输出兼容来源信息。
4. 非实现者独立审查实现与冻结测试，现有完整 PR Check GREEN 后按原门禁合并。本轮 Python 测试保留；最终仍需明确 Rust 0 发现/0 执行，不以空集代替 Python 行为验证。
5. 明确应用已批准的补丁，在本地真实固定上游完成零模型检查，再用新 output namespace 重启优化集搜索。原失败研究目录不覆盖；冻结候选后再执行留出配对，不自动采用到日常 Vega。

回滚为撤销本兼容修复和研究环境中的精确补丁；原 App、用户配置、CPA 服务与数据库均无迁移。

## 先行验证记录

- 测试提交 `8125c8934e908fee59eb83cac828a0bb32f37b3a`，5项Python测试已于生产未改时完成并独立审查；冻结SHA256 `99904883886b27b5a257d8784de887e1ea899e3eecdd37f449bb7964508624ab`。
- [有效RED39630336171](https://github.com/puzige/vega/actions/runs/39630336171)：主组40项执行，5个模板异常子场景 `ValueError not raised`；另9个缺helper接口错误、1个旧来源准入拒绝，原始输出 failures=5/errors=10。其它5+15项Python通过，Format/Clippy成功，Rust步骤跳过而未发现/执行。原始来源与patch固定于[PR531](https://github.com/puzige/vega/pull/531)；此后最小实现不得改冻结测试。

## 研究环境应用与真实零模型检查

固定上游 HEAD 后，显式应用本仓库补丁；不在加载器内自动安装、改源码或修改私有注册表：

```sh
git -C /isolated/harness2 apply --check /path/to/vega/research/harness2/patches/vega-native-prompt-surface.diff
git -C /isolated/harness2 apply /path/to/vega/research/harness2/patches/vega-native-prompt-surface.diff
```

之后使用全新 Python 3.12/3.13 进程。补丁 SHA256 为 `8db17ca7306a24cd954062bba4609a2551d495a195eecf58a3f4c8e1cff70d6d`，补丁后实际源 SHA256 为 `b6f7389ed27dd10f3d921d6d672ae186aef46562b68fb56c78ba5de6f0f017f8`。上游原始 SHA 与变更来源分别记录，不把批准补丁伪称未修改源。

安装器生成的 `dependencies/` 只从未跟踪 status 清点中排除，由 OpenCode/Bun 各自的固定版本、官方兼容补丁与执行文件身份单独核验；全仓 staged 与精确 tracked diff 检查仍保留，其它未知源码不放行。

2026-10-22 本地真实研究检查已通过：全新进程、无模型凭据，实际固定上游和批准补丁下调用真实 bridge preflight，INV/DOM/aggregate/compose 四模板均完成构造，长度分别为 7245/7356/5486/4044 字符；模板集 SHA256 `c804c01367d301ebc6c548e791464a7bcae804afdfaed9a9995cc568c26095b5`。原生 worker 仅结构性 preflight，live_calls=0、transport_verified=false，不创建 study runs 目录，约299毫秒；这不是新的模型效果或传输验证。

补丁和实现已由非实现 Agent 独立静态审查；五项先行测试及旧 controller/测试字节未改。完整同测试 CI GREEN 仍须按 PR531 的最终 head 记录核实，真实优化与留出收益仍待研究结果。
