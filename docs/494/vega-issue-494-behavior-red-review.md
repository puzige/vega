# 首次完整行为 RED 与测试自身错误审查

审查基线为 `2971b357daac35187593c4aae3c485397a4096ae`，完整 [Actions 39367011160](https://github.com/puzige/vega/actions/runs/39367011160)：133 发现、133 执行、123 通过、10 失败、0 跳过。原始 job、逐失败名称和日志 SHA256 见[行为 RED 清单](vega-issue-494-behavior-red.json)。Clippy 另报 App 测试重复声明同一个 HTTP helper。此结果不是 GREEN，不允许删测或转正式 PR。

本记录在后续修改前形成。原始冻结清单、源断言映射和三行 SQL 更正清单保持原样。下列更正涉及测试资料、输入和物理操作位置，不再声称与原始测试逐字相同；原始源码和本次更正均须能从 PR 恢复。逻辑验收目标与原有结果断言保持，不能修改生产逻辑迎合错误资料。

## 允许先行更正的测试错误

| 测试/位置 | 错误的独立依据 | 允许的精确更正 | 保留的验收 |
|---|---|---|---|
| runtime catalog 的 grok-build-0.1 | 固定 pi 0.84.4 官方数据中，该模型在 `https://opencode.ai/zen/v1` 属于 Responses，Chat 没有该身份；其他提供方的同名模型不能借用 | 仅此表行 API 改为 Responses | 精确逻辑等级仍为 `[high]`；API/入口/模型精确匹配 |
| runtime required replay 的 DeepSeek 表行 | 固定 758 条来源中没有 deepseek-reasoner；api.deepseek.com 的 deepseek-v4-flash 有 requiresReasoningContent | 该已知严格来源表行模型改为 deepseek-v4-flash；未知代理表行不变 | 缺 raw 时失败且零工具；真实空字符串 raw 时一次执行并完成；不新增品牌猜测 |
| runtime output reserve | 通用 request helper 已给调用方上限 256，测试却只改 reserve 为 800 并期望 800；原有 `min(caller, reserve)` 行为有明确更小上限 | 本用例显式设置调用方 max_tokens 为 800 | 原来的 wire/request 800 断言不变，目录 maxTokens=100 不覆盖用户预算；生产 min 逻辑不变 |
| runtime JSON invalid table | read 的 offset schema 明确允许 integer 或 null，optional_usize 将 null 作为缺省；null 不是非法输入 | 将负例 `offset:null` 改为真正错误类型 `offset:false` | 零工具效果、未改原始 JSON；不禁止合法 null，不做参数修补 |
| composer 子集和未知模型 | 现有滑块的默认选项为独立控件；没有 Off 时第一个等级索引为 0 | 子集 high 从索引 2 改为 1；未知六档从 index+1 改为 index | 同一 high/六档逻辑选择和精确 wire 值，集合和模型列表不变 |
| settings save/reload | GPUI emit 在 update_window 闭包结束后派发；同一闭包内读取外部订阅者必然过早 | 将已有点击和观察拆成 update、排空 TestAppContext、观察；重载订阅同样处理 | 相同控件点击、重复保存禁用、generation/operation fencing、冲突草稿保留和一次重载 |
| App HTTP helper | 两个模块对同一 http.rs 各自 path mod，触发 duplicate_mod | App main.rs 声明一个 cfg(test) compatibility_http，两个测试模块导入同一 helper | helper 字节、网络替身响应和全部断言不变 |
| runtime 工具图片 | 当前 Read 明确只支持文本；PNG 返回 BinaryFile，RuntimeToolResult 没有图片输出。它不是本产品的合法图片生产者 | 保留用户/历史图片零发送分支；工具分支改用现有 ComputerUse 的真实 observe 图片链，仅外部 desktop transport 为进程内替身 | 文本模型在已执行一次工具后阻止第二次发送；图片覆盖允许第二次发送并包含原图片；不扩展 Read 产品能力 |

图片替身只能返回外部 MCP 协议响应（权限、应用/窗口列表、窗口状态和 owned PNG），不能返回已经批准的调用、Observation、ControlSnapshot 或工具终态。真实路径必须继续经过 prepare/authorize/execute、图片解码、目录/目标选择、publish、apply_observation 和请求编码。用例先通过真实目录发现获得 target_id，不直接设置批准/观察状态；拒绝意外动作及网络/OS/进程调用，限时并真实 shutdown，不能泄漏全局 owner。保留原来的成功/失败、执行数、provider body 数、PNG 未改和 wire 图片断言，不追补新的验收范围。

允许为外部 desktop IO 加最小私有传输接缝及 cfg(test) factory；所有业务决策只保留一份。该接缝不改变生产的 Desktop/ManagedClient 调用、权限/目标/epoch 检查；开发专用入口/替身须在 GREEN 后独立删除。优先 cfg(test) 支持，避免永久保留仅为测试服务的抽象或依赖。

## 真实实现缺口，暂不在更正提交内修改

1. Responses 私有 raw reasoning 与 replay layout 未纳入 OwnerCredentialProvider 的递归键/值凭据检查。既有冻结 M11 断言要求保护所有私有通道；不得改写有签名/opaque 数据或改掉期望错误。
2. 实际 manual context accounted worker 从旧 thread context_settings 读取容量，而当前用户配置在精确 provider/model 的 model_context_policies。旧表只是迁移提示，不是运行 fallback；应沿用真实当前 policy 的 input_limit/output_reserve 权威，不向测试写旧表或搬用 pi 目录容量。
3. InvalidRead 目前进入 Approved 并调用 execute_prepared 后才返回失败；应以真实 Authorization::Terminal 在执行前结束。保留失败反馈和零执行约束，不仅修改统计计数，不改变合法 nullable 参数。

先只实施上表更正并再次运行同一 133 定义，取得资料更正后的真实行为 RED 后，再针对未通过的冻结断言授权最小实现修正。不得 ignore、改阈值、删用例、修改结果期望、借 API 混用或回退配置得到绿色。若发现需要改变验收含义，停止本步骤并先记录独立新周期。

## 更正提交前的主会话复核

事前审查发布为 [PR comment](https://github.com/puzige/vega/pull/504#issuecomment-6283260542) 后，专职子 agent 只改表内10个代码路径。主会话独立读完差异和图片共享链，逐项核对所有报告路径的前后SHA、25个原artifact、原始133个文件/名称/属性组合，确认原始两个大型清单、SQL更正清单、依赖/workflow未改；三个真实缺口和既有输出cap代码仍为父提交字节。630个assert宏数量相同，只有两处批准的滑块物理index表达式变动，结果、计数和wire预期均未改。

[完整更正源码补丁](vega-issue-494-behavior-wiring-correction.patch) SHA256 `f9474c982fc7395de51e19d3d0df8bb711f1cb2e200b97adf4bc8da880e0fa36`；[原始逐文件/断言/定义审计](vega-issue-494-behavior-wiring-audit.json) SHA256 `636104610e7651fd3bdb58dc2b40f1cf4817e0601db9a647fc64f1fd6389d39d`。审计中的/tmp路径是生成时位置；公开相邻补丁和后续完整提交才是长期恢复入口。

外部IO注入只在cfg(test)存在，非test的DesktopConnection是原Desktop类型别名。进程分支保持原Desktop/ManagedClient，窗口读取只是移动原有list_apps、list_windows、checked和parse_windows；真实权限/epoch/目标/图片解码/观察发布不变。测试分支先真实发现目录，再用生成的target_id观察，替身只回ManagedResult并拒绝额外动作；原业务判断未被替身替代。开发专用trait、factory和enum将在完整GREEN后随测试删除。

主会话再次独立执行fmt check、diff check和locked/offline/no-deps metadata，均退出0。没有本地运行Vega、编译、Clippy、Rust/Python测试；尚无本次更正的云端结果。
