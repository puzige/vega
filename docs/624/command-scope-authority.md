# D4：单次命令范围授权基础层

## 阶段边界（2026-10-27）

基线 `4251682ba8c86dd82eb55f8f2e10fedfdae6f20b`。本阶段实现独立的请求/决策类型、live-run owner、持久审计 CAS 与不可重建/不可重复消费的内存许可。它不是扩围功能上线：不修改 Bash 输入 schema，不接通 runtime/UI，不创建 Broker/CA/lease，不增加 profile 权限，也不启动任何进程。后续接线必须先完成独立 scope 提示、dispatch 消费和实际执行策略的完整链路，不能用本阶段的审计行当执行许可。

目录扩围的规范化、dev/ino、保护根与 hardlink 检查尚未完成，因此本阶段服务仅接受 network=true、write_roots 为空的请求；非空目录请求明确失败，不静默丢弃。完整 D4 目录契约不变。Execute+FullAccess 不进入此服务；Ask/Plan/Execute ReadOnly 仍只描述本地文件权限，不把联网说成远端无副作用。

## 数据与边界

接线增量沿用 D4 请求、审批与执行状态机：`requested_scope` 随完整 Bash 输入持久化；独立范围队列只有 AllowOnce/Deny/Timeout，监听者更换、取消与卡片消失都失效。准备好的执行策略必须等于已接纳 fence 推导的 ReadOnly/WorkspaceWrite，bindings 必须等于本次规范化范围的快照；任一失配在请求入库前拒绝。单次许可消费后才创建网络代理、自检或启动命令，失败不退还许可。最终同一 profile 用于自检与 shell，目录身份与配置在批准、消费及启动前复核。代理关闭需要等待 listener/连接任务，不把取消信号当作清理回执。

本接线批次已写入 runtime、会话适配器、独立范围卡、Direct/Broker 与额外目录策略，以及解析/默认拒绝、队列生命周期、目录替换、持久消费失败和 accepted-policy/bindings 失配的开发回归。固定 1.98.0 格式、差异空白及 9 组中英键/插值参数静态检查通过；编译与运行结果待该批次的云端 CI。实现全部完成后统一非写入者审查。目录预检与 profile 构造不冒充 OS 竞态安全或原生进程清理验证；应用级 reset 与剩余权限 UI 仍未完成。

`df6651c59b58580574dbc126a8d91337432f9400` 的 [Actions 40462010952](https://github.com/puzige/vega/actions/runs/40462010952) 在 Clippy 与 Nextest 编译阶段均报 E0624：Bash 新路径调用的 `TempRoot::validate_identity` 仍为 sandbox 模块私有。尚未取得测试发现或执行计数。修正仅把该现有校验开放到 crate 内，不扩大公开 API、不更改校验或测试断言，待新提交云端验证。

修正 `6aa75c6031b208962ad2e817033628c840d2c608` 的 [Actions 40462627353](https://github.com/puzige/vega/actions/runs/40462627353) 继续发现 type_complexity 与 E0603：新 reader 字段缺类型别名，运行时回归所在的兄弟模块不能访问私有 scoped_bash。提取 reader 类型别名，并把该回归移入 scoped_bash 的开发测试子模块、仅共享 test-only fixture；不放宽生产模块可见性或断言。两轮尚未执行 Rust 测试，均保留首次日志，不盲重跑原 head。

- `vega_conversation::types` 保存独立 `RequestedCommandScope`、`CommandScopeRequest`、`CommandScopeDecision { AllowOnce, Deny, Timeout }` 与内容无关的失败词汇。严格解析拒绝未知/重复字段、空范围、错误类型和空/过长 reason；reason 最多 512 UTF-8 字节且无换行/控制字符。绝对目录文本只做词法验证，不能冒充规范化批准。
- 请求精确绑定 command bytes、有效 timeout、PreparedBash 的工具实例和项目 cwd、run/call/request id、完整 PermissionFence、scope 和 bindings 快照。摘要不替代相等比较。Debug 不输出命令、路径、配置或凭据引用；快照仅含 binding 配置，不含秘密值。
- 只有已持久接纳、仍 streaming 的 run 可创建 live owner；数据库身份、epoch/reset/admission、完整持久 fence 每个状态转换均核对。委派 run 还须核对原始 parent fence/parent message、仍 running 的委派归属及 held release state。活动 run 不从线程的新默认重建 authority。
- 请求须匹配同 run 中已 approved/running 的 Bash call，且普通/危险审计有效。精确 command/timeout 必须等于已持久的安全 input；若隐私投影改变 command，则拒绝扩围而不额外持久化原始秘密。普通审计保持原值，scope 不写 permissions 或替换 ordinary/danger approval。
- binding reader 是可信接线提供的配置快照读取接口；在请求、决定和消费前重新读取，放在 SQLite 写事务外，再精确比较并验证 binding 语法/重复 alias。它不读取秘密或代替执行阶段的严格凭据验证。生产配置来源/严格 secret reader 的接线在后续阶段完成，不能把测试 reader 当本机配置验证证据。

## 状态与消费

- live owner 不可序列化；pending handle/token 只持其 Weak 身份，owner 消失、取消或换 run 均失效。数据库没有恢复 token API。请求/决定/消费均持有已有 permission coordination，并用 Store 的写事务提交独立审计。
- request_id 唯一且 `(run_id,call_id)` 唯一；冲突不覆盖旧记录。pending→approved/denied/timeout/cancelled/failed、approved→consumed/timeout/cancelled/failed 只通过 exact-record CAS；任何存储/提交失败不给 token 或 dispatch receipt。
- pending handle 和批准 token 均不 Clone，决定与消费取走 handle；同一请求只响应一次。AllowOnce 不能由 ordinary Once/Always/Auto、MCP 或 ACP 决策反序列化代替。超时从创建 pending 时的单调时钟截止时间计算，批准不重置期限。
- consume 检查 owner、cancel/deadline、当前 exact call/prepared/scope、bindings、持久 run/call/fence，然后在事务内把 approved 原子变成 consumed。返回的 receipt 不可 Clone/序列化；后续执行接线必须在任何自检/Broker/进程尝试前先消费，并把 receipt 移交单次构造入口，失败不能返还 token。
- identity/cancel/timeout/存储错误消耗内存 handle，不恢复批准；失败审计若也写入失败，仍拒绝启动。drop 不在 UI/render 或析构路径执行 I/O；残留 pending/approved 行不具执行能力，reset 可按既有规则取消。
- consumed 只证明许可已花费，不证明执行或清理。本阶段把 reset 的未决 scope 检查扩为 consumed/unknown；没有可信执行收尾前不得把 consumed 当安全。后续需单独实现可信清理回执，不以 leader/管道结束推导后代安全。

## 验证与后续

开发回归使用隔离 Store、真实 run fence/普通审批/PreparedBash 和合成 binding reader，覆盖严格 codec、一次决定/消费及旧审计不变、重复请求、改变 command/timeout/实例/call/scope/bindings/fence、取消/超时/owner 丢失、重新创建 owner、持久 CAS/存储失败与 consumed reset 阻塞。不执行 shell、Seatbelt、Broker、网络或本机配置访问。

十二项新增开发回归已编写（scope 十一项与 reset 一项），本地 pinned rustfmt/diff/static 已通过；实际编译/行为执行与非写入者增量审查仍待本次冻结 head。没有本地执行或本机配置变更。既有 Actions 执行开发回归，非写入者审查冻结增量。测试最终按整卡规则留痕后清理。下一阶段接通 runtime 输入与队列/UI，再接 Direct/Broker 和目录规范化，保留无监听者 fail-closed、真实 GPUI 导航、Stop/reset/剩余权限 UI 的验收要求。PR 继续 draft，本阶段不是整卡完成。
