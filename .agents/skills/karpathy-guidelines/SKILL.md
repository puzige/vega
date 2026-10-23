---
name: karpathy-guidelines
description: Behavioral guidelines to reduce common LLM coding mistakes. Use when writing, reviewing, or refactoring code to avoid overcomplication, make surgical changes, surface assumptions, and define verifiable success criteria.
license: MIT
---

# Karpathy Guidelines

Behavioral guidelines to reduce common LLM coding mistakes, derived from [Andrej Karpathy's observations](https://x.com/karpathy/status/2015883857489522876) on LLM coding pitfalls.

**Tradeoff:** Use the smallest change and risk-appropriate verification; avoid process overhead that does not help delivery.

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
- "Add validation" → "Implement validation and verify relevant invalid-input boundaries"
- "Fix the bug" → "Make the smallest fix and choose necessary regression checks"
- "Refactor X" → "Preserve behavior and verify affected invariants according to risk"

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
   exec-guide 的验收与 API 边界纪律一致；孤儿清理仅限自己的
   变更引入的死代码，或当前用户明确授权的 Rust 测试清零范围。
4. **Goal-Driven Execution 对应快速交付**：遵守
   [验证与交付政策](../../../docs/vega-rust-test-delivery-lifecycle.md) 和
   [exec-guide §7.2](../../../docs/vega-exec-guide.md#72-快速交付与按风险验证当前规则)。
   明确验收后用最小改动解决问题，按风险选择必要验证，可直接修复或先实现后补测。
   不强制测试先行、独立测试前置审查、有效 RED、冻结后禁止补测、GREEN 后删测及证据链；
   不为流程造临时测试、先红后绿或删测提交。纯文档/格式不强加测试。
   需要测试时验证稳定边界的行为及业务/安全结果，mock 不替代被测决策；不以源码字符串、
   mock 调用或覆盖率充数。新测试用行为命名，issue/task 编号只留文档。
   不为过关放宽断言、删除失败测试、ignore 或盲目重试换绿；修测试/接线错误须说明依据，
   不按实现输出反推预期。验收变化先更新 spec，不要求重开 TDD 周期。
   不从旧 ref 批量恢复已清除的历史 Rust 测试及支持，不强制新增测试验证后删除；
   保留现存 Python 测试及 fixture、生产/benchmark/通用 CI，历史报告不是当前验证。
   本地只做低 CPU 静态/格式检查，不运行 Vega、不编译/构建/打包/测试；自动测试与完整门禁
   只交 Actions，打包仅云端发布。实际 PR gate 保留 fmt / 全目标 Clippy / 三组 Python 检查 /
   Nextest，最新 head required CI 通过才能合并。记录精确版本、已执行检查和真实结果；
   未运行写待 CI，缺失计数写未取得，`no Rust tests` 表示 0 发现/0 执行，不是测试通过。
5. **遇阻即停**：本技能 "ask when unclear" 在 Vega 落地为 exec-guide §6 的
   `[BLOCKED]` 上报格式，不自创方案绕过。
6. **代码禁止任何注释**（2026-09-23 用户裁决，exec-guide §3/§4）：`///`、`//!`、
   行内 `//`、`/* */` 一律不加。需要注释才能读懂，说明命名或结构没写清楚——改命名、
   拆函数、提类型。唯一例外：`unsafe` 块紧邻的 `// SAFETY:` 一行。本技能
   "Don't improve adjacent comments" 指不改他人已有的无关注释，不等于可以新增注释。
