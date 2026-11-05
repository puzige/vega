# 写锁边界专项（#891）

## 版本、目标与边界

实施基线 `2aab794f7671da5b970aa18bb70419f85c9cf360`。目标是使一致性读取不预留 writer，使不变状态有安全的读取快路径，消除 DB→YAML 与 YAML→DB 反序，并收窄锁内外部操作/重计算；不调大超时、不吞错、不绕过凭据检查、不重放操作。

原现场仅有 `credential_check/mcp/database/5/5`，不能区分协调器合成 busy 与引擎 busy，具体持锁者未知。静态风险不等于动态复现。

关联 #887 / PR #889 的配置锁重试已停写，head 为 `00b05e1242ca45489e6a0e66a56b3a381825e1b3`，其 CI Nextest 失败。本卡不接手该卡、不复制未验收重试和测试；MCP 修改仅事务快路径，必须与该 head 的接口差异核对，未来整合重新验收。#783 技能内存/持久化协议、#781/#782 drain/安装协议不属本卡重写范围。

## 验收矩阵（先于实现，初始全部待验证）

| ID | 前置/操作 | 可观察结果 | 验证层级与证据 |
|---|---|---|---|
| A1 | 合成 WAL 库，另一连接持 writer；读取 automation timing snapshot | 返回同一已提交快照，不等待 writer、不取得写许可 | Actions 开发并发回归；静态调用链 |
| A2 | 同上；automation refresh 无任务/无到期任务 | 返回一致性快照，无写许可；有到期项释放读快照后进入 writer、重新加载，只转移一次 | Actions 并发/状态回归 |
| A3 | 恢复预检期间另一连接持 writer；合法/过期 checkpoint、权限变化 | 纯读预检不争 writer；仍拒绝 stale/权限不符；实际准入事务最终复核不移除 | Actions + 独立审查 |
| A4 | privacy digest 相同；变化；并发 epoch 更新 | 不变值读取不申请 permit；变化在 writer 内复核，清除旧例外与 epoch 更新原子提交 | Actions + 静态 |
| A5 | 草稿权限保存/已物化/epoch reset 或冲突 | 草稿仅一致性读与内存结果，不持 writer；物化权限仍 CAS；已存在、reset、旧 epoch 拒绝 | Actions + 静态 |
| A6 | reasoning 默认值重试与 MCP 同步并发；会话偏好变化 | 不持 DB writer 等待 YAML；旧请求不能覆盖 superseded 偏好；无死锁 | Actions 合成并发；实际 UI 用户手测 |
| A7 | MCP 配置/投影/pending 未变化；有真实变更/撤销/清理 | 不变状态不争 DB writer；变化路径原子核对，撤销旧 lease，失败 fail-closed，不缓存旧 key | Actions + 与 #887 差异核对 |
| A8 | 显式技能激活、历史脱敏、分叉图片、workspace/owner 准备 | 不将可前置外部读取/解码/目录创建放全库 writer；最终提交仍核对源/权限/身份；stale/cancel/error 无部分提交 | Actions 风险回归；静态锁范围 |
| A9 | 上述 busy 路径与失败 | 最小诊断区分 queue/engine，数据内容不入日志；不重试已完成工具或重复请求 | 静态/Actions |
| A10 | 最终提交 | 独立审查结束；开发测试留痕清理；最终 head required CI；用户手测前不关闭卡 | PR/CI/用户证据 |

## 实现顺序与安全设计

1. 只读预检用 `unchecked_transaction()` 的 deferred 一致性快照；不得在同一读快照升级写入。实际写路径重新开 immediate，并在事务内重新读取条件。
2. `refresh` 的读快照检查是否有到期项；没有则直接返回。有则结束读事务后开启写事务，重读存在性/任务，避免 stale 读后更新。权限草稿分支使用读事务，保留 admission 与全局权限协调。
3. 隐私 digest 的读取命中以该 SELECT 为线性化点；变化仍在写事务内复核，旧例外不可用于新 digest。调用方 keystore revision/fence 检查保留。
4. reasoning 默认值重试与 session_reasoning::initialize/save 共用会话偏好专用协调器。重试在该协调器内读取并核对当前偏好，然后执行原 YAML 字段 CAS；不持有 DB writer。普通偏好写入先取该协调器，再进入短写事务；不存在 writer→偏好协调器反向入口。分叉只复制到新生成的 thread ID，不更新原会话，不属于同一偏好竞争。配置保存失败保留原错误，不能以保存后发现 stale 冒充 CAS。协调器不替代跨进程 YAML CAS；本次不改变原先进程内会话编辑契约。
5. MCP 用配置 edit + DB 一致性读快照核对 digest、pending、完整声明/投影、revision 与 tombstone；只有变化才转入 writer 并重新核对。待发布 YAML 在 writer 之前完成，writer 内比较 pending_base_digest/pending_declarations 与准备时完全一致；变化则拒绝，不覆盖新 pending。发布后 DB 失败仍由原 durable pending 恢复。无清理项和无删除行时，用一致性读快照验证 projection 后返回，不取得全库 permit。真正删除凭据仍保留原 owner/ref/revision 和跨文件删除的协调边界，不能为缩锁删除末端安全证明。配置 edit 不授权恢复撤销的 authority。
6. 外部准备保留 descriptor 身份/摘要/预算安全检查。数据库原子写不是文件冻结手段。源版本/指纹和 epoch 变化时拒绝或重新准备，不在持 writer 时自动重做慢工作。历史脱敏正文/工具偏移/时间戳/summary 必须原子安装。
7. 保留必要准入、终态、权限 CAS、用量归属、checkpoint 安装事务；读事务和同连接重入不是缺陷。

## 专项清单与闭合规则

| 项 | 基线问题 | 状态 |
|---|---|---|
| automation timing/refresh | 纯读/通常空操作 immediate | 已实现 deferred 快路径；真实到期重新开启 writer 并重读；待 CI |
| resume preflight | 纯读取 immediate | 已实现 deferred 预检；实际准入末次校验保留；待 CI |
| privacy epoch | 不变 digest 先 permit | 已实现读取命中；变化仍原子复核；待 CI |
| draft permission | 内存保存先 writer | 已实现一致性读；物化 CAS 保留；待 CI |
| reasoning retry | DB→YAML 反序，锁内文件 I/O | 已实现专用偏好协调与原 YAML CAS；待 CI/用户实际 UI 验证 |
| MCP reconcile/cleanup | 不变状态 writer；锁内文件发布 | 已实现投影/清理快路径与 writer 前发布；真实清理保留 owner/ref/revision 边界；待 CI，与 #887 组合尚未验证 |
| skill preflight | writer 内双读文件/解析预算 | 双读/解析已前置；末次 descriptor/文件状态证明与依赖最终历史的预算保留；准备范围独审/CI 待完成，不扩展 #783 |
| legacy redaction | writer 内重复全历史扫描 | 已实现 deferred 准备具体更新计划、源/summary/offset 和凭据/例外版本复核、savepoint 原子安装；独审/CI 待完成 |
| fork history images | writer 内解码 | 已前置图片解码；最终源、图片行/归属、截图证明仍原子复核；待 CI |
| workspace capture/owner acquire | writer/permit 内准备目录/文件 | 已前置，capture 不创建目录；legacy standalone 四入口补齐；待 CI |
| diagnostics | queue/engine 失败区分 | 复用既有 write_coordination queue/begin/commit 诊断，不新增内容日志；原现场持锁者仍未知 |

每项最终标已修复、必要原子边界（含理由）或真实阻碍/剩余风险；未处置不得宣称专项完成。无法安全前置的末次文件身份/权限证明保留，不机械删除。低频设置幂等空分支仅列观察，不扩展产品行为。

## 开发候选记录

当前候选尚未编译/执行，不标行为验收通过。读取、工作区、配置/偏好范围已有非实现者静态审查；准备阶段独审绑定内容指纹 `e9fc8123303e9107cc0aac4fb34a555139aef375bef3583ea7abc17eef06a2e0`。计划批准回归补齐 Plan 模式前置条件，未放宽成功断言。准备范围新增合成回归覆盖无 writer 准备、源/summary/offset 变化、安装失败回滚、技能源替换、取消与最终预算。

首轮开发 head `f353b3b7d2da71e53659fa2aeaada74fee86de9c`、[PR #893](https://github.com/puzige/vega/pull/893)、[Actions 41994478809](https://github.com/puzige/vega/actions/runs/41994478809) 在编译阶段失败：通知计数错误读取为 `usize`，图片 BLOB 错误缺少到 `ForkError` 的转换；两项按原日志修正，Rust 发现/执行数量未取得。独审发现的权限快照与实际策略未绑定、技能错误误分类已修复草稿：以实际 owner 值生成 epoch，固定同一例外行构造不可变策略，复核文件身份/版本、epoch、例外行与 revision；新增撤销交错、owner/epoch 不匹配、文件替换和技能错误回归。增量独审及新 head CI 待完成。

同步新基线 `a98be7a5e6cac93fe95bf47cb8d450f03108150a`，仅含不重叠工具卡片/文档变化；保护脏改动后 rebase 并恢复成功。本地完整 `cargo fmt --all -- --check` 与 `git diff --check` 已执行通过；新修复仍须当前版本静态检查。本段为开发过程记录，最终当前证据见下节；与 #887 组合兼容仍待取得；实际 reasoning UI 重试、跨进程 pending 竞争及服务清理路径不能由现有静态结论冒充已验证。所有开发 Rust 测试保留至云端证据固定，随后清理并对最终无测试 head 重跑门禁。

## 已固定开发验证与最终候选

- 开发通过 head：`20ce663cecae3fb6c38ff77deda98629145320d0`；[Actions 41999430525](https://github.com/puzige/vega/actions/runs/41999430525)，Nextest job `127150633023`、Clippy `127150633345`、required check `127153091563` 全部 success。
- 实际 Rust **47 发现 / 47 执行 / 47 通过 / 0 失败 / 0 跳过**。保留 Python 三组分别 5、15、40 项，均 OK。日志前段 shell 分支脚本文字里的 `no Rust tests` 不是此次执行结果。
- 可恢复源码：远端标签 [`evidence/891-write-lock-tests-20ce663c`](https://github.com/puzige/vega/tree/evidence/891-write-lock-tests-20ce663c)，已 push 并用 ls-remote 读回完整 SHA；[原始提交 patch](https://github.com/puzige/vega/commit/20ce663cecae3fb6c38ff77deda98629145320d0.patch)。内容 API 读回权限回归 blob `8a7d2315a8521524f667d16d753fc6d761823643`。
- 第三轮曾 40 通过 / 7 失败：六个配置 fixture 的 macOS 临时路径符号链接祖先触发合法围栏，改用 canonicalize 真实根；超量图片原构造先被 schema ordinal 约束拒绝，改为直接调用生产验证函数。生产校验及预算拒绝断言未放宽；预算用例只证明 validator 拒绝，不冒充完整 fork 回滚。第四轮上述修正全通过。
- 独审：读取/工作区、配置/偏好和准备集成已结束；权限绑定、技能拒绝契约、可空查询和夹具增量均无剩余发现。静态结论不替代真实 UI/跨进程验证。
- 最终清理：移除 11 个开发 Rust 测试文件及 11 处独占模块声明，生产代码仅删除测试接线；未变更 manifest/lock/workflow/Python/生产与安全断言。当前 `cargo fmt --all -- --check`、`git diff --check` 通过；Rust test/cfg(test) 属性静态清点 0。最终无测试 head 完整 required CI **待执行**，必须明确记录 `no Rust tests`（0 发现/0 执行），不称测试通过。
- 用户手测仍待：实际 reasoning UI 重试、MCP 同步/清理与 #887 组合；原现场具体持锁者仍未知。Issue 保持开放，用户验收前不 Done。

## 验证政策与非目标

本地仅快速格式和静态检查，不运行 Vega/编译/check/clippy/test。开发回归只交现有 PR Check，采用合成库/配置/文件；无外部真实模型/服务 E2E。本卡不改用户数据库/配置/原会话、不新增依赖/运行测量服务、不删除锁、不放宽断言。Rust 开发测试在验证后固定可恢复 SHA/patch 再移除，最终 CI 明确 0 发现/0 执行。UI 实际集成由用户手测。
