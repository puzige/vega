# 独立失败诊断的可用性与缺记录解释

## 范围与原始证据

用户要求先给方案和验收，再补齐 MCP 配置访问细分诊断、排查 busy 没有独立持锁记录。基线 `852d4ebf14983d83e03b16b02e5cf099f60a1e92`。本轮只写 `feat/891-diagnostic-availability`，不修改用户数据库、配置、安装或重试会话。

- 新现场：2026-11-06 14:39:40（UTC+8），run `01M6QS948YP0G68GKXJSAMET5X`，`request_preparation/io/mcp/configuration/other`；运行 0.1.401，进程 14:04:38 启动。确认新版 MCP 配置访问失败复发，不是 SQLite busy；无法从 other 判断锁中毒或文件锁冲突。
- 最新 event_pipeline/database/5/5 仍为 11:28:53，run `01M6QC3MGSWMB28H2TJF9020G4`。当前数据库旁没有 `persistence-failures` 目录。当前文件/父目录 owner 与模式未见不符，但现在的元数据不证明历史状态。
- sidecar 从 0.1.398 起提供；不能用当前安装 0.1.401 证明 11:28 的运行进程已加载它。没有该时刻进程构建身份或保存错误原文，具体未记录原因及持锁者保持未知。
- 确认的诊断缺陷：保存失败只发 generic warning；read_by_run 把 NotFound 当空数组，export 仅以 Result::is_err 判不可用。目录缺失与正常空集混同，保存原因丢失，且没有进程内回执可导出。
- 已确认 MCP 同范围由 #903 / PR #904（head `796b9000e5164fca3c56fe4d3ad143adc98edcd2`）单独处理，已 READY 停写、独审/云端阶段，目前 Nextest 失败未交付；#887/#889 独立负责锁忙预算。本会话不覆盖或重复实现。新现场应关联到 #903，不能称已修复；该 PR 的阶段分类不依赖本轮 sidecar 变更。

## 验收（先于实现计划）

| ID | 前置/动作 | 必要结果与证据 |
|---|---|---|
| A1 | 合成真实协调队列超时及 SQLite writer 冲突 | 持久化原观测；queue/engine 不混同，不虚构外部 owner；Actions 合成回归（复用原观测协议） |
| A2 | sidecar 目录/锁/slot/写入/发布失败 | 导出固定失败步骤、稳定 I/O kind 和可选系统错误码，不记录路径、SQL、任意原始错误文本或用户内容；原业务错误不变 |
| A3 | 没有诊断目录、目录存在但无该 run、读取被拒绝 | 明确区分 missing / readable / failed；missing 不宣称从未有失败。保留旧字段兼容，升级 export schema |
| A4 | sidecar 保存失败，当前进程导出，再重开 Store | 可见有界的 save-failure 回执，按数据库身份+thread/run 隔离；回执标明仅当前进程，不能冒充持久化记录。重启丢失边界明确 |
| A5 | 成功事件、成功保存、超过保留容量、unsafe symlink/hardlink/模式 | 成功事件无新增文件 I/O；64 slot/1 pending/1 lock 规则不变；进程回执最多64条；既有安全拒绝不放宽，无备用文件或业务重试 |
| A6 | 诊断自身失败后业务返回 | 仍返回同一业务错误，不重放事件、工具或模型请求，不延长 writer 超时；真实 actor 故障回归 |
| A7 | 最终交付 | 云端行为回归+独立非实现者审查；测试源码远端留痕后清理；最终 head 无 Rust 测试且 required CI 通过。Issue OPEN / In review 待用户验收 |

## 最小实现计划

1. 在 persistence_failures 内增加内容无关失败步骤与 I/O 分类，以受限类型包裹底层错误码（不保留原始 error 对象/文本）。各文件操作在实际失败边界添加步骤，不改变文件协议、锁顺序或安全检查。
2. 在原 actor 保存入口记录保存失败的有界进程回执（保存失败步骤、时间、category 与身份）；使用 Store 已固定数据库身份隔离，不引入第二套不受保护磁盘回退。warning 只输出同一类型化字段，不打印错误文本。
3. 导出保留 persistence_failures 和 legacy unavailable，同时增加读取状态/细分失败及当前进程的保存失败回执，schema 升级；目录缺失明确不可用，正常空集仍可读。进程回执可在主数据库事件为空时构成诊断结果；导出不写盘。
4. 开发回归使用合成私有目录和独立 DB，验证 typed fault、重开/隔离/容量、真实 queue/engine snapshot、actor 原错误及脱敏。不开新依赖或工作流，不本地编译/运行/测试。

## 非目标与安全边界

不从缺记录反推历史持锁者；不声称修复 database busy，不修改 final source 锁内策略；不放宽文件安全、重置/清除锁、修改权限或用户配置。MCP 可操作恢复/细分诊断沿用 #903，不从 other 推断 poison。所有历史缺记录原因仍未知；进程未加载诊断、入口未覆盖或 sidecar 自身失败是待区分假设，不是已证实事实。

## 开发验证与合并前清理

- 开发版本：`c88650fcbc947aa03963a0284e12cae4a609f887`；可恢复源码：[远端证据 tag](https://github.com/puzige/vega/tree/evidence/891-diagnostic-availability-tests-c88650fc)，已读回指向该完整 SHA。Store / conversation 测试 blob 分别为 `7deff073893008d43adea3529958d00570f2abda` / `d266abd14cbee20119a68f93f23d1946b69c07a0`。
- [Actions run 42179566500](https://github.com/puzige/vega/actions/runs/42179566500)：Rust 8 发现 / 8 执行 / 8 通过 / 0 跳过；Python 三组 5 / 15 / 40 通过；fmt、全目标 Clippy、required check 成功。未本地运行 Vega、编译或测试。
- 非实现者独立只读审查对上述精确 head 给出 PASS，无阻断发现或必要回归缺口；前两次中断不计通过。主要核对文件安全、三态导出、进程回执隔离与容量、actor 原错误及锁退栈边界。
- 证据限制：写入/发布/目录同步等部分步骤为 fault 转换回归，不冒充实际文件操作失败注入；同进程重开 Store 不等于进程重启；成功事件未创建 sidecar 加静态控制流核对，不冒充 syscall 级零 I/O 观测。历史缺记录原因与具体持锁者仍未知。
- 清理仅删除两份本卡开发测试（Store 300 行、conversation 155 行、共 8 入口）及两处各 4 行的独占模块接线；没有测试专用依赖、feature 或 fixture 需要另清。生产行为、文件协议、Python 测试和工作流保持不变。
- 清理版本仍须独立 delta 审查及完整最新 head required CI；最终 Rust 0 发现 / 0 执行记为 `no Rust tests`，不是 Rust 测试通过。合并后 Issue 保持 OPEN / In review，等待用户手测；合并不等于安装或验收。
