# Vega 问题与需求收集工作流

本页约定如何把发现的问题留下来，尤其适用于当下没有人或 Agent 能立即修复的情况。登记事项不等于已排期、已开始或已验收。

## 信息各放哪里

| 内容 | 唯一权威位置 |
|---|---|
| 产品目标、取舍和路线图 | [Notion PRD](https://app.notion.com/p/peanut996/Vega-Native-AI-Agent-Desktop-PRD-3caae0ca542b8056b086ee2e990d9b8a)；[本地同步副本](vega-prd.md)仅供离线阅读 |
| 可执行的缺陷、需求和复现证据 | [Vega GitHub Issues](https://github.com/puzige/vega/issues)；一个独立问题对应一个 Issue |
| 排队、处理和验收状态 | [Vega Desktop GitHub Project](https://github.com/users/puzige/projects/2/views/1?system_template=kanban)；不再维护第二套可编辑的 Notion 待办看板 |
| 实现契约、任务卡和验收规则 | 本仓 `docs/`、[执行总纲](vega-exec-guide.md)；Issue 不代替 spec |

Notion PRD 可以链接 GitHub Project，但两边不要各维护一份状态或优先级。此 Project 属于 `puzige` 个人账号；仓库的 Projects 页未必显示它。执行时通过 GitHub CLI 查询当前条目、字段及权限，不依赖静态条目数量。

## 收集：先留下证据，不强迫当场修

1. 用户明确要求记录、追踪或以后处理 Vega 的问题/需求时，先搜索开放和已关闭的 Issue，优先补充已有 Issue，避免重复。仅请求解释、诊断或评论时，不擅自创建外部事项。
2. 没有对应 Issue 时，记录能确定的最小事实：简短标题、实际与期望、复现步骤或触发条件、Vega 版本/commit、影响范围、截图或有界日志、来源对话/PRD/spec 链接。未知项标为「待确认」，不可编造。
3. Issue 创建后，关联到 Project 并设为 `Backlog`。没有修复资源时仍保留 Issue，并向用户返回链接、当前状态和下一步；不要设置虚假的负责人、期限或 `In progress`。
4. GitHub/Project 不可用或权限不足时，如 Issue 仍可创建，就先保存 Issue 并说明「待加入看板」；若连 Issue 也无法创建，交付可直接粘贴的 Issue 草稿和实际阻碍。不要声称已保存或已同步。

`puzige/vega` 是公开仓库。Issue、附件、日志和截图不得包含 API key、凭据、未脱敏的用户资料或敏感本地路径；必要时只提交脱敏摘要，私密证据另行向用户确认存放位置。

## 分流与交付

执行卡片必须使用 [vega-kanban-delivery skill](../.agents/skills/vega-kanban-delivery/SKILL.md)：需求分析 → 测试用例 → 实现计划 → 实现 → 真实 E2E/本地截图验收 → 合并 master 并验证 → 清理本卡本地分支/worktree → 回写上下文 → 关闭 Issue/Done。互不影响的卡可并行实现，构建按 worktree 隔离并通过调度器限制并发，旧测试资源和原生 UI 验收仍须互斥。连续取下一张仅限用户已授权连续执行时。

- `Backlog`：已记录，尚待分流或补规格；即使暂时没人修，也留在这里。受阻项在 Issue 中写清阻碍和下一责任方。
- `Ready`：优先级、影响模块、复现/验收条件已明确，并在 `docs/` 找到或补齐相应规格与任务卡。产品取舍冲突回到 Notion PRD/用户裁决；遵守 `AGENTS.md` 的 spec-first 和卡外先问规则。
- `In progress`：只有实际开始工作才进入。实现使用独立分支/worktree；PR 关联 Issue 和 spec，附真实测试及 E2E 证据。
- `In review`：代码或方案已提交审查/验收，但尚未完成；不能因为代码已写完就标 `Done`。未通过则留在此列或退回并写明失败证据。
- `Done`：适用门禁及真实验收通过、证据持久保存、master 集成验证、本卡本地分支/worktree 清理、上下文回写均完成后，才关闭 Issue 并标记。任何一步受阻都不算完成；状态更新后重新查询确认，向用户报告真实链接。

`Priority` 现有选项为 `P0`、`P1`、`P2`，`Size` 为 `XS`、`S`、`M`、`L`、`XL`。有足够依据再填写；没有依据就留空或标明待评估，不凭空承诺工期。不要为匹配文档擅改 Project 字段或列。

## 使用 GitHub CLI

在已获相应权限的环境中，优先用 `gh` 查询和更新，不需要用户手工维护本地配置文件：

```bash
gh issue list --repo puzige/vega --state all --search '<关键词>'
gh issue create --repo puzige/vega --title '<标题>' --body '<脱敏的复现与验收信息>'
gh project item-add 2 --owner puzige --url '<Issue URL>'
gh project item-edit 2 --owner puzige --url '<Issue URL>' --field Status --value Backlog
gh project item-list 2 --owner puzige --field Status --field Priority
gh project field-list 2 --owner puzige
```

变更状态时，把 `Backlog` 换成上述实际列名。创建/编辑前先核实 Issue、权限和目标 Project；命令示例不是授权 Agent 在仅被要求分析时自动创建 Issue。

Project API 需要 GitHub `project` 权限。Agent 不得索取或记录 token；权限由用户在 GitHub 授权流程中授予。缺少 Project 权限时按上面的降级路径办理。
