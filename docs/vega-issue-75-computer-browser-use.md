# Issue #75 — macOS Computer Use 与 Browser Use

状态：2026-10-12 用户授权自动选型与实装；本规格先于实现冻结。关联 [Issue #75](https://github.com/puzige/vega/issues/75)。

## 1. 目标与选型

用户选择 macOS 本机桌面与浏览器、可随时接管，并明确要求自动选择方案、把两项能力实装到 Vega。交付一条生产闭环：开启能力 → 选择并授权目标 → Agent 观察/操作/验证 → 用户接管/继续/停止。仅增加工具声明、设置占位或测试替身不构成交付。

首期选型：

| 能力 | 固定执行器 | 选择理由 |
|---|---|---|
| 电脑操控 | Cua Driver 0.34.0，embedded bounded daemon + stdio MCP proxy | 有窗口目标、AX/图像观察、受限 manifest 与宿主持有的生命周期；无需另一套模型循环 |
| 浏览器操控 | Playwright MCP 0.0.83 + 用户浏览器 Extension | 有语义快照/引用、实际 tab 选择、MCP 图像；适合受控动作适配器 |

Browser Use 0.13.11 已有独立 Browser Harness 执行层，不因自带 Agent 而排除。Harness v0.1.13 的独立 MCP 主要提供坐标/CSS、广权限 JS/CDP、路径型截图及独立 daemon；当前需求更适合已有语义引用与扩展目标选择的 Playwright。完整 Browser Use Agent 会另起任务循环，不纳入首期。以上是按目标控制、集成成本与分发条件作出的工程选择，不是实测成功率排名。

范围：Vega Native runtime、macOS、单用户、用户选定的桌面窗口和浏览器标签页。两项可以在同一任务启用，但所有实际动作串行，共享进程内独占控制权。ACP 不获得这些工具，UI 明确其当前不可用；不隐式把本机权限转交外部 Agent。

非目标：云端 VM、无人值守常驻、任意 Python/JavaScript/CDP、脚本录制、任意上传下载、全屏任意窗口控制、跨进程的多个 Vega 实例争用协调、供应商专属 computer 工具协议。

## 2. 用户流程与 UI

R1. Settings → Agents 在 Native 能力区域提供“电脑操控”“浏览器操控”。默认关闭，分别持久化 enabled；开启只是允许后续请求，不是操作授权。沿用现有 settings 导航、共享主题和布局 token。

R2. 检测页说明真实就绪状态：打包组件缺失/版本不符、macOS 权限缺失、浏览器扩展未连接、可连接、执行中、已接管、已停止或失败。设置页仅异步检测随包组件，并明确“系统权限和浏览器连接将在使用时检查”。首次实际连接检查执行器协议、系统权限/扩展连接；静态 manifest 合法不能标作服务 ready。不能通过测试响应或改用户数据库伪造就绪。发行包随附执行组件，不在普通任务中临时执行 npx、curl 安装脚本或读取全局 CuaDriver.app。开发者可按明确命令准备专用目录。

R3. 首次请求能力时显示本次运行授权：能力、目标、观察与交互范围。桌面通过只读 app/window 列表供用户选择，绑定 bundle ID、PID、window ID；用户不能被要求手填 PID。浏览器连接扩展，用户选择实际 tab，Vega 再固定其身份。连接等待可取消，不能锁住 UI。窗口标题/页面标题/URL 是外部数据，不能变成指令或授权。

R4. 用户授予本次任务范围后，专属动作无需对每次点击重复确认。ReadOnly 不能执行 mutation；Ask/Plan 等现有模式不被能力开关绕过。未授予的范围、目标变更或需要新的影响范围时重新进入授权；普通 MCP 仍按既有 Once 策略，每个原始 server/catalog 不获得本次授权。

R5. 当前任务可见状态条显示正在操作的目标，并提供“接管”“继续”“停止”。接管是可信 UI 控制，不是向模型发送自然语言建议。接管后保留任务和独占控制权；继续需要新的观察；停止与现有 Stop 一致地结束运行并释放资源。模型不能调用 resume 或授予自身权限。

R6. 任务结束、切换执行路由、关闭任务/应用时回收会话。切换可见任务不能让控制状态不可见；若现有 UI 无法持续展示后台控制，应先暂停。设置可恢复，grant、epoch、tab 连接、元素引用、截图不跨运行或重启恢复。

R7. 最新观察从本次运行控制条查看，显示真实来源与状态；工具卡显示安全动作与结果摘要。原始图片只在本次运行有界内存中保留，历史只保存安全工具摘要和“图像观察未保留，继续时重新观察”。不把工具观察写成用户输入，不参与自动标题的用户消息提取。

## 3. 架构与权限合同

R8. 向 Native 工具目录注册专属有限工具，例如 `ComputerObserve`、`ComputerAct`、`BrowserObserve`、`BrowserAct`。模型只提交动作语义与最近 observation ID；PID、window ID、tab identity、run ID、session label、epoch、可执行文件路径由宿主绑定。禁止将原始 MCP catalog 直接当作能力工具集。

R9. 每个运行持有控制状态、epoch、目标与 CancellationToken。状态至少区分未连接/待授权、运行、暂停中、已接管、停止中、已停止、失败。派发动作前验证 run、授权、epoch、目标存活与观察世代；同一运行禁止并发 mutation。观察后的页面/窗口/导航变化使旧引用失效。

R10. 接管先同步撤销 epoch 与新动作入口，再取消在途请求。晚到响应不得恢复执行权或成为可操作的最新观察。继续创建新 epoch，先观察后 mutation。暂停期间独占 lease 不释放，避免另一个任务在用户接管时操作。停止/终态才释放。

暂停中不能继续；停止中不能释放 lease。只有旧 mutation 已 settle，或本运行持有的执行通道已关闭且不可能继续派发，才可进入已接管/已停止。等待授权、连接、观察的晚到结果同样必须验证 epoch 和运行终态。测试覆盖暂停后立即继续及 A 停止中 B 争抢 lease。

R11. 停止先拒绝新 dispatch 和自动重试，取消请求、结束后端会话并回收仅由 Vega 持有的 child。保留用户 Chrome、标签页与桌面目标应用。不能按进程名批量杀进程，不能把杀 proxy 当作 daemon 已停。操作结果分别表示未派发、已完成或结果未知；已经交给 OS/网页的动作不能撤销，结果未知不自动重试。

R12. 外部页面、AX 文本、截图及 MCP 返回值均为不可信观察。系统提示与 provider 转换保留这一来源，不允许其改写工具权限。广权限执行、任意文件传输、credentials/cookies/storage 工具、页面注册的 WebMCP 工具不进入初始动作集合。

R13. 运行中的 UI 无法完全原子锁定。首期接受人工同时改变页面/窗口导致的残余竞态，执行器和 Vega 均尽量检查目标并在动作后观察；不声称 frame 检查或停止能撤回已发送事件。目标不确定、权限失败、schema 不兼容时停止该能力，禁止静默退回全屏、任意 tab、全局 daemon 或未授权 delivery。

## 4. 桌面执行器

R14. Vega.app 直接启动固定资源中的 driver（argv，不经过 shell）：

```text
cua-driver serve --embedded --host-bundle-id ai.vega
  --socket <私有run目录>/driver.sock
  --permission-mode bounded --capability-manifest <run manifest>
  --approve-capability-manifest
cua-driver mcp --embedded --host-bundle-id ai.vega --socket <同一socket>
```

运行目录 owner-only，socket/manifest 不接受模型输入。`host-bundle-id` 与 `session` 都不是认证凭据；真正的权限归属依赖从 Vega.app 发起的进程链。不得用默认 macOS `mcp` 路径连接全局 CuaDriver.app。

R15. 选择目标前，只读 discovery 仅可列 app/window 元数据，不可截图/输入/启动/终止 app。固定版会将无 PID/window 的枚举归为 display observation，因此 discovery manifest 明确仅 allow.tools=[list_apps,list_windows,check_permissions]、resources.desktop.display=true、expires_after=5m、idle_timeout=2m；display 字段不能被解释为同时允许未列出的截图/输入工具。用户选择后停止 discovery daemon，创建全新 bounded manifest，resources.desktop.windows 只列选定的 {pid,window_id}、display=false，不授 apps.windows:all 或 desktop.applications。Vega 同时绑定所选 bundle/PID/window 身份。manifest 用序列化生成，不能插值任意文本；app 标识不支持 glob。运行 manifest 可到期2h、空闲15m，过期后重新授权，不自动续权。

R16. 初始 backend tool subset 为 check_permissions/list_apps/list_windows/get_window_state/click/type_text/press_key/hotkey/scroll。连接时只读检查 check_permissions(prompt=false, probe_direct_capture=false)，缺少 Accessibility 或 Screen Recording 权限时给出可见失败；不调用会转向全局应用的 permissions grant。`get_window_state` 绑定 PID/window ID，包含 AX 树和截图并限制深度/元素/图片尺寸。输入使用 `target:{kind:"window",pid,window_id}`，坐标或 element_token 二选一，显式 delivery_mode；元素来自最近快照。MCP click 坐标为顶层 x/y，不能照搬 Rust sum type 的 position 包装。

R17. 首选后台定向 delivery。若目标不支持，应返回可操作失败，不能静默改变输入范围。需要前台能力的后续调整必须显式体现在授权和规格中。坐标从模型看见的图片映射回 get_window_state 返回的 PNG 像素，使用 original_width/display_width 与 original_height/display_height 比例；capture_id 路径由 Driver 反转其内部缩放和 Retina backing scale。Vega 不再提前转为 macOS points，不加全屏 origin。输入使用 typed target 时不得混入 legacy pid/window_id/scope 字段。

R18. 关闭时尽力 end_session，再执行 `cua-driver stop --socket <owned> --expected-pid <owned pid>`，限时等待后只终止持有的 child handle，最后关闭 proxy、清理私有运行文件。daemon 崩溃/断连后不自动恢复授权。

## 5. 浏览器执行器

R19. 使用随包 Node 与固定 npm 依赖树的 Playwright MCP cli，参数包括：

```text
--extension --no-webmcp --image-responses allow
--timeout-action 5000 --timeout-navigation 30000
--output-dir <私有run目录>
```

不下载自带 Chromium；复用用户 Chrome 与扩展选定 tab 的登录态。扩展独立更新，不宣称 npm lock 固定了扩展；连接时记录可取得的版本/能力并检查所需工具 schema，不兼容则可见失败。

R20. 初始允许的适配动作仅为观察、导航、基于快照引用的点击/输入、按键及必要的语义滚动。真实 backend subset 使用 browser_snapshot、browser_take_screenshot、browser_click、browser_type、browser_press_key、browser_navigate，宿主内部仅用 browser_tabs 的 list 校验扩展选择的唯一目标。v0.0.83 的点击/输入参数是 target，不是旧 ref。截图 viewport、CSS scale，不开放 whole-page 无界截图。

R21. 一个 Vega run 一个 MCP client/process。只有宿主可选择 tab；模型不能 new/close/select 任意 tab。目标绑定不能只依赖可变 index 或 URL；结合扩展连接身份、列表/选中状态与 snapshot 世代检查，关闭、切换、popup 跟随等目标变化使旧观察失效并进入重新授权/连接。若上游接口缺少不可变身份，必须在实现记录中明确剩余限制，不能宣称它是浏览器级强隔离。固定上游的 browser_tabs(list) 内部调用 ensureTab，用户关闭目标后可能先新建 about:blank；Vega 拒绝该非 http(s) 目标并断开，不能承诺绝不出现新空白页，也不关闭用户标签。

R22. 不暴露 browser_evaluate、browser_run_code、browser_run_code_unsafe、任意 CDP、文件上传、cookies/storage、页面 WebMCP。首期不启用坐标 vision 动作；遇到 DOM 不可定位的 canvas 等页面可截图说明并交给用户接管。已授权导航允许页面正常跳转；URL allowlist 不是完整网络安全边界。

R23. 停止断开并回收本次 Node child。`browser_close` 会关闭页面，不能作为保留用户 tab 的停止方式。在途动作结果未知时不重试。扩展断连或目标消失后恢复须重新观察/授权。

## 6. 图像与模型输入

R24. 工具结果图片必须是 typed ToolObservation，关联 call ID、能力、run/epoch、observation ID、目标与原始/显示尺寸。复用现有 ImageAttachment 的安全解码、像素和编码校验；拒绝损坏、超限、伪 MIME 或不支持格式。图片/debug/error/log 不能输出原始 base64 或敏感内容。

R25. managed backend 需要独立的有界多模态 MCP 解析/传输选项；普通 MCP 的文本预算与授权不因此放宽。标准 text/image content 与 structuredContent 分别解析，保留 isError/降级/拒绝语义，不把 RPC 完成直接标为动作验证成功。Cua 固定版返回 MCP 2025-06-18，专用连接需要正确支持这一修订；不得套用现有最新修订的 resultType 规则。其他协商版本只增加有实际后端依据且有回归的支持。

R26. 限额：输入单图最多 8 MiB、16 MP；展示/模型图最长边 1600；本次工具观察最多保留最近 2 个、图片总计 8 MiB。达到总预算时移除较旧图片并保留 call ID 与丢弃说明。用户上传图片预算与历史不静默改动。base64 膨胀计入 managed transport 限额，禁止逐块合法但总量无界。最终 wire 限额与数据归属须在测试中对应。

R27. 内部历史保持 Tool 来源。Responses 的 function_call_output.output 使用 input_text/input_image 数组，image_url 为 data URL、detail=auto；Chat Completions 使用文本 tool result，整批 assistant tool_calls 的所有 tool 结果完整输出后，才在 provider codec 中投影显式标记为 external tool observation、关联 call ID 的多模态 user wire 段。不得在并行 A/B tool 结果中间插入 user 段。后者只是 API 编码适配，不能插入持久化用户消息、获得用户授权或参与标题提取。provider 不接受视觉时可见失败，浏览器文本 DOM 路线仍可用；禁止把 base64 作为普通文本发送。

R28. 截图和完整 AX/DOM 观察正文仅在 live run 有界缓存内；durable result summary 与 ephemeral observation payload 分离。completed map、事件、outcome 与存储只持元数据/handle，不能逐步 clone 原始图片而绕过缓存上限。不改 user-only image_attachments 表，不新增截图 blob。上下文统计、hash/fingerprint、压缩/裁剪必须认识工具图片；同尺寸不同内容也要改变 fingerprint。既有基于编码字节的估算不准确时保持估算标识，不直接声称真实视觉 token 数；可采用有界 JPEG 再编码降低流量，仍需完整记录尺寸与图像质量限制。压缩回读替换 messages 时使旧观察失效并要求 fresh observe，运行重开同样不恢复截图/元素操作权。

## 7. 组件交付与依赖批准

R29. 发行 bundle 携带 Cua Driver 0.34.0、Node LTS 24.21.0、Playwright MCP 0.0.83 与完整 npm lock。外部组件为本卡批准的运行依赖，不新增另一个 Agent/模型 SDK。组件来源仅官方发行资产/npm，下载校验固定 SHA-256 或 lock integrity，提取检查路径，保留许可证；不执行上游 install scripts，不运行 package 的全局安装器。Node darwin-arm64.tar.gz 的官方 SHA-256 为 bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057；Cua universal-binary.tar.gz 为 940dc008e0f7c5d217d14c0f247d1ebab91b1bac965f4a649d19e8c789bdfd81。资产选择若改成平台 npm 包，须登记实际官方 digest 并保持版本不变。Vega 现有发行平台是 macOS arm64，本卡不另开 x86 发布流水线。

R30. 固定相对布局：`Contents/Helpers/cua-driver`、`Contents/Helpers/node`、`Contents/Resources/computer-use/playwright/cli.js`，另有 schema/version/entrypoints 的 manifest。playwright/cli.js 为本仓极小 JS 入口，加载完整 node_modules/@playwright/mcp/cli.js；不能只复制上游 cli.js 而丢其相对依赖。只从真实应用 bundle 或显式开发 prepare 目录发现，不能 PATH 扫描到别的全局工具冒充受管组件。

R31. 打包阶段准备依赖并验证，运行阶段不下载/升级。签名按 nested executable → app 顺序，保留现有独立更新签名；Node 所需 hardened-runtime entitlement 按官方发布要求处理。无 Apple 凭据时保留仓库已有 ad-hoc 路线，不能宣称取得公证。只针对本卡的打包纯函数/manifest/失败路径加自动测试，不新增真实网络/外部应用 E2E 门禁。

R32. Rust 依赖优先复用 workspace 既有 serde/serde_json/tokio/tokio-util/image/base64/sha2/libc 等。新增第三方 crate 须主控在规格登记批准；runtime 不依赖 GPUI。跨 UI/controller 共享类型经 vega_conversation::types 暴露，底层 runtime 类型遵循现有 re-export 模式，禁止构建循环依赖。新增代码无注释，只有 unsafe 的 SAFETY 例外。

## 8. 实现与所有权

1. 主控冻结本规格、验收矩阵和依赖合同，处理设计裁决、review、PR/CI/合并。
2. 专职 core agent 在本卡主工作树实现 runtime 控制、MCP 图像、provider、conversation/store 配置、真实后端 wiring 和 UI；按状态机 → 图像 → 后端 → UI 的顺序提交可审查结果。
3. 专职 packaging agent 在独立工作树实现 scripts/组件清单/npm lock、xtask package 与 release nested signing。不得修改 core 源码；主控在 core 停写后整合。不同 agent 不同时写同一工作树。
4. 只读 reviewer 检查图像/provider/权限和 diff。所有执行器 adapter 需对实际 schema 写有界 fixture 测试，不能仅测试空 trait 或 mock 的返回值。
5. 只跑本卡定向测试；全量 fmt/clippy/nextest 留云端。一个 Issue 一个 PR，≤3 个最终提交。PR 关联但不自动关闭 #75；CI 绿后 squash merge，Project In review，等待用户真实手测。

## 9. 实现前验收矩阵

所有用例初始状态为待实现/待验证。自动测试使用进程内业务、安全、存储、GPUI 测试替身；不得把 mock 结果写成真实 OS/浏览器成功。

| ID | 前置/操作 | 预期可观察结果 | 测试层级 |
|---|---|---|---|
| CU01 | 默认配置、禁用能力、ACP/ReadOnly/Ask 路由请求 mutation | 不获得未授权执行能力，现有模式合同保留 | runtime/conversation |
| CU02 | Settings 开关保存并重启服务 | 设置恢复；grant、epoch、连接、图片均不恢复 | config/conversation/UI |
| CU03 | 未授权请求、批准指定窗口/tab、再次请求新目标 | 首次等待；仅该 run/目标可操作；新目标重新授权 | production controller + fake backend |
| CU04 | 两个任务竞争，持有者暂停再停止 | 唯一 lease；暂停不让出；停止后可获得新 lease | state/concurrency |
| CU05 | 接管时在途动作返回、模型连续排队动作 | 先撤 epoch；零新增 dispatch；晚到响应不能解锁 | cancellation/state |
| CU06 | 继续后直接用旧 observation/element/坐标 | 拒绝；fresh observe 后才允许 mutation | adapter/runtime |
| CU07 | Cua 输入目标/字段/delivery 错误、窗口消失 | 正确固定 argv/schema/manifest；拒绝越界，无全屏回退 | adapter fixtures |
| CU08 | 浏览器 index变化、关闭、popup、导航、用户切换目标 | 旧引用失效；不得自动改控别的 tab | adapter fixtures |
| CU09 | MCP 同名工具、未知工具、JS/WebMCP/上传请求 | 受管身份不由名字决定；不透传；普通MCP仍Once | tools/permissions |
| CU10 | 合法text+image、伪MIME、损坏、超像素、超base64总量 | 有界解析；拒绝错误；safe Debug；普通MCP旧限额不变 | MCP/image unit |
| CU11 | 同一截图输入Responses与ChatCompletions | 正确wire格式/顺序/call ID；内部仍Tool来源 | provider request fixtures |
| CU12 | 3次观察/超8MiB、压缩、历史恢复、标题生成 | 最多2图/8MiB，预算与hash更新；无伪用户消息/图片落库 | runtime/context/conversation |
| CU13 | pause/stop/关闭/失败，子进程晚退 | 取消与只清理owned资源；保留目标app/tab；unknown不重试 | lifecycle fake transport |
| CU14 | 缺资源、版本/schema错、TCC拒绝、扩展未接、重连失败 | 真错误与下一步可见，UI不锁死、不伪造ready | service/GPUI |
| CU15 | package版本/哈希/路径坏、锁文件漂移、签名顺序 | 有界fail closed，固定资源布局，不执行install scripts | packaging unit |
| CU16 | 浅深/窄窗状态条，接管/继续/停止点击与键盘 | 真实controller状态与操作可达，共享token，原Stop回归 | GPUI |
| CU17 | 合并后的Vega.app，用户授权TextEdit窗口并输入/观察/接管 | 原生操作生效；其他窗口不被改控；权限归属可解释 | 用户真实手测 |
| CU18 | 登录Chrome选tab，多步表单/导航/接管/关闭tab | 登录态保留，定位正确，停止后无新增动作、Chrome保留 | 用户真实手测 |

交付记录应另列具体定向命令、首次失败原因、最终原始输出片段/hash、CI链接、实现与合并身份。真实 TCC、签名责任链、扩展竞争与 GUI 成功率在用户手测前统一标 NOT RUN，不以源码检查冒充。

## 10. 固定版本证据

- [Cua embedded SDK](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/guides/use-the-sdk.mdx)、[daemon/stop](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/reference/cli/daemon.mdx)、[MCP proxy](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/reference/cli/mcp.mdx)、[permissions](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/guides/permissions.mdx)。
- [Cua click](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/reference/mcp-tools/click.mdx)、[keyboard](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/docs/content/docs/cua-driver/reference/mcp-tools/keyboard.mdx)、[window contract](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-contract/src/windows.rs)。
- [Playwright MCP 0.0.83](https://github.com/microsoft/playwright-mcp/blob/v0.0.83/README.md)、[package依赖](https://github.com/microsoft/playwright-mcp/blob/v0.0.83/package.json)、[匹配Extension合同](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/extension/README.md)。
- [Browser Harness v0.1.13 MCP](https://github.com/browser-use/browser-harness/blob/v0.1.13/docs/MCP.md)、[Browser Use三种接入中的CLI MCP](https://github.com/browser-use/browser-use/blob/0.13.11/browser_use/mcp/cli_mcp.py)。
- [Cua discovery观察授权](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs#L1951)、[bounded window权限](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/session_manifest.rs#L169)、[Cua initialize协议](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/protocol.rs#L345)。
- [Responses正式schema](https://developers.openai.com/api/reference/typescript/resources/responses)、[Chat Completions正式schema](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)、[Node 24.21.0官方校验和](https://nodejs.org/dist/v24.21.0/SHASUMS256.txt)。

## 变更记录

- 2026-10-12：按用户 goal 授权冻结首期两项能力、自动选型、run-only 观察与 provider 投影、受管组件分发、18项验收矩阵。后续发现上游合同矛盾时先记录规格修订再实现。

实施澄清（2026-10-12）：暂停撤销 epoch 子 token，Stop 和终态才取消 root token；Paused 阻塞下一模型轮与工具派发且保留运行。原目标仍存活时继续重建连接并 fresh observe，不重复授权；目标变更或 grant 过期重新授权。审批/继续回调同时核对 captured epoch 与目标身份。受管截图允许短命 owner-only 运行目录；PW output-dir、Cua TMPDIR 均限定在该目录，截图消费后立即清理，owned child 退出后清理目录；不保留截图历史。启动只传必要 HOME、LANG 和私有 TMPDIR，不继承 NODE_OPTIONS、NODE_PATH 或 PLAYWRIGHT_MCP_*。

补充裁决（2026-10-12）：BrowserObserve 的 include_image 默认为 false，strict wire schema 将其列为必填布尔值并说明 false 是文本模型路线；DOM 始终进入模型，UI 仍可查看最新截图，BrowserAct 继承上一观察的选择。桌面要求视觉支持。未创建 named Cua session，因此无需 end_session，运行生命周期由私有 bounded daemon、owned stop 和回收控制。首期仅接受执行器列出的一个 current tab；用户浏览器可保留其他未连接标签。动作前检查连接与唯一目标，动作后允许同目标正常标题/http(s) URL变化，popup/多tab/关闭导致 Unknown 并重新连接。上游没有 immutable tab ID，无法检测同一 index 在检查间隙被替换的全部竞态；不宣称浏览器级强隔离。
