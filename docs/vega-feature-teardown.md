# ✦ Vega — 竞品功能拆解与取舍（Feature Teardown）

**版本** v0.1 · 2026-08-28 · 关联：[vega-prd.md](vega-prd.md) v0.3
调研对象：Codex Desktop / Devin Desktop / Antigravity / ZCode / WorkBuddy（均有官方来源，推测项已标注）

---

## 1. 功能全景矩阵

| 功能域 | Codex Desktop | Devin Desktop | Antigravity | ZCode | WorkBuddy |
|---|---|---|---|---|---|
| 多项目×多线程 | ✅ + fork 会话 + worktree handoff | ✅ Kanban 指挥中心 + Spaces | ✅ Agent Manager 多 workspace 并行 | ✅ 分组拖拽+时间线 | ✅ 多窗口多 agent |
| Composer 模式 | Auto/Read Only/Plan | Normal/Plan/Ask | 四档权限 | 执行前确认 | Craft/Plan/Ask 三模式 |
| @引用 / /命令 | ✅ @文件/技能/线程，/model /plan /fork… | ✅ slash 切模式 | ✅ /goal /grill-me /browser… | ✅ @插件/文件/会话，/side /btw | ✅ @文件/专家/连接器 |
| 权限体系 | OS 沙盒三档+按需提权 | Deny/Ask/Allow ×读/写/命令/HTTP/MCP 分层 | 四档+Seatbelt 沙箱 | 敏感操作确认+Hooks | 默认权限 vs 完全访问 |
| Agent 执行 | 自研+本地沙盒+Computer Use | 自研 Devin Local(Rust 重写省 30% token)+Cloud 移交 | 多 agent 并行+浏览器验证 | Goal 系统+checkpoint 回退 | 本地+云端沙箱 |
| 子 agent | ✅ 可 fork | ✅ 前后台+自定义 profile | ✅ Subagents+Teamwork | ✅ 子智能体 | ✅ 动态任务拆解 |
| Diff 审阅 | ✅ +逐行评论+PR 徽章/review comments | ✅ +云端 AI Review | ✅ Review Changes | ✅ 评审+评论定位 | ✅ Changes 区 |
| Open in… 外部编辑器 | ✅ VS Code/Cursor/Zed/Terminal… | 本身是 IDE | 本身是 IDE | ⚠️ 独立工作台 | ✅ |
| 产物系统 | ✅ Summary pane+富预览 PDF/表格 | IDE 内置 | ✅✅ Artifacts（计划/walkthrough/**录屏验证**） | ✅ 计划/checkpoint/录屏 | ✅ Artifacts/Preview/部署 |
| 定时任务 | ✅✅ GA：cron+模板+**自我排程跨天续跑** | ✅ 云端 cron+Playbook | ✅ /schedule | ✅ 定时+**闲时任务(0.67x)** | ✅ RRULE+模板+推送微信 |
| 插件/集成 | ✅ 111+ 插件市场（skills+MCP 打包） | ✅ **ACP 多 agent 共治**+MCP | ✅ MCP+Skills+Hooks | ✅ 插件市场+MCP | ✅ 30+ 连接器+2 万+ 技能市场 |
| 远程 | SSH devbox(alpha) | SSH+dev container+云快照 | ❌ | ✅ SSH/WSL+**手机遥控+IM Bot** | ✅ IM 遥控电脑 |
| 用量透明 | ⚠️ 档位额度，CLI /status | ✅ 配额 gauge+Session Insights | ⚠️ /usage /credits 额度不透明 | ✅✅ 用量统计页+定价公开+闲时系数 | ✅ 积分倍率+任务消耗 |
| 独有特色 | Computer Use、Sites 建站、图像生成、Memory | ACP、IDE 一体、按 token 计费 | 录屏验证、知识自学习 | 闲时任务、IM Bot、项目记忆/知识库 | 专家体系、资料库、人机双写 |

## 2. 关键结论

**品类收敛（Table Stakes，不做就出局）**：多项目多线程、Plan/Ask 模式切换、权限分级、diff 审阅、Open in…、定时任务+模板、MCP、用量显示、中断恢复、checkpoint/回退（ZCode/WorkBuddy 已做成标配）。

**透明度真相**：五家都在做「用量可见」，但全是**配额/积分/倍率**层面（ZCode 最深：统计页+公开定价+闲时系数）。**没有一家做到：每次 API 调用级真实成本、跨模型性价比对比、预算硬控制**——Vega 的 Token 经济差异点依然成立，但门槛比想象中高：至少要超过 ZCode 的统计页才算差异化。

**意料之外的发现**：
1. **Plan 模式是三家中标配**（Codex/Devin/WorkBuddy 都有 Ask/Plan/Execute 三态）——Vega composer 必须有，进 Phase 1。
2. **Checkpoint 回退**（ZCode Goal-checkpoint、WorkBuddy 一键回退）——agent 信任基础设施，进 Phase 2。
3. **闲时任务**（ZCode：算力空闲免费跑）——和 Token 经济天然联动（低峰低价模型跑非紧急任务），是 Vega 可以做得更绝的差异点，进 Phase 3。
4. **Devin Local 用 Rust 重写 agent 省 30% token**——证明「Runtime 效率=成本」已被竞品验证，正是 Vega 自研 Runtime 的卖点论据。
5. **手机遥控/IM Bot**（ZCode/WorkBuddy）——重资产但粘性极强，配合 Token 告警推送有价值，Phase 4-5 评估。

## 3. Vega 功能取舍表

### ✅ Phase 1 做（MVP 必备）
| 功能 | 对标来源 | 备注 |
|---|---|---|
| 多项目×多线程 + SQLite 持久化 | 全员 | A1/A11 已有 |
| Composer v1：**Ask/Plan/Execute 三模式** + @文件 + 模型选择器 + 权限模式 | Codex/Devin/WB 三模式标配 | **PRD 需补充：Plan 模式** |
| 权限门禁：变更前确认 + 危险命令硬拦截 | ZCode 确认流 + Codex 沙盒 | E5 已定 |
| 流式会话 + 工具卡片 | 全员 | A2 已有 |
| Diff 审阅（只读高亮+hunk 导航） | 全员 | A5 已有 |
| Open in… 外部编辑器交接 | Codex 菜单 | A5 已有 |
| **Token 实时计数 + 每任务成本标注** | 超越 ZCode 统计页 | A10，差异化核心 |
| 中断/恢复 | 全员 | A3 已有 |

### 📅 Phase 2（Open Agent）
| 功能 | 对标来源 | 备注 |
|---|---|---|
| ACP 编排外部 agent | Devin 已验证 | D6 已定 |
| PTY 终端 + 命令卡片 | 全员 | A6 |
| Commit 辅助 + PR 创建 | Codex PR 徽章 | A5 v2 |
| **Checkpoint 回退** | ZCode/WorkBuddy | **PRD 需补充** |
| 成本仪表盘（模型/项目/天） | 超 ZCode 统计页 | A10 v2 |
| 子 agent 派发 | 全员 | A3 v2 |
| 多 Provider（Anthropic/DeepSeek/Ollama） | 全员 | A3 v2 |

### 📅 Phase 3（Trust & Automation）
| 功能 | 对标来源 | 备注 |
|---|---|---|
| 定时任务 + 模板/快捷指令 | 全员标配 | A7 |
| **闲时任务（低价模型跑非紧急任务）** | ZCode 首创，Vega 可做得更好 | **PRD 需补充，与 Token 经济联动** |
| 项目记忆 | ZCode/WorkBuddy | **PRD 需补充** |
| MCP 连接器 | 全员 | A8 |
| Harness Trace/Golden Set/护栏 | 无竞品做深 | A9，差异化 |
| 预算上限告警 | 无竞品做 | A10 v2 |
| Worktree 隔离执行 | Codex/Antigravity | agent 并行前置 |

### 📅 Phase 4-5（Ecosystem+）
SSH 远程（D7）、插件市场、移动遥控/IM Bot（配合 Token 告警推送）、团队协作、云端同步

### 🚫 明确不做
| 功能 | 理由 |
|---|---|
| Computer Use（桌面操控） | 工程重、安全事故率高，与「可信 agent」定位冲突；浏览器自动化（Phase 4+ 经 MCP）已覆盖多数场景 |
| Sites/云端建站部署 | Codex 特色但偏离开发者工作台主线 |
| 内置完整 IDE/编辑器 | D5 已锁定 |
| 云端 Agent 托管（前期） | Phase 5 再说 |
| 图像生成等非编码多模态 | 聚焦 |
| 专家市场（WorkBuddy 式） | 与开发者定位不符；技能/指令模板（轻量版）在 Phase 3 A7 覆盖 |

## 4. 需要回写 PRD 的变更（→ v0.3.1）

1. **A2 Composer 增加「Ask/Plan/Execute 三模式」**（三家标配，Plan 持久化为可批准的计划产物）
2. **A5 增加 Checkpoint 回退**（Phase 2：任务内检查点，一键回退到任意 checkpoint）
3. **A7 增加闲时任务**（Phase 3：低价/低峰模型调度非紧急任务，Token 经济联动）
4. **A11/A3 增加项目记忆**（Phase 3：项目级记忆页，沉淀约定与偏好）
5. **A10 定位上调**：差异化门槛 = 必须超过 ZCode 用量统计页（配额→真实成本、单模型→跨模型性价比、事后→预算硬控制）
