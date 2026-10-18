---
name: karpathy-guidelines
description: Behavioral guidelines to reduce common LLM coding mistakes. Use when writing, reviewing, or refactoring code to avoid overcomplication, make surgical changes, surface assumptions, and define verifiable success criteria.
license: MIT
---

# Karpathy Guidelines

Behavioral guidelines to reduce common LLM coding mistakes, derived from [Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876) on LLM coding pitfalls.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## Vega 适配（本仓库优先级裁定）

本技能是**行为准则**，不覆盖、不替换 Vega 仓库的任何既有硬性规则；冲突时一律以
Vega 规则为准。具体边界：

1. **Spec 先行不变**：任何实现必须对应 `docs/` 中的任务卡与 spec 章节
   （AGENTS.md「最高原则：SDD」）；卡外工作先问，本技能的 "Think Before Coding"
   不构成跳过任务卡的理由。
2. **执行宪法优先**：[docs/vega-exec-guide.md](../../../docs/vega-exec-guide.md)
   的红线（依赖白名单、fail-closed、验收协议 §6/§7）优先级高于本技能；
   "Simplicity First" 不得作为省略 fail-closed 校验、验收证据或门禁的理由。
3. **Surgical Changes 对应本仓纪律**：不顺手重构、不改无关行——与
   exec-guide 的冻结测试目标/预期与 API 边界纪律一致；孤儿清理仅限自己的
   变更引入的死代码，或当前用户明确授权的 Rust 测试清零范围。
4. **Goal-Driven Execution 对应当前 TEST-FIRST**：遵守
   [Rust 清零/测试先行规格](../../../docs/vega-rust-test-reset-tdd.md) 和
   [exec-guide §7.2](../../../docs/vega-exec-guide.md#72-test-first-与有效测试当前规则)。
   有行为变化时，先冻结验收并在生产未改时写齐测试，交 Actions 确认目标行为缺失的
   有效 RED，再冻结测试目标/预期并写最小实现，验证同一测试 GREEN 与最新 head 门禁。
   无关编译、依赖或 runner 故障不是 RED；最小接口缺失只标接口缺失 RED，不能称行为断言已执行。
   实现阶段及完成后禁止补测、按实现输出反推断言、放宽/改期望、ignore 或重试换绿；
   新需求/遗漏边界/新 Bug 先停止本轮、记录验收变化并重新先测后修。测试错误/接线修正
   先留证审查，不得改变验收含义。测试须验证稳定边界的行为/业务/安全结果，不能只查源码、
   mock 调用、镜像算法或内部字段覆盖率；调用次数仅在本身属于契约且结合结果/终态时有效。
   mock 不替代被测业务/安全决策；新测试用行为命名，issue/task 编号只留文档。
   纯文档/格式无行为变化不强加测试，纯重构保留已有先行测试；本次清零不人为造测试/RED，
   保留 Python 发布/组件准备/delta 测试及 fixture，历史报告不是当前验证。
   旧分支先同步最新 master；保留有效测试不包括从旧 ref 整目录/整仓恢复已清除的历史集合。
   逐项核对新增测试和必要支持的来源；历史测试改名/改写不自动算新增，确需新回归独立先测后修。
   合并前复核删除结果和最终差异，见 [合并纠偏规格](../../../docs/vega-rust-reset-reconciliation.md)。
   成功判据还须附 RED 基线/测试补丁/失败原因、冻结目标、后续实现顺序与同一测试 GREEN
   的可访问原始证据，squash 后仍可审查；最终绿色 CI 不自动强制或证明 TDD。
   本地只做低 CPU 静态/格式检查，不运行 Vega、不编译/构建/打包/测试；RED/GREEN、Clippy
   和完整门禁只交 Actions，打包仅云端发布，不因旧任务授权扩大本地权限。实际 PR gate 保留
   fmt / 全目标 Clippy / 三组 Python 检查 / Nextest，未运行写待 CI，缺失计数写未取得；
   日志/摘要中的 `no Rust tests` 表示 0 发现/0 执行，不是 Rust 测试通过。
5. **遇阻即停**：本技能 "ask when unclear" 在 Vega 落地为 exec-guide §6 的
   `[BLOCKED]` 上报格式，不自创方案绕过。
6. **代码禁止任何注释**（2026-09-23 用户裁决，exec-guide §3/§4）：`///`、`//!`、
   行内 `//`、`/* */` 一律不加。需要注释才能读懂，说明命名或结构没写清楚——改命名、
   拆函数、提类型。唯一例外：`unsafe` 块紧邻的 `// SAFETY:` 一行。本技能
   "Don't improve adjacent comments" 指不改他人已有的无关注释，不等于可以新增注释。
