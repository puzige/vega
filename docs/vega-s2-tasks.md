# ✦ Vega — S2 任务卡（Sprint 2 · 侧边栏 & 项目模型 · W3-4）

**版本** v0.1 · 2026-08-29 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt
**S2 目标**（phase1-plan §2）：侧边栏（新建任务/项目/会话历史）；项目注册（选文件夹→识别 git repo→分支感知）；多项目多线程数据流。
**Sprint DoD**：建 2 个项目 × 各 3 个 thread，重启后状态完整恢复（含侧边栏折叠/pin/归档可见性）。

---

## 卡依赖图

```
T09 侧边栏骨架与视图路由
 ├─▶ T10 项目注册（文件夹→git 识别→CRUD）──┐
 └─▶ T11 Thread 模型与会话数据流 ──────────┤
            （vega_conversation 起步）      ├─▶ T12 侧边栏会话列表与项目分组 ──▶ T13 会话管理操作
```

执行者 Prompt 模板同 S1（[vega-s1-tasks.md](vega-s1-tasks.md) 头部），替换任务卡即可。

---

## T09 · 侧边栏骨架与视图路由（A1 布局）

- **前置**：S1（T07 主题 / T08 设置已就位）· **参考**：ui-spec §1 §4.6；tech-spec §1
- **目标**：主窗口落地 ui-spec §1 布局解剖——Sidebar 260px + 内容列，路由机制就位
- **产出**：
  - 主窗口布局：左侧 Sidebar 260px；右侧内容列 max-width 820px 水平居中、左右留白 ≥24px
  - `Cmd+B` 折叠侧边栏至 0，折叠状态持久化（`UiPrefs` 增 `sidebar_collapsed: bool`，serde default 向后兼容既有 config.toml）
  - 窗口宽 <960px 时侧边栏自动折叠（ui-spec §1 表）
  - 内容区空态按 ui-spec §4.6（居中引导语；"快捷模板按钮"仅占位不实现）
  - Sidebar 内部本卡只放区块占位（项目列表 T10、会话列表 T12 填充）；现有设置入口（Cmd+,）与 ✦ Vega 占位视图保留
- **验收**：布局对照 ui-spec §1 目视；Cmd+B 折叠/展开即时；重启后折叠状态保持；960×600 最小窗口无布局破裂；色值 grep 零命中
- **禁区**：不做列表数据（T10+）；不做 composer/会话内容（S3）；不动主题 token 机制

## T10 · 项目注册：文件夹 → git 识别 → CRUD（A1-03）

- **前置**：T09 · **参考**：tech-spec §2（projects DDL）；ui-spec §4.1（分组）
- **产出**：
  - Sidebar「添加项目」入口：系统文件夹选择器（先查该 rev gpui 的 dialog API（如 App::prompt_for_paths 类）；无则降级为路径输入框，报告注明选型）
  - **git 识别零依赖实现**：读 `<path>/.git`（目录或 worktree 指针文件）判 repo；解析 HEAD 得默认分支（`ref: refs/heads/X` → X；detached → 空）；非 git 目录允许注册（DDL `git_default_branch` 可空）
  - projects 表 CRUD（vega_store 内新增函数；**workspace 依赖补 `ulid` 精确锁定**——白名单已有，DDL id 即 ulid）：注册（path UNIQUE 冲突 → 内联 danger 错误条）；移除（直接移除数据库行，不动磁盘文件）；排序最小实现 = 「按名称 / 按最近打开」切换（拖拽排序后置，报告注明与 A1-03 字面的实现方式差异）
  - 打开/选中项目即更新 `last_opened_at`
- **验收**：添加 1 个 git 目录 + 1 个非 git 目录 → 列表与分支显示正确；移除后消失；重启后项目仍在（项目数据全在 SQLite，config 不存）；path 重复注册被拒且有错误条
- **禁区**：不动 threads（T11）；不做任何 git 写操作/远程操作；不引 git 相关 crate

## T11 · Thread 模型与会话数据流（A1-02 · vega_conversation 起步）

- **前置**：T10 · **参考**：tech-spec §1 §2 §3（本卡只取数据模型子集）；§6（默认值来自 config）
- **产出**：
  - `vega_conversation` 首个实质内容：`types` 模块放 Thread 数据结构（与 DDL 对齐）+ `ThreadMode`/`ThreadStatus` 枚举（**ConversationEvent 等流式类型仍留 S3/S4，本卡不许提前定义**）；threads CRUD（create / list_by_project / update 字段集）
  - 「新建任务」入口（sidebar [新建任务] + `Cmd+N`）：在当前项目建 thread——`model` 取 config `defaults.model`（空则空串）、`permission_mode` 取 defaults（空缺按 DDL 默认 confirm）、mode 按 DDL 默认 execute；建后打开该 thread
  - 打开 thread → 更新对应 project `last_opened_at` 与 thread `updated_at`
  - 依赖链正式落地：vega → vega_ui → vega_conversation → vega_store（tech-spec §1 图）；内容区显示当前 thread 标题头 + 空态占位（「会话内容 S3 接入」）
- **验收**：Cmd+N / 按钮建 thread → 以最小列表形式可见（T12 前的过渡显示）→ 杀进程重启后 thread 还在；无新表（六表之外零 DDL 变更）
- **禁区**：不做消息流/composer/流式（S3）；不碰 vega_runtime；**types 模块之外不放跨 crate 共享类型**（红线）

## T12 · 侧边栏会话列表与项目分组（A1-04）

- **前置**：T10 T11 · **参考**：ui-spec §4.1 §4.6
- **产出**：
  - 项目分组可折叠（折叠状态持久化，同 T09 机制——记 config 或 SQLite，实现者选一并报告）；分组内 thread 条目按 `updated_at` 倒序
  - 条目规格照 ui-spec §4.1：单行 = 标题（省略号截断）+ 右侧相对时间（"2h" 样式）；选中态 `bg_active` + 左侧 2px 强调条；未读 = 500 字重 + 圆点（`unread` 字段本卡恒 0，置位逻辑 S3 流式到达时做）
  - 点击条目切换当前 thread（内容区标题头随之切换）
  - 自动化入口灰显占位（A1-13，Phase 3 前灰显——顺手行）
- **验收**：2 项目 × 各 3 thread 的层次/排序/选中态正确；折叠状态重启保持；相对时间显示正确（可用改库时间戳方式验证）
- **禁区**：不做全局搜索（A1-06 后置）；不做未读产生逻辑（S3）

## T13 · 会话管理操作（A1-05）

- **前置**：T12 · **参考**：tech-spec §2（threads.status / pinned 列）
- **产出**：
  - 重命名（行内编辑，Esc 取消 / Enter 提交）
  - 归档 / 恢复（archived 隐藏于主列表 + 分组尾部「已归档」折叠区可展开查看）
  - 删除（确认弹层一次；**同事务删除该 thread 的 messages / tool_calls 行**——DDL 无级联，防孤儿行；token_usage 保留作成本审计）
  - pin 置顶（置顶组优先于时间倒序；再点取消）
- **验收**：四操作各自生效且重启保持；删除后 `SELECT COUNT(*) FROM messages WHERE thread_id='X'` = 0（SQL 验证无孤儿）；置顶组排序正确
- **禁区**：不做未读逻辑（S3）；不做拖拽排序；不做多选批量操作

---

## S2 完成定义（DoD，Sprint 验收）

- [ ] T09-T13 全绿；hooks 门禁在 master 绿
- [ ] **2 项目 × 各 3 thread，重启后状态完整恢复**（项目/分支显示/会话列表/折叠/pin/归档可见性）
- [ ] 960×600 最小窗口无布局破裂；色值 grep 零命中；unwrap/expect 非测试段零命中
- [ ] exec-guide §3 红线全过（架构师走查）；六表之外零 DDL 变更
- [ ] `cargo xtask bench` 仍出数（回归不 block）

> 架构师预裁（拆卡时定，执行者照做、人类可否决）：① T10 排序以「名称/最近打开」切换为最小实现，拖拽后置；② T11 `model` 允许空串（S4 接 provider 后才有真值）；③ 项目移除不加确认弹层（不动磁盘文件），确认类交互 S5 权限体系统一时统一；④ 非流式的 `unread` 置位逻辑归 S3。
