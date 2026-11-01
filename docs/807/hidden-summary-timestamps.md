# 隐藏任务统计摘要与时间比较

## 用户反馈与本轮修订

用户在包含 #808 的版本中反馈：8:31 正常回复，10:33 再发送仍无时间分隔。本轮修复隐藏 TaskCostSummary 打断比较，并排查其它不可见投影。基线 `81aebf4db1821f6868aceec8c2068fa8522a3f14`。本节替代原规格中“零行 Summary 仍打断”的契约及对应旧测试预期，不恢复旧测试集合。

## 验收与实现

1. 正常回复经 MessageFinished 和 apply_task_summary 后，Completed Summary 为零行，透明跳过，不更新 previous、seen、segment_owner、saw_user，不生成统计条目前缀或使用统计时间。下一用户与上一助手的权威时间差严格大于一小时生成一次前缀；等于或小于不生成。助手间严格大于十分钟规则不变。
2. 实时完成、历史恢复和跨页加载、重复重算均使用同一规则。测试调用真实事件处理、apply_task_summary、apply_history_page/apply_newer_history_page 和 refresh_timestamps，不用镜像算法。
3. 可见 Failed/Interrupted Summary 仍是独立断点。ContextCompaction 即使零行仍打断，显式边界/Fork/续接、缺失时间不能因跳过统计而被跨越。Subagents 保留 #808 透明规则。
4. 第一遍继续排除 Summary，统计摘要不成为时间来源或 first_visible。第二遍在 owner/断点处理前跳过零行 Summary，保留其它判断。

## 不可见条目排查结论

| 条目 | 当前处理及本轮结论 |
|---|---|
| Completed TaskCostSummary | 零行但原先 reset；本轮修复为透明跳过 |
| Subagents | 无论可见与否均已透明跳过，不改 |
| ContextCompaction | Succeeded/Cancelled 零行，以及重复投影隐藏；代表真实压缩边界，保留断点，不按可见性一律跳过 |
| RunActivity / RunActivitySegment / Tool | 运行标题可折叠隐藏、分段可零行、子任务工具可隐藏；有 owner，使用相同逻辑消息去重，不新增时间；不改 |
| Assistant 空正文或隐藏过程正文 | 保留消息 owner 和缺失时间语义；不能因不可见而跨缺失时间，不改 |
| 空 User 正文 / UserImages | 同一用户主体；仅图片仍比较一次，不按空文本丢失主体，不改 |
| ArtifactGroup | 无可见文件时零行；所属助手投影不独立计时，不改 |
| Permission | 处理后 remove_active_permission 实际移除条目，不留下隐藏卡；仍显示的无归属权限保留独立边界，不改 |
| Plan / SkillActivation / Screenshot / ToolGroup | 正常 row_count 至少一行或为一行，不属于此次隐藏断点缺陷 |

范围仅时间分组及必要回归。不改数据库、权限、生命周期、UI 文案、复制或日期格式，不新增生产依赖。检查依据：model::StreamEntry::row_count、phase::process_entry_hidden、content 的身份/权限移除/摘要接入及 context_control 的压缩投影。

## 验证边界

本地只做快速格式、差异与静态检查，不执行 Vega、编译或测试。新增开发 Rust 回归只由 PR Check 运行，原始源码、完整 SHA、指纹及实际云端结果留痕后按现行政策移除测试及独占接线；最终无 Rust 测试 head 再跑完整 required CI。独立审查与用户手测不可由空集门禁替代。
