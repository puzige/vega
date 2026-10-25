# #601 — 产物卡片结构与交互

版本：v1.0，2026-10-25。Issue：https://github.com/puzige/vega/issues/601 。
当前基线：`420a0eb9a06594173b658f7e3a4437e3d2b5432d`；分支 `feat/601-artifact-card-hierarchy`。已 fetch、安全更新并建立独立 worktree；主目录原有未跟踪内容不动。

## 1. 目标与边界

承接 #542 已交付的回合结束聚合、同文件去重与捕获修复，仅调整产物卡片信息层级和操作。实现依据是本卡用户批准的产品要求、现有 Vega 组件和共享 token；Issue 中旧第三方 bundle 静态记录不作为实现输入，不提取、复制私有组件或资产。

- 一个组级头部：文件图标、`Edited N files`（单个文件为 `Edited <filename>`）和真实的 `Open Review`。
- 默认显示 3 个文件；更多时显示 `Show N more files`，展开后保留 `Collapse files` 和对应共享箭头，可反复切换。
- 目录次要色、文件名主色；长路径两段可截断，优先保留文件名。完整路径进入可访问名称和按需详情。
- 文件行整行触发现有预览。保留六个外部打开目标及其权限、错误、结果语义；不再重复摆放预览小按钮。
- 字节数不常驻，保留在路径悬停详情；不新增成功说明或解释段落。
- 不改捕获/去重/延迟归属、数据模型、持久化、打开白名单、权限、安全检查，不回填历史；不实现增删行数、文件级 diff 定位或 Undo，不扩展 standalone 的 Review 能力。

## 2. 已核对调用链

- `artifact_card.rs::ArtifactCard::render_compact` 当前 24px，同权重路径、常驻字节数，只有 hover/focus 小按钮；现有 `preview`、`open`、`toggle_menu` 是操作与状态的唯一生产入口。
- `ArtifactGroup` 持有文件/错误、去重与展开状态；`render` 目前没有头部，默认 6 行且只有展开。`sync_file/capture` 和回合归属不修改。
- `conversation_stream/composer.rs::artifact_group` 创建并观察 group；观察回调通过 `invalidate_item` 重新测量并恢复既有滚动锚点。`flush_artifact_group` 依赖零行判断空组，必须保留空组为零。
- `render_rows.rs` 渲染 group；`message_anchors.rs::estimate_entry_height` 当前通用估算按 24px。新增专用几何后必须同步该分支，不能只改绘制高度。
- `render.rs::emit_open_diff` 已有 `reading_preview || thread.is_standalone()` 守卫，发出既有 `OpenWorkspaceDiffRequested { thread_id, project_id }`。头部入口最终必须回到此方法，不新增事件类型或假设文件定位。
- `core.rs::set_reading_preview` 已传播到 card；新增 group 的 Review 可用态同步传播，初始化也依据当前会话状态。

## 3. 验收矩阵（实现前确定）

| ID | 前置 / 操作 | 可观察结果 | 验证路线 |
|---|---|---|---|
| C1 | 回合结束，1/3/4+ 个已捕获文件 | 单组单头；默认至多3行；文件数只在头部表达；0文件且0错误保持空组 | 静态接线、必要开发回归、用户手测 |
| C2 | 4+ 文件，鼠标或键盘展开再收起，多次切换 | 全量/前三行准确切换，始终有切换入口，箭头/aria-expanded一致；焦点不留在卸载控件，滚动锚点沿用原路径 | GPUI可观察交互（若新增）、静态、手测 |
| C3 | 点击文件名/目录/空白，Tab后Enter/Space | 现有 ArtifactPreviewRequested 恰好一次，完整路径可访问；无需hover先找小按钮 | GPUI事件订阅与真实输入、手测 |
| C4 | 点击菜单触发器/目标，Esc/外点，键盘遍历 | 不串触发预览；六目标仍走原open；菜单deferred、回焦/错误/禁用保持；不可见菜单不参与命中/Tab | 静态完整链、必要GPUI回归、手测 |
| C5 | 项目会话点击/键盘激活Open Review | 经emit_open_diff发出原thread/project事件，打开整体Review，不声称定位文件 | 实际事件链检查、必要GPUI回归、手测 |
| C6 | reading preview、standalone、无文件/不可用文件、opening | Review不越过守卫，禁用原因就近可获取；不可用预览不发请求，外部打开沿用已有能力判断，重复操作不重入 | 静态守卫、必要GPUI回归、手测 |
| C7 | 折叠/展开组同时包含捕获错误或隐藏文件错误 | 所有真实错误仍在组外折叠限制之外显示，取消保持现有不报错语义 | 静态、必要状态回归、手测 |
| C8 | 浅深色、窄窗、长路径、1/3/4+行，滚动/续跑 | 目录次要色、文件名主色，主体可辨识；共享token；边角不靠父overflow裁切；组高度估算与绘制一致 | 格式/静态、独审、用户视觉/真实系统手测 |

## 4. 最小实施计划

1. `vega_theme::Layout` 增加本组件行高/头高/折叠上限等确需共享的 typed token，不改全局24px ROW_HEIGHT。建议文件行和切换行36px，头部44px，边界1px；间距/颜色/字体复用现有token。
2. `artifact_card.rs` 改紧凑行布局、完整路径a11y、整行预览和稳定行/菜单焦点；保留open和menu安全逻辑。子菜单操作必须阻止冒泡，父行Enter/Space只在行焦点时预览，不能劫持菜单键。
3. 改 `ArtifactGroup` 头部和可逆折叠。新增Review状态及复用现有事件类型的发送/订阅接线：group请求必须由所属stream回调重新执行emit_open_diff守卫，不直接绕过。只读态和standalone提供真实不可用态与简短原因。空组不因为头部变成非空。
4. `composer.rs` 创建/订阅group；`core.rs` 传播只读态；`message_anchors.rs` 专用高度估算，复用既有group观察与invalidate_item，不改全局锚点算法。
5. 专职单一writer实现；按风险选择少量稳定行为回归，不恢复旧Rust测试集合，不为流程造测试，不把静态字符串检查当行为回归。停写固定内容后交非实现者独审，与同一版本Actions并行。

### 几何核算

若采用头44px、文件/切换/错误36px、外边框上下共2px：非空组总高 `44 + 36*(visible_files + overflow_toggle + errors) + 2`；`overflow_toggle = total_files>3`（展开与收起均保留），空组为0。

| 状态 | 计算 | 总高 |
|---|---|---|
| 1文件 | 44+36+2 | 82px |
| 3文件 | 44+3*36+2 | 154px |
| 4文件折叠 | 44+(3+1)*36+2 | 190px |
| 4文件展开 | 44+(4+1)*36+2 | 226px |
| 4文件折叠+1错误 | 44+(3+1+1)*36+2 | 226px |

分隔线必须包含在各行指定高度内；最后可见子行自行设置底角（父overflow不裁圆角）。实际token若微调需先同步此表，不更改验收。

## 5. 验证与交付

本地只运行低CPU的 `cargo fmt --all -- --check`、`git diff --check`、定点静态检查；不运行Vega、不编译/构建/Clippy/测试或CUA，不安装/重启应用。GPUI公开 `TestAppContext::add_window`、`VisualTestContext::simulate_click/simulate_keystrokes/debug_bounds` 与事件订阅可作为开发回归观察路线；如使用，先核真实API并通过现有Actions执行，不新增通用CI或依赖，不扩production API只为测试。

自动验证交既有 `pr-check`：fmt/全目标Clippy、Python三组、Nextest。若采用开发Rust测试，先固定原始源码/完整SHA/patch与实际云端结果，独立清理全部测试及独占接线后，对最终head再次取得完整required CI、Rust 0发现/0执行。空集不是行为测试通过。若不新增Rust测试，记录按风险选择的静态/独审与用户手测范围，不伪造动态结果。

CI每5分钟查一次，完成通知例外；失败先查原日志、区分产品/观测/验收/环境并修责任层，不放宽断言/删失败测试换绿。最终独审无缺口且最新head required成功才主动squash merge。Issue保持开放，合并后In review等用户纯手测；CI不代表视觉、系统打开/预览或剪贴板已验收。

回滚：整体撤销本卡UI与token/事件接线/估算变更，保留#542捕获实现；不单撤头部而残留高度估算或事件订阅。

## 6. 进度

- 用户确认后认领评论已发布并读回： https://github.com/puzige/vega/issues/601#issuecomment-6392270729 。标识 `vega-601-artifact-card-20261025T020614Z`。
- 阶段：专职实现已停写，待固定提交、非写入者独审及同版 Actions。改动为 `artifact_card.rs`、`composer.rs`、`core.rs`、`message_anchors.rs`、`render.rs` 和主题 token 六个生产文件；`render.rs` 仅将既有守卫方法改为 sibling 可见，不改守卫或事件含义。
- 实现停写差异（相对规格提交 `256eb7be5504a37749d107293e71a9e3b52f194d`，`git diff --no-ext-diff --no-color --binary --full-index HEAD --`）：34,407 bytes，SHA-256 `f284ad798d88f29c78d05348e8f9cf522b48949b135c997b9c5163cb26f53117`。主会话已读回核对。
- 本地定点 Rust 1.98 rustfmt、`cargo fmt --all -- --check`、`git diff --check` 均退出0；完整捕获/去重/归属方法保持不变，几何静态核算符合上表。未新增或恢复 Rust 测试及独占接线、依赖或CI；本次行为验证采用完整静态链与独审，实际原生焦点、事件、视觉、滚动与外部打开待用户手测。无本地产品执行/编译/测试/CUA。以下记录后续实际结果，不将静态检查当作编译或行为通过。

### 首轮审查与失败诊断

- 任务候选 `f5d8ae93a279ffc9e73255b43afd445a982eb4ad`，PR https://github.com/puzige/vega/pull/609 。非实现者完整静态审查无 actionable finding，但该结论不替代云端编译。
- Actions https://github.com/puzige/vega/actions/runs/40157019604 实际失败：Clippy `121060456077`、Nextest `121060456298`、required `121061143785` 均failure。Python三组实际5/15/40项OK；Rust编译失败，发现/执行计数未取得，不把日志脚本中的空集分支文本误报为0发现/0执行。
- 原始Clippy job日志100,566 bytes，SHA-256 `1dda5f82469924f602526e131dd7cb6f4b21195d1781a85476c9ab39f1073982`；Nextest日志74,161 bytes，SHA-256 `3c00dd47f8d0c89f5de3df37d6a23c7de11cc0cd89cb30c6ed71cfdd92d17a17`（`gh run view --job ... --log` 输出身份）。
- 根因是实现层Rust借用错误E0502：`artifact_card.rs:534` 在 `menu_key.read(cx).focus[1].focus(window, cx)` 同时持有不可变实体读取并尝试可变借用cx。不是测试观察故障、验收变化或外部环境问题。修复先克隆真实FocusHandle结束读取，再调用原focus，保留原键盘及所有产品验收，不改CI或断言。
- 为满足strict分支门禁，已正常合入最新master `572a3c6f1b50ea34c2214245dcd18ee3a70bb737`；保留其#607压缩布局/估算和#605侧栏修复。单一原writer只修该借用点，停写后复用独审者审修复及集成delta，推同一PR获得新SHA完整门禁。
