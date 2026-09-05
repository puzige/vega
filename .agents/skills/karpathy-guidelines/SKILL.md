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
   exec-guide 的「测试断言原样」「API facade 冻结」一致；孤儿清理仅限自己的
   变更引入的死代码。
4. **Goal-Driven Execution 对应验收协议**：成功判据 = exec-guide §7 的验收命令
   全绿（fmt / clippy / test / build 及任务卡规定项），并附原始输出。
5. **遇阻即停**：本技能 "ask when unclear" 在 Vega 落地为 exec-guide §6 的
   `[BLOCKED]` 上报格式，不自创方案绕过。
