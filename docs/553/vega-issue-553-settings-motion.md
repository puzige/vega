# #553 设置列表与详情的轻量层级过渡

## 版本与授权

- 实施规格 v2，2026-10-24；Issue：https://github.com/puzige/vega/issues/553。
- 本轮基线：`0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`。
- 分支：`feat/553-settings-motion-takeover`；隔离 worktree：`vega-553-settings-motion-takeover`。
- 接手标记：[settings-motion-takeover-20261024T1715Z](https://github.com/puzige/vega/issues/553#issuecomment-6387173666)。同卡仅一名实现写入者，Git 与集成由主 Agent 负责。
- 旧规格及完整调查保留于 [v1.1](https://github.com/puzige/vega/blob/46adb1787c40fa9e49258b80bc890978f06c3fd5/docs/553/vega-issue-553-settings-motion.md)，不覆盖旧工作树。
- 本文替代旧文档的待授权、强制先行测试和共享接口未实现等过期流程描述；不减少 S1–S14 产品边界。
- 规则：[执行总纲](../vega-exec-guide.md)、[设计守则](../vega-design-guidelines.md)、[验证与交付政策](../vega-rust-test-delivery-lifecycle.md)。本地不运行 Vega、编译、构建、测试、打包或 CUA；仅快速静态检查。自动检查由现有 Actions 执行。

## 产品表现与不变量

1. 窄窗：真实导航完成之后，仅非交互标题文字短淡入。返回按钮、按钮文字、焦点环、字段、校验与错误始终即时且完整可用。返回列表即时恢复列表，不为离场保留旧详情。
2. 宽窗：provider 既有选中填色、model 分区既有底部 marker 做原位颜色/alpha 过渡。语义选中和实际表单立即切换；旧 marker 退场不得维持旧对象的选中语义。
3. 共享时长 120ms、Kit 默认 `ease_out_cubic`，无 delay/spring/过冲；复用 `Motion::INTERACTION_DURATION_MS`，不与其他卡争写共享 token。
4. 初挂载、同目标、响应式断点变化、恢复显示不新开导航动画。快速反向只投影最新真实目的地；动画完成不提交导航、保存或焦点变更。
5. 不整页/整列/整表单滑动、缩放、淡入，不插值尺寸，不保留第二份表单；不改业务草稿、保存、确认、校验和配置语义。
6. 不新增常驻解释、toast、提示或第二套导航；使用当前标题、返回动作与选中态。
7. Reduce Motion 直接终态。隐藏、失活、离开或完成后停止装饰帧；无 draw 的失活→恢复也不得补播旧效果。业务异步结果和错误不因此暂停。
8. 保留现有 list/detail/section ScrollHandle 与编辑器实体。返回时焦点落到当前仍存在的列表对象；对象消失时使用有效入口，不能留在卸载的表单。

## 验收矩阵

| ID | 场景 | 必须结果 |
|---|---|---|
| S1 | 窄窗列表→provider 详情→返回 | 守卫批准后局部标题反馈；操作即时；原列表 scroll 和合理焦点恢复 |
| S2 | 有草稿时切换/返回 | 草稿、输入、校验和位置保留；确认出现；目的地不提前反馈 |
| S3 | 取消确认/留在本页 | 原位置不变；不播放或预消耗未来导航反馈 |
| S4 | 放弃修改/保存成功继续 | 真正导航后恰好一次反馈；原清理规则不变 |
| S5 | 校验失败/保存失败/正在保存 | 留在正确位置，错误及时可见，草稿可恢复，原 busy/guard 保留 |
| S6 | provider→model 基本/思考/定价→返回 | 实际 provider/model/section 身份一致；覆盖各类草稿守卫 |
| S7 | 键盘和过渡期间输入/保存/取消/返回 | 无等待、幽灵节点或旧表单收输入；焦点与操作名有效 |
| S8 | 快速连选/连续返回/反向 | 最新真实目的地唯一权威；可中断、不排队、不复活旧对象 |
| S9 | 删除/重排/空列表 | 稳定身份、不串对象，沿用有效 fallback/空态和可用焦点 |
| S10 | 窄↔宽含草稿/动画 | 立即最终布局；不重建编辑器、不丢草稿/校验/scroll、不补播 |
| S11 | 宽窗 provider/model 选择 | 仅原位 marker/填色过渡；详情、语义选中、输入即时切换 |
| S12 | Reduce Motion/隐藏/失活/结束/恢复 | 终态或停止装饰帧；恢复不补播；不暂停真实错误和异步结果 |
| S13 | 浅/深主题与可访问性 | 真实标题即时更新；返回按钮/焦点环始终清楚；状态不只依赖运动 |
| S14 | 滚动中进入/返回/多次编辑/错误恢复 | 原独立 scroll 保留；返回原有效列表对象或合理入口；不跳无关位置 |

## 当前源码与实施约束

- `settings/provider_management.rs::provider_command`：saving/draft guard 先于真实 `Select` / `BackToList`，不在 pending 请求上触发视觉状态。
- `settings/model_configuration.rs::request_settings_destination` / `finish_settings_navigation` / `resume_settings_navigation`：守卫完成边界；取消与失败不得误触发。
- `render_provider_management` 宽窗设置 `detail_open = true` 是布局投影，不是用户导航。
- provider row 填色在当前选中背景；model navigation 的现有分区 marker 在 `render_model_navigation`。
- `interaction::decorative_value` 已合入，使用 Kit `transition_with_status` 和 window activation 订阅；首次 target 直接采用，不能以首次 `target=1` 冒充有限标题入场。标题必须通过锁定 Kit 有限入场接口/显式初始采样实现，不自建动画引擎。
- 实现范围仅本卡 Settings 内部状态、导航视觉投影、局部标题/marker 接线及必要返回焦点修复。不写其他卡共享 sampler、Motion token、消息滚动/复制/产物、运行时数据库、CI 或依赖。
- 发现真实基线焦点缺口，先记录精确入口及修复范围，保留 S7/S14，不用“原行为不变”遮蔽。
- 实现前定点调查需明确实际 API、有限标题入场和生命周期路线；不可因 API 猜测而扩大到整个操作栏淡入。

### 定点调查确认的实施路线

源码基线仍为 `0fe30576d5884df6f7c2e5f75a0bb580eb883b9f`；规格首提交为 `5ce8a9d905e04695567cc5397952b6b1f6d26cd0`。只读调查未改源码，确认无新增依赖/API 阻塞。

- 新增 Settings 私有 `motion.rs`，使用真实 provider/model/section 身份、一次性导航许可及最近模式；许可仅在守卫后的真实变更产生，render 在 `resume_settings_navigation` 后投影最新目的地，不让配置重排、宽窗自动 `detail_open=true` 或同目标保存重开误触发。
- 锁定 Kit 首次采样直接落 target，且 keyed state 只在 prepaint/paint 可用。采用委托现有 Element 的私有绘制适配，只包装 provider 名字和 model 名字文字节点。合法许可首次 paint 同 channel 零时长 seed 0，再以 120ms 采样到 1；后续只取 1。计时、缓动和帧请求交共享 sampler，不自建时钟。
- seed 前检查可见性、激活和 Reduce Motion；owner 在失活时撤销未消费许可，模式变化/隐藏/离开清除许可并直接终态。不能用采样后 opacity=0 反判自身不可见，不能因重新挂载 keyed state 重播。
- provider row 当前按 index 的绘制身份改成稳定 provider 名字；选中填色隔离为装饰背景层，不被 `.interactive(..., selected)` 的立即选中 rest 覆盖或重复绘制。保留文字、命中、焦点环、hover/pressed 和拖拽原行为。model marker 按 provider/model/section 稳定身份采样 accent alpha，透明边框槽位保持几何不变。
- 已确认焦点缺口：`BackToList` 当前仅设 `detail_open=false`；model 返回清掉 model 配置但无入口回焦。修复仅在真实返回提交后记录目标，于目标 render 同步实际 FocusHandle 后消费，不等动画回调。provider 复用 `provider_edit_focuses` 的稳定 name 映射；model 配置入口增加 `(provider, model)` 稳定 focus；对象消失回到有效邻项/可见入口，空列表回到实际添加入口。保留所有 scroll handle 和输入实体。
- 文件限 `settings/{motion,mod,state,provider_management,model_configuration,render_impl}.rs`；主 Agent 写本规格，唯一专职实现者写源码，非实现者审查。共享 interaction/theme、依赖、CI 不改。

## 验证与交付

- 本地：`git diff --check`、限定格式检查、静态检查稳定身份、守卫后接线、表单/scroll 实体保留、注释/依赖/CI 无越界。静态分析不称运行验证。
- 实现按风险选择必要开发回归；不强制 TDD、不为常量或源码字符串造测试。若有 Rust 开发测试，只由现有 Actions 执行，固定可恢复源码/patch/完整 SHA/实际结果后合并前清理测试及独占支持。
- 最终完整 head 经非实现者独立审查及现有 `pr-check`：fmt、全目标 Clippy、保留的 Python 检查、Nextest 清点与执行；无 Rust 测试明确写 `no Rust tests`（0 发现/0 执行），不称行为通过。
- CI 每 5 分钟查询，不 watch/短轮询；实际完成/失败通知或用户即时要求除外。最新 head required check 通过且审查无阻断后 squash merge。
- 真实输入、保存失败、焦点/scroll、窄宽布局、浅深主题、Reduce Motion 与可发现性由用户手测；不以静态/CI/系统合成事件代替。
- 合并后 In review，Issue 保持 OPEN，停止取下一卡并等用户。无本地安装、启动、重启或强退 Vega。

## 回滚

只撤销本卡标题/marker 装饰投影和其生命周期接线，恢复即时呈现；不得撤销业务守卫、保存、草稿、错误或必要焦点/scroll 保全，不影响共享 sampler 的其他消费者。

## 当前证据状态

已读取原卡与接手评论、确认 Project In progress、fetch 并更新基线、新建隔离分支；尚未实现、未触发 CI、未手测，不宣称行为或性能改善。
