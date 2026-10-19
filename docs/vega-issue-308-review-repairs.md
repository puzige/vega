# #308 — GREEN 后审查修复与交付

版本：v2.7，2026-10-19。状态：Codex 已完成修复方案；等待 Vega 依次完成新基线集成、新辅助功能回归 RED、最小修复及带测试 GREEN。**Codex 再审查前不得删测。**

Issue：https://github.com/puzige/vega/issues/308
Draft PR：https://github.com/puzige/vega/pull/484
分支：`feat/308-new-thread-order`；复用本卡 sidebar-reorder worktree，单一写入者。

本版延续 [v2.6 的排序、消息失败及 M1–M7 契约](vega-issue-308-message-order.md)，落实用户最新授权：“更新两项修复方案 → 交 Vega 修复并重新 GREEN → Codex 审查 → Vega 独立删测并跑最终门禁”。用户已澄清本轮是代码 review，不是实际操作 VoiceOver；真实读屏/模型/应用集成仍待用户手测，不标通过。

## 证据与当前基线

- 原冻结测试：[7c5fd1ab3432f9d11219a1340d30b1dfc1a81bed](https://github.com/puzige/vega/commit/7c5fd1ab3432f9d11219a1340d30b1dfc1a81bed)；[九份源码及 hash](https://github.com/puzige/vega/pull/484#issuecomment-6254993633)。原 41 个测试及 fixture 目标/预期保持冻结，不修改、不删除。
- 原 RED：[39164663186](https://github.com/puzige/vega/actions/runs/39164663186)，41 执行、10 通过、31 失败、0 跳过；不用于追认本轮新辅助功能回归。
- 已实现并带测试 GREEN：[a6c23b78d27d8c5843351c8f88c2d55a2f9fa213](https://github.com/puzige/vega/commit/a6c23b78d27d8c5843351c8f88c2d55a2f9fa213)，[39198098569](https://github.com/puzige/vega/actions/runs/39198098569)：41 发现/执行/通过、0 失败/跳过；Python 34、fmt、全目标 Clippy、required check 成功。原 GREEN 与中间 37/4 结果及祖先完整保留。
- 新 master：`f054c35874513bab4c65d7e179fdac6758503182`（#488）。已发布 `0028_runtime_failure_details.sql`，本卡仍使用 `0028_sidebar_message_order.sql`，GitHub 读回 CONFLICTING / DIRTY。
- [Codex 审查与根因](https://github.com/puzige/vega/pull/484#issuecomment-6259639058)。本版文档在任务分支与远端同步到 a6c23b78 后先行提交；生产基线的冲突解决按下述明确顺序由 Vega 执行，不在文档阶段改业务代码。

## 验收矩阵（先于修复）

| ID | 前置及操作 | 必须满足的结果与证据 |
|---|---|---|
| R1 | 集成 f054c358；旧库迁移序列末尾已有 runtime 第28项 | 已发布 1–28 项 SQL、顺序和语义不变；本卡 SQL 原文不变，文件顺延为0029并在第28项之后追加。旧 schema27 的四案经真实 migrate/reopen 保留排序、消息、JSON及其他元数据；原41项不改并在新head云端通过。静态比对迁移原文字节和清单顺序，不把维护型编号调整伪造成行为 RED |
| R2 | 在修复角色/事件前，以真实生产归档元素构造器分别构建 archive=false/true 两状态 | 一个新的行为用例断言元素有ID、`Element::a11y_role()` 为 Button；通过 `Element::write_a11y_info` 写入真实 AccessKit Node，名称分别为“显示已归档/隐藏已归档”，支持 Click 与 Focus。旧实现缺角色/Click，应在此断言取得有效 RED；不检索源码字符串，不检查测试自己构造的假按钮，不伪造完整AX树 |
| R3 | 修复归档元素后，运行原41项及R2同一用例 | 预期42发现/执行/通过、0失败/跳过，并通过Python34、fmt、全目标Clippy及总门禁；鼠标、Tab/Enter/Space每次仅切换一次，焦点、选中态、导航/草稿与全部排序/事务/fence不变。实际计数以日志为准，不以定义数代替 |
| R4 | Codex核实新GREEN、源码与全部生产差异后，独立删测 | 删除Rust测试/可执行doctest和真正独占支持；最新head完整门禁明确no Rust tests（0发现/0执行），Python34等通过。删除不能夹带生产修复；原源码/RED/GREEN永久可找回 |

R2 是本次审查发现的辅助功能缺口的新一轮先测后修。原41项 GREEN 不证明它，也不允许将新用例插进旧冻结文件来追认。只新增一个有区分度的两状态语义用例；如需额外用例/支持或改变验收，先交 Codex，不自行扩张。

## 根因与具体修复

### R1：迁移编号与已发布版本冲突

`Store::migrate` 按 `MIGRATIONS` 的数组索引决定 `user_version`。master 的第28项已是 runtime_failure_details，不能在它之前插入 sidebar SQL，也不能覆盖/重命名已发布迁移。

1. fetch 后普通 merge 最新 master，保留 a6c23b78、7c5fd1ab 及所有 CI 来源祖先；不 rebase/force-push 丢弃证据。先保护已有改动，同一范围不得并发写入。
2. `crates/vega_store/src/lib.rs` 冲突处保留 master 的 `0028_runtime_failure_details.sql` 第28项；将本卡尚未发布的 SQL **原文不变**重命名为 `0029_sidebar_message_order.sql`，在它后面追加为第29项。
3. 对已发布1–28项与 f054c358 做字节/顺序核对；0029内容与 a6c23b78 的 sidebar SQL 字节一致。保留#488其余生产变更，不恢复其测试。
4. 原四个 legacy 测试固定的是源 schema27，没有把升级后的版本硬编码为28；不需要改变原测试。不得编辑用户DB、篡改user_version或在本地运行迁移。编号维护本身不造RED，行为由原冻结迁移用例在新head重验。

若最新master又占用0029、存在未列出的冲突或公开接口变化，先回报Codex更新明确清单；不要自行覆盖、改冻结断言或删测避过。

### R2：归档控件未导出辅助功能节点和Click动作

锁定 gpui-pre0.3.4 的 Div 仅从显式 role 返回 `a11y_role`，focusable 不创建角色；Element 仅为有ID及role的元素导出节点。Click动作仅由 on_click 或显式 on_a11y_action 导出。现有 vector_control 的 aria_label + mouse-up + 普通keydown不足以形成可操作节点。

#### 先行测试及允许的纯结构接线

在 `vega_ui/src/sidebar/threads_block/organization/render.rs`：

- 将 private `vector_control` 返回类型从 AnyElement 改为 `gpui_kit::Stateful<gpui_kit::Div>`，只推迟类型擦除，移除末尾的 into_any_element；现有 `.child(...)` 消费者不变。不得改控件的属性、事件、状态或样式。
- 把当前 render_recents_pi 中归档控件那一组真实参数原样提取为 private `archive_control(hidden_until_hover, cx)`，返回同一具体元素；名称继续从真实 `org.archive` 推导，正式渲染也调用该构造器。不新增公共方法、观察全局或注入角色/Click的测试覆盖分支。
- **此阶段不设置 Button、不换 on_click、不新增可访问激活处理**。逐项比对提取前后属性和事件，仅结构接线可以先于RED。

新增 `organization/archive_accessibility_tests.rs`，在 organization.rs 的 cfg(test) 下单独接线；不改九份冻结源码。使用本卡最小 owned DB 与必要 globals/Sidebar fixture；可直接用 GPUI公开 Element API 和 `gpui_kit::accesskit::Node/Action`（当前依赖已 re-export，无新依赖）。必须调用正式 `archive_control` 构造真实元素，不能由测试自行提供label/role/click来满足断言。两状态只作明确fixture前置，通过真实Node验证语义输出；不是仅断言mock调用或源码存在。

当前TestWindow不能激活完整a11y树，不修改依赖、不建立自制platform或HTTP/provider mock。新的测试证明真实元素导出的语义属性，原41项证明普通输入和业务终态；真实VoiceOver仍待用户，不声称已执行AXPress或真实朗读。测试前置若无法用以上已核实API落地，给出最小缺口交Codex，不假造结果。

完成上述测试准备后提交/普通push同一draft #484，用现有Actions取得新用例的目标RED，完整保留原41项。Vega回报精确commit/run/job、原始失败及计数并停止；**Codex读回有效RED、冻结新增用例后，才明确交回最小修复。**

#### RED确认后的最小生产修复

仍在 render.rs 的实际构造链中，仅对 `Control::Archive`：

1. 显式 `role(gpui_kit::Role::Button)`，保留真实id、两状态动作名称、focusable/tab_stop与现有几何/主题/选中/焦点样式。
2. 归档改用同一 `on_click` 回调调用既有 `control_action(Control::Archive, window, cx)`，消费事件；锁定GPUI将该listener导出为Click，并把辅助功能Click映射到真实点击路径。
3. **归档不能同时保留旧mouse-up与手动Enter/Space回调**：GPUI on_click已有键盘keyup激活，两套并存会切换两次。用明确archive条件选择该分支；其他vector控件继续原mouse-up/keydown行为，不能顺带扩大为全侧栏辅助功能改造。
4. 不改变 Control::Archive 的分页/显示逻辑、组织数据或排序。原41项继续检查普通鼠标与Tab/Enter/Space只切一次，新增同一语义用例检查按钮/名称/Click/Focus。

本修复无需新runtime、依赖、图标、控件文案或可访问状态基础设施。原辅助功能激活不应被误报成真实VoiceOver已通过。

## 修复后的审查和独立删测

Vega将R1/R2修复提交、原41项与新增用例源码/hash、RED及最新42项GREEN固定在同一PR，交回Codex并停止写入。Codex复核：已发布迁移不动、0029顺序正确；原冻结九文件不变；纯结构提取与可访问角色/事件分支范围正确；没有双切换、其他控件行为变化或新业务差异；精确head全部门禁GREEN。

**Codex发布第二次明确交接前，Vega不能自行删测。** 第二次交接才授权独立提交删除下列测试及真正独占支持，并在同一PR跑最终门禁：

- 原九份测试/fixture文件、R2新用例及cfg(test)模块接线，workspace/xtask中如有其他Rust测试须逐项核实来源，不能删未GREEN测试。
- 本卡独占的tempfile/dev GPUI test-support、原锁版本测试支持包及依赖边；保留生产/benchmark依赖，不升级包，不整份回滚Cargo.lock。
- 三个跨crate只读投影/error observer、owned config override、测试跳过updater、八处debug_selector等无生产消费者的测试支撑。
- **保留**有生产消费者的completion处理器/token、archive_control/vector_control真实构造链、R1/R2生产修复、迁移、Python测试/fixture、通用CI/Nextest、benchmark及发布能力。

以删测前最新GREEN为基准审查差异：只有测试/独占支持移除或必要还原，无新增业务变化。若清理发现需改生产行为，先停并交Codex，不能塞入删除提交。删测head必须取得0发现/0执行与`no Rust tests`、fmt/全目标Clippy/Python34及required check全部通过，回报后停止等Codex。

## 允许范围、命令与交接

允许修改：本版文档；本卡SQL重命名和MIGRATIONS；organization/render.rs的上述纯提取与archive分支；organization.rs仅新增测试接线；一个新的a11y语义测试文件。新master变更只作原样集成。除独立删测阶段的明确清单外，不改原41项/fixture、store消息/排序、runtime、模型、权限、定价、其他UI、CI、工具链或发布流程。

本地仅快速fmt/diff及离线locked metadata；不运行Vega、编译、cargo check/Clippy、Rust/Python测试、迁移或打包。测试、全部编译与门禁只由现有pull_request Actions执行；不新建工作流、不rerun碰绿、每5分钟查询，完成通知/用户即时要求除外。

原41项始终保留至新42项同批GREEN留证；新增R2用例从新RED开始冻结。GREEN后不追加用例、不放宽断言/ignore，其他新需求/遗漏另开周期。原始7c5fd1ab/a6c23b78和本轮完整SHA/源码/RED/GREEN/独立删除提交/最终空集run均固定在PR，不依赖本地临时文件或过期artifact。

Vega领取前读AGENTS、执行总纲、工作流与本版，确认任务分支/前置与单写入者，置In progress。每次回报都保留Issue开放，写当前执行者待领取、下一执行者Codex，不自行合并/关闭/清理。Codex在本次用户连续授权下跟进本卡各阶段；若最终门禁与审查均满足，才按工作流转正式/合并并保留本卡资源进入In review等待用户手测。回滚通过另行PR，不改已发布迁移或用户数据。
