# Vega Review 后续任务

2026-09-05 · Owner Codex · Implementation Luna / max

此文件承载本次用户追加的明确任务卡，不替换 S1–S8 历史记录。当前交付与下一步见 [vega-review-current-status.md](vega-review-current-status.md)。

## R4 · 客户端 UI 翻新（LOCAL VERIFIED；总门禁未放行）

- 来源：用户要求参考 Codex / ChatGPT 客户端翻新 Vega UI。
- 前置：基于当前 master 的隔离分支；R1 模型修复另行并行实施，集成时保留全部状态与 guard。R0 阻止最终门禁放行，不阻止独立视觉开发。
- Spec：[vega-ui-refresh-sdd.md](vega-ui-refresh-sdd.md) v0.2。
- 范围：主题、侧栏、空态、会话、composer、工具卡片、设置与 Diff 的统一视觉改版；复用原 controller 行为。
- 产出：原生 Rust / GPUI 实现、可交互离线 HTML 预览、更新 ui-spec、交付与验收报告。
- 验收：fmt / clippy / workspace test / build、既有关键交互回归、真实窗口视觉与中文 / 键盘 / 窄窗走查。电脑使用权限早期曾阻挡截图，但重启 Codex 后已恢复；现有窗口的 AX / 截图 / 菜单路径已由主 Agent 复核，最终联合构建已完成菜单、标题、多行输入、设置和宽度等实窗复验；真实 IME 候选、完整 Tab 遍历、live provider 与性能边界见当前交付记录。
- 禁区：pi、新外部依赖、DDL、provider / permission 绕过、benchmark 口径变化、假截图、替换安装、remote write。
- 提交：`feat(A1-01): refresh native client interface`，一张卡 ≤3 个本地 commit，最终门禁未绿不放行集成。
- 遇阻：上报主 Agent 具体冲突 / 证据；不反复向用户请求已授权的实现许可。
