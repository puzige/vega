# R14 — ZCode 模型设置与可验证连接

Type: Implementation handoff
Status: Delivered locally — [root acceptance](vega-r14-acceptance.md)
Owner: 主 Agent review / integration；Astra medium implementation
Base: `f885a03`（R13 已验收）；2026-09-06 已 fetch / rebase origin/master

## 目标与参考

延续用户「逐项对比 ZCode 按钮和功能、优先功能/UI、真实用户 E2E」指令。本轮先交付 M02 和 M03 的模型测试部分：供应商启停、顺序、模型列表发现/导入和真实连接检查，同时把模型设置改成 ZCode 的供应商列表 + 详情布局。主 Agent 2026-09-06 通过原生 CUA 观察了 ZCode 自定义供应商页面与未提交的模型编辑弹窗；只确认 UI 入口，未点击真实刷新/测试、未修改参考应用或读取其密钥。参考页面有供应商启用状态、拖动排序、Base URL、Chat Completions 格式、遮蔽 API Key、逐模型测试/编辑/删除和添加模型。

上下文窗口/最大输出/多模态能力编辑留到后续单独接入真实请求与上下文处理，不能只存一个没有实际效果的设置。外观/通知、商业订阅、性能 bench/soak、本地 master / 远端发布均不在本卡。

## 产品行为

1. 模型设置内部左列为有序供应商列表，右侧为当前供应商详情。名称、启用状态、Base URL、当前支持的 Chat Completions 格式、已保存凭据状态、逐模型行分层展示。没有提供其他协议的假选项。添加/编辑供应商沿用已验证的表单能力；必须保留长模型列表和键盘输入验证。模型页空状态提供添加入口。
2. 每个供应商可启用/禁用；旧 config 未声明 enabled 时默认 true。禁用保留模型和凭据，不删除或改写已有任务。新的模型选择器与运行前解析仅考虑启用供应商；唯一性也只按启用供应商判断。已选模型不可用时显示真实不可用状态并阻止新请求，不能悄悄选择另一个模型；已运行的请求使用其冻结配置。重新启用后可重新选择。默认模型存储值无需偷偷重写。
3. 供应商可拖动排序，并提供上移/下移的键盘/点击等价入口。顺序写入配置，重启后保留，并决定可用模型列表的展示顺序。排序不得改变同名模型的唯一性规则或当前对话选择。首尾操作不可用，不允许越界/重复/丢失供应商。
4. 「发现模型」只在用户点击时读取该供应商配置的 `/models`，展示可选择的模型 ID 候选，显式「添加所选」合并到本地列表，去重且保留已有模型/顺序。不能把服务器列表直接覆盖本地模型；取消不写配置。支持空列表、无新增项、HTTP 错误、无凭据、解析错误、超时和取消。GET 列表成功只代表列表可读，不能显示模型测试成功，也不能从名字猜测视觉/上下文能力。
5. 每个已配置模型有「测试模型」：通过实际 OpenAI Chat Completions 请求路径发送固定、无用户内容的短文本检查，显示正在测试、成功或可理解的失败结果。使用新连接检查专用的有界 HTTP/stream 路径，复用现有 encoder/transport 逻辑；不得写入任务、消息、费用记录，不执行工具、不发送当前聊天内容。成功必须验证有效的模型返回，HTTP 200 空/畸形响应不得判成功。停止、请求总时限 15s、连接时限 5s、响应体总上限 1 MiB、单次无重试；防止无限流/无限错误体。模型列表上限1000、ID上限200 UTF-8 bytes，重复、空白、控制字符等沿用现有验证。设置页说明测试会发送少量文本请求。
6. 凭据从 R10 owner-only 本地凭据文件获取，仅在用户显式请求网络动作时读取。使用已有 key_ref，不访问 Keychain。凭据值不得进入 config.toml、共享 projection、Debug、日志、错误、HTTP URL或截图。错误使用有限枚举/状态码，不展示服务端原始错误体；拒绝带 userinfo/query/fragment 或非 HTTP(S) 的 base URL；不跟随重定向携带密钥到别处。
7. 所有新增磁盘/网络 IO 在后台；render 不做 IO。每个 Settings 实例只有一个模型操作在途，重复点击合并/禁用。异步结果带确切供应商快照、操作 ID 和页面 generation；切换供应商、修改/删除/禁用配置、取消、关闭设置后，旧结果不得出现在新目标、覆盖新配置或继续写入。取消可立即释放界面，晚到结果丢弃。网络返回本身不写配置，导入必须显式提交。
8. 新配置写入从磁盘重新读取并校验当前供应商基线；与编辑时基线冲突时不覆盖更新。仅修改请求涉及的配置字段，保留 defaults/ui/其他供应商/凭据引用和当前编辑草稿。成功持久确认后更新 UI 并发出 SettingsSaved 让 app 刷新目录；失败保留输入并显示重试/重新读取入口。现有 provider 编辑必须保留新增 enabled 字段，旧测试 literal 的机械字段补齐属于本卡批准适配，不得削弱原有断言。
9. 使用现有 theme tokens / icons / 文本样式。浅色和深色、960×600 与1280×750均可操作，内部列表/详情可滚动，长 URL/模型 ID 不推挤操作按钮。新按钮支持鼠标与 Tab/Enter/Space，弹层 Escape 取消且不触发背后页面动作。保留 R13 三栏工作区、组织/草稿/导航、R10凭据和设置统计。

## 架构与归属

- 新增 `ProviderConfig.enabled` 兼容默认值；本卡无 DB schema、无新依赖。
- 网络实现放 `vega_runtime`（不得依赖 UI 或 conversation），使用原有 reqwest/tokio/stream/encoder 能力。跨 UI / app / service 的请求、结果、错误放 `vega_conversation::types`；遵循既有 runtime 与 conversation adapter 分界，不增加循环依赖。
- conversation 提供 provider settings service，负责安全快照、配置 patch/冲突、凭据读取与网络入口。D 先提交明确的可编译 API 契约并通知 U；U 不自行复制一套同名 API。服务可以用现有 AppConfig 作为存储 authority，但不得将 key value 放入共享状态。
- R14-D / Astra medium：store config；runtime 网络检查；conversation service/types；app 的可用模型 catalog 与 run resolution；相关生产入口回归及 `docs/vega-r14-data-delivery.md`。UI目录不归D，必要 literal 适配通知U。
- R14-U / Astra medium：`crates/vega_ui/src/settings/` 与最小 app Settings 接线（若需要）、theme/icon token；production SettingsView 指针/键盘到真实 service 回归及 `docs/vega-r14-ui-delivery.md`。不要改 D 的 app_agent / window session catalog 逻辑；app接线新文件先与D协调。
- Root：规格、独立审查、squash integration、外部自有 E2E 数据/本地 HTTP server、native 验收、最终证据和任务队列；不直接实现产品。

两个任务各 ≤3 个实施提交。可并行做独立 UI 结构和服务；契约提交后 U cherry-pick，root 集成时去重。实现卡外问题先上报主 Agent。每个 executor 先读 AGENTS.md / docs/vega-exec-guide.md，先 fetch/rebase，专属 sibling worktree。

## 验收与证据

- D：真实临时 config / 本地 R10 credential store / loopback HTTP server，从生产 service 到实际 HTTP 验证 URL、Authorization、固定短请求、成功/认证失败/畸形/空返回/超大/超时/取消/重定向；错误输出无测试密钥；GET成功不等于POST成功；排序/禁用/旧配置/冲突/无部分写/重复模型候选合并；app 目录和运行前禁用行为，冻结运行不改。
- U：挂载真实 SettingsView，实际点击/键盘 → production service → owned config，验证选择/启停/排序/取消/发现候选导入/逐模型测试/错误恢复与旧结果隔离。不用直接改 UI 字段代替主流程；必要状态竞态 seam 清楚区分证据等级。
- Root：关闭所有旧 Vega 并核实0实例后启动新构建唯一实例。使用自有 config、合成测试 key、loopback server，原生操作供应商启停/排序、模型发现导入、连接成功和失败/取消、返回聊天目录、重启保持，核对真实 HTTP 和持久状态。不读取真实密钥、不调用收费模型。原生不覆盖的边界明确记录程序化证据。
- 门禁：fmt、strict workspace all-target clippy、workspace tests --no-fail-fast、workspace build；保留首轮完整日志、耗时、head/tree/SHA256及失败原因。历史 Git 时序故障若出现单独记录，禁止改无关代码/放宽超时/重复求绿。用户要求性能测试降优先级，继续跳过 bench/soak。

## 主 Agent 实现审查补充

网络检查继承正常 OpenAiProvider 的环境代理策略，不得无条件 no_proxy 导致测试与正常运行路由不同。显式禁止自动重试。复用现有 SSE decoder 处理合法多行 data 事件，在完整有界响应内确认真实有效输出/完成标志；128 token 检查预算内只返回有效 reasoning_content 的模型可以判连通成功，不能误报为畸形。该内容仍不得显示/保存，成功不等于视觉/上下文能力验证。网络完成后后台复验 provider baseline，避免外部修改后的迟到结果冒充当前配置成功。Provider表单保存与新patch使用同一service锁和字段authority。已有同步保存测试可以等待真实异步完成后保持原断言，不得削弱。

新增后台provider写入必须与旧侧栏/default/theme配置写入共用store配置编辑锁，read→patch→atomic write为同一受保护操作，防止全量旧快照互相覆盖或共享.tmp碰撞。各UI保存只合并自己修改的字段并校验基线，保留其他最新字段；网络不得持锁。批准D为该整合发现增加一个窄修复提交，U/S各在自己归属中接线。允许UI复用既有workspace固定版本tokio和tokio-util作为普通依赖，无新三方crate或版本。

用户已点击保存的短原子磁盘事务允许完成；关闭设置必须取消网络及候选状态，迟到save回调不能重开设置、切换当前目标或清空新草稿。不要求回滚已确认且已提交的保存。模型页表单改成点编辑再展开后，旧测试可先通过真实指针打开表单，再保留原键盘/字段/持久断言。
