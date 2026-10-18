# Composer 下一轮输入预测

状态：v1，2026-10-18 冻结实施。关联 [Issue #469](https://github.com/puzige/vega/issues/469)。用户确认默认开启，复用当前会话模型，保留设置开关。

## 1. 目标与范围

Native 主会话成功结束后，为空 Composer 生成一条可编辑的下一轮输入建议。建议以弱提示色显示；Tab、→、Enter 接受为草稿，不发送。既有发送、队列、steer、附件、权限和中文输入法语义继续有效。

只覆盖新完成的、用户发起的 Native 主会话轮次。失败、主动停止、plan 模式、待审批/待回答、自动任务、子会话和 Codex ACP 不生成；不为打开历史会话补生成。ACP 没有可复用的独立预测协议，本卡不启动外部应用/新 ACP 会话，也不改用另一个模型。Vega 没有预测 fork 接口，使用独立的只读请求；不承诺缓存命中。

视觉采用 Vega 现有 TextInput 排版、`text_tertiary` 和 Composer 几何，不采用参考应用解包产物的 CSS、内部 token 或资产。请求和结果不增加用户/助手消息、活动条目、线程或工具记录。

## 2. 验收矩阵（实现计划之前冻结）

自动断言仅在 Actions 运行；本地静态检查不能替代下面的行为证据。

| 用例 | 前置状态与操作 | 可观察结果 | 层级/证据 |
|---|---|---|---|
| P1 正常生成 | 开关开；Native 主会话用户轮成功结束；空 Composer | 用当前提供方和模型请求一次；显示建议；真实输入内容仍为空，不能发送 | conversation MockProvider + app/GPUI |
| P2 接受 | 建议可见，分别按无修饰 Tab / → / Enter | 一次填入完整建议，光标末尾，可继续编辑；无提交事件、无新增主轮请求 | GPUI 真实按键路径 |
| P3 原键位 | 没有建议，或按 Shift/Alt/Ctrl/Cmd 修饰键 | Tab 遍历、→ 移动、Enter 换行、Cmd+Enter 发送等原行为保留 | GPUI 回归 |
| P4 草稿保护 | 请求期间输入，或已有空白/普通文本草稿，或删除输入回空 | 不展示/不接受旧结果，原草稿保持；删除回空不复活已失效预测 | app/GPUI |
| P5 中文 IME | 建议可见后开始合成，含中文及 emoji UTF-16 范围、候选确认 Enter/Tab | 合成期间不显示或接受建议，候选确认不触发建议填入/提交；最终用户输入保持 | EntityInputHandler + GPUI |
| P6 附件与阻塞 | 有附件、导入中、排队/steer 消息、待审批/回答、提交/模型切换/上下文操作中，或只读预览 | 不发起或展示预测；普通操作不受影响 | app/GPUI |
| P7 生命周期 | 发送、开始下一轮、切换线程/模型/模式/后端、删除线程、关闭窗口、Esc 丢弃 | 立即清除/取消旧请求；迟到响应不投到新身份；无历史补生成 | app/GPUI |
| P8 开关 | 旧配置缺字段；通用设置关闭/重开 | 旧配置默认为开；保存不覆盖无关配置；关闭立即清除并取消，关闭期间零新请求；重开等待下个成功轮次 | config + settings + app |
| P9 失败与弃权 | provider 错误、超时、取消、空输出、null、畸形/额外字段 JSON、过长、工具调用、未正常结束 | 静默弃权，不影响主会话成功状态或显示模型错误；不重试 | MockProvider |
| P10 使用量与隔离 | 请求成功、弃权或收到 usage 后失败 | usage 按现有辅助调用语义计费，缺价格保持未知；消息/主轮状态不变，无内容或凭据日志 | conversation/store |
| P11 非目标轮次 | 失败/停止、plan、自动任务、子会话、ACP、打开历史 | 零预测请求，已有功能保持 | app/GPUI |
| P12 UI 可发现性 | 浅深主题，窄窗口，长中英文建议 | 建议与 placeholder 不重叠、无强制布局跳动；有简短 Tab 接受提示；复制/选区/IME 仍只读取真实草稿 | GPUI + 用户手测 |

## 3. 配置与 UI

- `AgentConfig.composer_predictions_enabled: bool`，serde 缺字段及 Rust Default 都为 `true`；不新增依赖或数据库 schema。
- 通用设置提供“输入预测”开关。说明只保留影响决策的一句“使用当前模型，可能产生额外用量”；控件支持鼠标及键盘，保存中禁用，保存失败沿用真实错误/恢复入口。
- 保存沿用 `SettingsView::save_preferences` 的逐字段并发冲突检查，不能重写提供方等无关字段。开关变化传到所有已缓存主会话并撤销预测；不会触发补生成。
- 预测只在真正空文本、无 marked/IME 状态且其所属会话可交互时出现。空白字符也算草稿。弱提示不进入 `TextInput.content`、复制、选择、无障碍真实值或历史。
- 显示时原 placeholder 被替换，保留简短“Tab 接受”提示；不增加进度、成功条或常驻背景解释。无预测时几何和原键位不变。
- 仅无修饰的 Tab/Right/Enter 接受；事件被消费且仅修改真实文本。Esc 丢弃当前建议并使该请求失效。输入法处理优先于接受；开始 marked composition 即清除当前建议，不因 compositionend 恢复过期建议。

## 4. 生成与隔离

- 在成功主轮真正结束、主状态完成投影及队列处理后才申请预测；主对话的 Finished、停止和释放资源不等待预测。一个成功轮次最多一次，不重试、不采样。
- 捕获线程、完成轮次/消息、模型、提供方、配置/请求世代与输入 revision；发起及接收时都核对。线程/模型/模式/后端变动、输入、附件/阻塞状态或开关变化撤销请求。不能只比线程 id 或 text.is_empty()。
- 所有者在 UI 控制层管理取消，预测只持自己的 CancellationToken，不取消主轮；父轮被取消则不启动预测。窗口/实体销毁释放请求资源。换会话不把旧结果投到新会话。
- 使用本轮相同模型及提供方，兼容既有 Chat Completions/Responses 提供方；不换小模型，不启动 agent loop、工具、MCP、shell 或 ACP。复用/采用相同凭据检查与异常授权过滤，缺失/歧义/撤销即弃权；禁止用简化裸 provider 绕过 OwnerCredentialProvider 和 pre-attempt guard。
- 在 conversation 服务层读取有限的、已完成的最近 user/assistant 可见正文；以现有 privacy 投影过滤。最多 16 条、每条 4,000 Unicode 字符，总计 16,000 字符；按最近优先保留后恢复时间顺序。不加入工具输出、thinking、隐藏系统指令、附件原始数据或审批合成指令；模型请求中把记录明确作为数据处理。
- 系统提示要求预测用户下一条可能输入，跟随会话语言；没有明确有用建议时返回 `{"suggestion":null}`。只接受一个严格 JSON 对象，且唯一字段 `suggestion` 是字符串或 null；空白、控制字符（换行/Tab 以外）、超过 200 Unicode 字符或非法结构均弃权，不猜测提取。
- 请求不传工具、不启用重试；使用 `chat_stream_once`，512 输出 token 上限，15 秒总超时（含建流和读流），独立取消。仅正常 End、无工具调用且完整流结束才接收；忽略 reasoning/replay 元数据，不使用其内容代替建议。
- 返回的 suggestion 仍经过凭据/隐私过滤；无法安全投影则弃权。预测失败静默，仅可有不带内容/URL/key 的固定诊断；不改主对话错误或重试状态。
- 收到的 usage 沿现有 `automatic_titles` 辅助计费方式记录在原线程，`message_id = None`；包含缓存 token 和可用价格出处，不伪造未知价格。不增加 shown/accepted 埋点或服务端实验配置。

数值预算用于资源边界而非新 UI 几何：16 × 4,000 = 64,000 候选字符，再限制总计 16,000；200 字符建议不超过请求正文边界；512 token 是输出上限，不宣称与字符一一对应。沿用输入框尺寸和排版，不新增像素断言。

## 5. 实现计划与文件边界

1. `crates/vega_store/src/config.rs`：增加默认开字段、配置兼容/往返断言。
2. `crates/vega_conversation/src/composer_predictions.rs`（新）、`lib.rs` 与 `types/` 导出：实现有界上下文、严格解析、独立取消/超时、无工具 provider 调用和辅助用量持久化；新跨 crate 类型统一从 `vega_conversation::types` 导出。生产 API 只服务真实控制器，不为测试扩大接口。
3. `crates/vega/src/window/composer_predictions.rs`（新）、`window/mod.rs`、`window/agent.rs`、`app_agent.rs` 及需要的现有 session/navigation 集成点：管理成功轮次申请、provider 权限、generation/revision/身份 fences、独立后台任务。可传递本轮已授权 provider，或按相同权限路径重新解析；不绕过 guard。仅此卡最小修改。
4. `crates/vega_ui/src/text_input/{mod,state,input_handler,element}.rs` 与 `conversation_stream/{mod,core,composer,composer_actions,render}.rs`：ghost 状态、渲染与真实按键接受；集中 eligibility/invalidation，沿现有输入 observe、附件、模式、后端/模型及队列更新接入。
5. `crates/vega_ui/src/settings/{preferences,render_impl}.rs` 与配置加载/SettingsSaved 现有入口：可立即生效的开关与正确保存语义。
6. 对应模块测试和 `crates/vega/src/tests/composer_predictions.rs`：覆盖上表 production controller、GPUI 按键及 provider 边界；必要时扩展既有辅助调用 test fixture 的路由，不放宽主轮断言、不消耗主轮 scripted rounds。测试名描述行为，不引入历史 issue/task id。

不改依赖、工作流、真实用户配置、已安装应用、其他任务/正文布局或安全策略。代码不新增注释，生产代码不用 unwrap/expect；主题/排版复用共享 token。

## 6. 验证、交付与回滚

本地仅 `git diff --check`、差异审查及 `cargo fmt --all -- --check` 等快速静态检查；不运行 Vega、编译、Clippy、测试或打包。

Actions：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo nextest run --workspace`。未执行项标记待 CI；最终以待合并最新 head 的 `check (fmt, clippy, test)` 为门禁。不新增真实进程/网络 E2E。

用户手测：Native 成功轮后确认灰色建议；分别以 Tab/→/Enter 接受并编辑后用原发送动作发送；中文候选确认、附件、队列/steer、切换会话、停止/失败及关闭开关各走一次；确认无自动发送、草稿丢失或旧建议回流。浅深主题及窄窗口查看 placeholder/提示。

回滚：先关闭开关停止请求；需要代码回退时通过 PR revert 本卡 squash commit。新增配置字段为可选 serde 默认，不涉及 schema 回滚。代码合并后 In review、Issue 开放、保留分支/worktree，等待用户明确手测结果。
