# Issue 75 — 真机验收缺陷修复

## 范围
用户于 2026-10-12 授权新建工作树修复。延续 #75 的桌面及浏览器执行合同；#307 首次启用引导仍单独跟踪。

## 已确认问题
成功 ComputerUse 历史回读遗漏 truncated=false，导致安全摘要误判为损坏。执行器 MCP isError 的原因被统一错误覆盖，无法区分扩展发现、窗口观察、权限和协议失败。浏览器首次 browser_tabs 失败及桌面选窗后观察失败的具体根因尚未确定，禁止据猜测更改权限或绕过目标授权。

## 要求
1. 成功 ComputerUse 历史重载恢复合法结果；失败与拒绝结果保持原语义，不恢复临时截图或页面内容。
2. 受管调用提供明确 executor、stage、有限 error_class 和可操作的安全提示。不得持久化或向模型输出原始执行器错误中的页面、输入、令牌、用户路径及其他敏感正文；未知错误归类为 unknown，不能误称权限问题。
3. 保持目标授权、租约、接管撤权、结果未知禁止重试等既有安全合同。
4. 根据固定上游源码及新的安全诊断证据修复实际适配失败；新增明确根因时先更新本规格再实现，不猜测性扩权。

## 验收矩阵
- 成功、失败、拒绝 ComputerUse 的持久化历史回读与工具卡投影；成功场景先证明旧代码失败。
- 浏览器连接与桌面观察的分阶段错误映射；恶意或含敏感数据的错误正文不得进入安全结果。
- 动作 outcome_unknown、撤权及不自动重试回归。
- 只跑相关进程内测试，全量门禁由云端 PR check 执行。
- 发布后真实桌面追加文本、浏览器表单填入与核对、接管恢复分别验证；不可用 mock 结果冒充真机通过。

## 实施
专用子任务实现历史回读及安全诊断，主控审查集成。实际适配缺陷在证据充分后更新规格并修复。沿用现有依赖，不新增真实外部 E2E 门禁。

## 实施计划与安全诊断合同

先用真实 Store 写入并关闭重开数据库，验证 ComputerObserve 的 success/failed/rejected 历史投影，再修复成功结果的 truncated 元数据恢复；不恢复图像或观察授权。受管调用按 Cua/Playwright 与 initialize、permissions、discovery、target_list、observe、screenshot、action 阶段分类。错误仅持有有限枚举，Display/Debug/UI/模型/持久化只输出固定代码和固定建议，原始 MCP 文本与 structured 私有字段不进入错误对象。已知上游 structured code 或精确消息格式可映射；其他消息统一 unknown。取消仍沿用既有 epoch 合同；动作调用一旦进入 dispatch 路径，无论后端拒绝、取消或观察失败，均保留 outcome_unknown 与禁止自动重试。

| ID | 风险/操作 | 预期 | 自动验证层级 | 状态 |
|---|---|---|---|---|
| RF01 | 成功、失败、拒绝摘要落库后重开 | 合法 ComputerUse 历史结果，成功不再损坏 | Store + conversation | PASS（进程内） |
| RF02 | 含路径、页面、输入、令牌的未知错误 | 固定 unknown；Display/Debug 均无正文 | runtime 结果分类 | PASS（进程内） |
| RF03 | 固定上游已知错误/传输错误 | executor/stage/class 可区分，权限不误判 | runtime fixture | PASS（进程内） |
| RF04 | 动作失败、撤权、旧观察与租约 | unknown 不重试；既有收口合同不变 | runtime | PASS（进程内） |

定向命令：cargo nextest run -p vega_conversation -p vega_runtime issue75。测试只替换外部执行边界，不启动真实浏览器/桌面驱动。云端门禁与发布后真机验收由主控推进。

## 本轮交付证据

- verified_at_utc: 2026-10-12T03:12:53.182Z
- branch: feat/75-runtime-fixes；基线为已合入 #306 的 master。
- Rust 文件顺序：crates/vega_conversation/src/agent/tests/mod.rs、crates/vega_conversation/src/history.rs、crates/vega_runtime/src/computer_use/backend.rs、crates/vega_runtime/src/computer_use/diagnostics.rs、crates/vega_runtime/src/computer_use/mod.rs。
- implementation_content_sha256: 36be43f55fc4376209338e67095d917eefdfe7854410e631502a18782583a90c（按本文所列五个 Rust 文件顺序串联相对路径与文件字节）
- Rust/Cargo: 1.98.0；自动测试只覆盖进程内合同。
- 代码范围：conversation 历史投影及持久化回归；runtime computer_use 的有限诊断和实际调用路径；无 UI、配置、组件版本、执行器 argv、权限范围变更。

| 要求 | 精确命令 | 原始结果 | 日志 SHA-256 |
|---|---|---|---|
| 首次缺陷复现 | cargo nextest run -p vega_conversation -p vega_runtime issue75_runtime_fixes | exit 100；2 tests run: 0 passed, 2 failed, 872 skipped；失败分别为 Corrupt 和缺少阶段诊断 | f88a2ca940adcf85398955320553d55934384a5393ab99a7a834d854bbce163d |
| 最终本卡回归 | cargo nextest run -p vega_conversation -p vega_runtime issue75 | exit 0；Summary [2.417s] 25 tests run: 25 passed, 853 skipped | 60242a25f0aa34d6bb9bb650663ea0a3be6da63ac2d18c2a658c6711ac05b4e5 |
| 文本检查 | git diff --check | exit 0；无新增代码注释 | 无 |
| 定向格式 | rustfmt --check --edition 2024（上述五个 Rust 文件） | exit 0 | 无 |

原始日志保留在本次会话的 /tmp/vega-75-runtime-fixes-red.log 和 /tmp/vega-75-runtime-fixes-final.log。全量 fmt/clippy/nextest 未在本地运行，交由云端 PR check。只读独立审查未发现新增 P1/P2。

固定来源：[Playwright extensionContextFactory](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/mcp/extensionContextFactory.ts)、[Cua get_window_state](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/platform-macos/src/tools/get_window_state.rs)、[Cua refusal](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/tool.rs)、[Cua background input](https://github.com/trycua/cua/blob/cua-driver-rs-v0.34.0/libs/cua-driver/rust/crates/cua-driver-core/src/background_input.rs)。只解析固定 code 或精确消息格式，不复制原始建议，不自动切换输入方式。

### 尚未验证

NOT RUN：本修复版真实桌面追加文本、浏览器表单输入、接管后继续。此前真实 browser_tabs/get_window_state 失败的确切原因尚未确定；本补丁先修复历史显示和诊断丢失，不能据此声明实际适配成功。待发布后用同一授权目标复测；按新 error_class 确定根因后先修订规格，再实施必要适配。

## 第二轮：首次浏览器人工授权期限（2026-10-12）

真机安全诊断已确认首次 browser_tabs(list) 等待用户授权超过受管请求统一45秒期限后，Vega关闭执行器；迟到点击 Allow 会得到已关闭relay的错误。用户和主控批准：仅每个新建浏览器连接的首次 browser_tabs(list) 使用300秒期限。后续目标列表、观察、动作、initialize与其他普通受管请求仍为45秒；Playwright action=5000ms/navigation=30000ms不变。300秒不得由模型指定，不延长到整个client生命周期；重复调用人工连接入口不获得第二次延长期。

Pause/Stop 仍立即撤epoch并取消在途请求，连接超时或取消仍使client失效并回收owned进程。迟到响应不得恢复连接、发布观察或继续派发；超时不自动重试，不扩大浏览器或桌面授权范围。

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| BW01 | 首连响应在46秒后、300秒前返回 | 首连成功，旧45秒代码先失败 | 内存MCP+Tokio模拟时钟 | PASS（进程内） |
| BW02 | 普通请求45秒无回复 | Timeout且client关闭；后续不可复用 | 内存MCP+Tokio模拟时钟 | PASS（进程内） |
| BW03 | 首连超过300秒且晚回包 | Timeout；晚包不可复活 | 内存MCP+Tokio模拟时钟 | PASS（进程内） |
| BW04 | 首连等待期间取消，随后晚回包 | Cancelled；client失效，零后续派发 | 内存MCP+Tokio模拟时钟 | PASS（进程内） |

另一次及时选择后的 target_list/unknown 尚不能归因为协议不兼容。仅在固定上游错误外壳与扩展协议证据充分后补充有限分类；不采集原始页面、令牌或profile内容。


### 固定 Playwright 错误外壳补充

固定 core commit e8149b8257d32dcf8f72573ecc43e72439da7080 的 extension manifest 为0.4.0，connect.tsx 的 SUPPORTED_PROTOCOL_VERSION 为2，与本次随包组件匹配；此证据不等于真实 attach 成功。顶层 catch、backend formatError 与 Response.serialize 均输出以 ### Error 加换行开头的错误，无证据表明前导空行是已测 unknown 的原因。backend 可合并多个错误，相邻text块以换行拼接，API错误会添加 browserType.connectOverCDP、browser.bind 或 browserContext.newPage 前缀，CDP错误会添加 Protocol error (method): 前缀，后面可以有 Call log。

分类只读取首个text块的错误段首行，剥离固定Error/API前缀与合法CDP方法包装，绝不在后续错误、Call log、stack或页面正文里搜索关键词。匹配精确Chrome附着失败模板时归 attach_failed；No attached tab/No tab found固定错误归 target_unavailable，不认定debugger冲突；合法CDP错误但未知内因归 cdp_command_failed；上游TimeoutError归 timeout。其他继续 unknown。Display/Debug只输出有限分类与固定建议，不保存原始正文、URL、tab/session ID、用户路径或错误哈希。

上游初始 _attachTab 失败可被catch(logUnhandledError)吞掉，因此本补丁不能回溯确定那次16秒错误的根因。browser_tabs内部ensureTab可能新建空白页的既有残余仍按原规格披露，验收必须核对授权目标。

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| BW05 | 固定序列化/API/CDP包装的连接及attach错误 | 有限分类，无正文；旧分类器先失败 | runtime fixture | PASS（进程内） |
| BW06 | error后附Call log/页面/多块内容，含伪造错误关键词 | 仅错误首行决定分类，不提取后文 | runtime fixture | PASS（进程内） |

来源：[MCP server](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/utils/mcp/server.ts)、[Response](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/backend/response.ts)、[Extension连接页](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/extension/src/ui/connect.tsx)、[Chromium debugger API](https://github.com/chromium/chromium/blob/main/chrome/browser/extensions/api/debugger/debugger_api.cc)。


### 第二轮交付证据

- verified_at_utc: 2026-10-12T03:56:08.783Z
- branch: feat/75-runtime-fixes；基线为已合入#310并发布0.1.72的master。
- implementation_content_sha256: 1c004bc2507fd47096ed831bd0f1425d91f1f998e66cc372190f69ce3d9b23bf（按managed.rs、backend.rs、diagnostics.rs的仓库相对完整路径与文件字节顺序串联；目录分别为vega_mcp/src及vega_runtime/src/computer_use）。
- 仅修改受管请求期限、浏览器首次连接接线、有限安全错误分类及本规格；没有UI、权限范围、组件版本、原始日志持久化改动。

| 验证 | 命令或用例 | 原始结果 | 日志SHA-256 |
|---|---|---|---|
| 旧期限首次失败 | issue75_browser_connection_wait_accepts_manual_authorization_after_45_seconds | exit100；Summary [0.021s] 1 test run: 0 passed, 1 failed, 57 skipped | b089e1d521584948af80beb2aca9ae6849d1902981ecfc0940118c1c0b7fc1a4 |
| 期限与取消 | cargo nextest run -p vega_mcp issue75 | exit0；Summary [0.013s] 7 tests run: 7 passed, 54 skipped | 1eb351bc5cade8cc548e097f69331d70ca99caf434deea2ddc3726e02943f4ca |
| 旧包装分类首次失败 | cargo nextest run -p vega_runtime issue75_browser_error | exit100；Summary [0.020s] 2 tests run: 0 passed, 2 failed, 298 skipped | 17224ce2f6136b9d3c163278d0fb1e51ab79d773179a162933c4955a73ad2dae |
| 最终本卡回归 | cargo nextest run -p vega_mcp -p vega_runtime -p vega_conversation issue75 | exit0；Summary [2.327s] 34 tests run: 34 passed, 907 skipped | c36a3fc41c2f10ef7618c2053377c365504f375a0ae9ef9e2843b935b9aecea9 |
| 文本与格式 | git diff --check；rustfmt --check --edition 2024（本轮三个Rust文件） | exit0；无新增代码注释 | 无 |

原始日志依次为 /tmp/vega-75-browser-wait-red.log、/tmp/vega-75-browser-wait-first.log、/tmp/vega-75-browser-wrapper-red.log、/tmp/vega-75-browser-connect-final.log。独立只读审查未发现新增P1/P2。全量门禁仍在云端。

NOT RUN：本轮版本的真实首次人工连接、浏览器输入、桌面输入、接管恢复。16秒unknown的具体根因不能从已经丢弃的上游正文恢复；更新后的有限分类是下一次验收的证据入口，不等于真实适配通过。0.1.72桌面permissions_missing另由主控按canonical安装和TCC身份重新验收，本轮不放宽预检。


## PR311云端阻塞的定向调查（2026-10-12）

云端run38201841191唯一失败为issue67_concurrent_settings_fails_closed_for_both_permissions，2254 passed/1 failed/5 skipped。panic位于agent.rs原719行：释放A/B provider后等待两个权限同时pending，尚未执行打开Settings及拒绝断言。此测试、普通write与权限队列相对本轮基线未改，两CUA能力在fixture中关闭，不能将失败归因于首次浏览器300秒路径。

主控批准最小测试诊断：保留pump_test_app原400轮、虚拟4ms推进与真实5ms等待，保留所有生产及安全断言；只在此等待失败时输出A/B pending/card/active/cancelled、固定tool状态及Settings开关，不输出工具输入或内容。定向执行该用例一次；不循环重跑、不增timeout、不降低并发或改变write合同。若未复现，只报告本次结果和诊断缺口，不声称修复；只有拿到根因证据后才可追加最小同步修复。


### CI调查结果与诊断交付

- verified_at_utc: 2026-10-12T04:11:01.221Z
- 结论：root cause unresolved。一次定向运行未复现，不能据此宣称CI失败已经修复。
- 诊断补丁仅涉及crates/vega/src/tests.rs及crates/vega/src/tests/agent.rs；两文件按相对路径与字节串联的content_sha256为3fd0174e697a119eb0d259b800b185141e2b85aa1df2f4ef10ac2cf511a72324。
- 原400轮、每轮GPUI虚拟4ms推进/真实5ms等待、ConcurrentProvider真实5秒gate、并发任务及所有拒绝/零mutation断言均未改变。新增诊断只在原等待耗尽后计算，不影响成功分支。

| 证据 | 原始结果 | 原始日志SHA-256 |
|---|---|---|
| 云端run38201841191：cargo nextest run --workspace | Summary [125.191s] 2255 tests run: 2254 passed, 1 failed, 5 skipped；唯一失败在权限入队前置等待，13.089s | 63ed4268ae256d8b95787020cc0fe9c1fca94d33bdc6002c003c16dc737598f9 |
| 一次定向：cargo nextest run -p vega issue67_concurrent_settings_fails_closed_for_both_permissions | PASS [1.087s]；Summary [1.088s] 1 test run: 1 passed, 255 skipped | bc00be51d5ec364ef95f466ba927daa3d23c82d9736ad2e48d2fafe1d48d1e9e |
| rustfmt --check --edition 2024（上述两个测试文件）；git diff --check | exit0 | 无 |

原始日志分别为/tmp/vega-75-pr311-ci-failed.log和/tmp/vega-75-pr311-concurrent-repro.log。没有再次执行此用例或旧云端run来重试到绿。

ConcurrentProvider在首文本后进入真实5秒gate，release之前仍有UI与持久化等待，故存在负载敏感前提；但原失败日志没有对应gate deadline panic，不能认定它就是根因。GPUI默认单轮且不重试，亦无证据证明该13秒由多seed迭代产生。后续新head云端若再次失败，将得到A/B的pending/card/active/cancelled、固定tool状态与Settings状态，以区分尚未提议、提前终态、撤权和UI入队方向；拿到证据前不改变生产write或测试同步合同。

独立审查校正：失败诊断的工具状态白名单按真实DDL收口为pending_approval、approved、running、success、failed、rejected、cancelled；并保留absent/query_error/unexpected固定降级值。此校正只影响失败文本，未再次运行测试；原单次PASS记录保持原样。


## 第三轮：浏览器目标列表 raw 合同（2026-10-12）

0.1.73 真机在全新、未被其他自动化附着的原生 Chrome 标签上及时完成扩展选择后，BrowserObserve 约22秒返回 executor=playwright stage=target_list error_class=unknown，尚未出现 Vega 目标授权，也没有动作。已丢弃的响应不能回溯，不能唯一确认本次失败来自上游 isError 还是成功结果解析。

固定 Playwright core e8149b8257d32dcf8f72573ecc43e72439da7080 的 browser_tabs 默认成功输出为 Markdown Result 章节；Response.serialize 还可以追加 Page、Events、Modal state。旧解析器扫描整个文本所有以“- ”开头的行，将合法元信息当作额外标签后拒绝，并把 BrowserExtension 归为 unknown。tabs handler 已先采样 headerSnapshot，故 Page 不是每次首连必有；这是已确认的兼容缺陷，不是对22秒真机失败根因的追认。Extension 0.4 初始仅将 selectedTab 发送给本 relay，后续 popup 或用户拖入连接组才可扩展目标；不能用挑第一个标签来规避唯一目标限制。

仅首次连接及后续校验的 browser_tabs(action=list) 固定发送 _meta.raw=true，使用官方 backend raw 参数。固定 Response raw 过滤仅保留 Error/Result/WebMCP tools/Snapshot；list 不生成后两类，成功只包含完整标签列表，Page/Events/Modal 元信息不会混入。错误仍经过 isError 有限分类。模型不能配置 raw、tab索引或连接范围。

解析完整 raw 结果，不过滤或忽略额外非标签行：只接受唯一规范 index0/current 的安全 http(s) 标签；拒绝多行伪造标题、多个标签、无 current、畸形行、受限URL与其他多余内容。成功响应的解析失败独立记为 stage=target_parse，有限 class 为 target_count、target_format、target_not_current 或既有 target_unavailable；不保存计数、标题、URL、原始文本、路径或令牌。isError 仍是 target_list，可据此区分上游拒绝与本地成功响应解析失败。

首次连接300秒、普通调用45秒、取消后毒化及晚包丢弃、epoch撤权、owned清理、未知动作禁止重试全部保持。

| ID | 操作 | 预期 | 层级 | 状态 |
|---|---|---|---|---|
| BT01 | 首连/后续目标列表请求 | 固定raw=true；只改变列表序列化，不改变权限 | 内存MCP/调用接线 | PASS（进程内） |
| BT02 | 官方raw成功行与附加页面/事件外壳 | 唯一完整raw成功；带外壳或其他行拒绝，不能静默挑选 | runtime fixture | PASS（进程内） |
| BT03 | 多目标、无current、畸形/伪造/受限目标 | target_parse有限分类，无私有正文 | runtime fixture | PASS（进程内） |
| BT04 | 首连300秒/普通45秒/取消晚包 | 既有期限和失效合同不变 | 内存MCP+模拟时钟 | PASS（进程内） |

来源：[backend raw参数](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/backend/browserBackend.ts)、[Response过滤和序列化](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/backend/response.ts)、[tabs](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/playwright-core/src/tools/backend/tabs.ts)、[扩展初始所选标签](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/extension/src/connectedTabGroup.ts)、[扩展事件范围](https://github.com/microsoft/playwright/blob/e8149b8257d32dcf8f72573ecc43e72439da7080/packages/extension/src/relayConnection.ts)。


### 第三轮交付证据

- verified_at_utc: 2026-10-12T06:18:27.283Z
- branch: feat/75-runtime-fixes；基线 f8b7690e（已合入#311并发布0.1.73）。
- 改动为受管浏览器列表raw参数与普通校验专用方法、完整raw解析与有限target_parse诊断；未修改UI、权限、组件版本、动作派发或原始错误持久化。
- 独立只读审查未发现新增P1/P2；主控复核同意。

| 验证 | 精确命令 | 原始结果 | 日志SHA-256 |
|---|---|---|---|
| 旧代码首次失败 | cargo nextest run -p vega_mcp -p vega_runtime -E 'test(issue75_browser_raw_target) \| test(issue75_browser_connection_wait_accepts_manual_authorization_after_45_seconds)' | exit100；Summary [0.023s] 3 tests run: 0 passed, 3 failed, 360 skipped | b16b3fa6c7c37fb0e852a48e6f63fdf206a8ac61739b113e4dad15771ff8ccca |
| 修复后本卡回归 | cargo nextest run -p vega_mcp -p vega_runtime -p vega_conversation issue75 | exit0；Summary [2.895s] 36 tests run: 36 passed, 907 skipped | 664100716a849fbf0a7f263fb841bd9d148dc313168376936a5072f2da86a270 |
| 定向格式与文本 | rustfmt --check --edition 2024 crates/vega_mcp/src/managed.rs crates/vega_runtime/src/computer_use/backend.rs crates/vega_runtime/src/computer_use/diagnostics.rs；git diff --check | exit0；无新增代码注释 | 无 |

原始日志为 /tmp/vega-75-browser-raw-red.log 和 /tmp/vega-75-browser-raw-green.log。测试是内存协议及进程内业务回归，没有启动浏览器或桌面执行器；全量门禁交给云端。

NOT RUN：本修复版真实扩展选择后出现Vega目标授权、观察及表单输入。22秒unknown的实际分支仍未唯一确认；下轮target_list代表上游调用失败，target_parse代表成功响应未满足目标合同，二者均不记录私有正文。桌面hotkey/background_unavailable另按固定Cua合同调查，本提交不改变该路径。
