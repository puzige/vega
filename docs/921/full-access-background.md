# Full Access 后台并发合同

用户授权：Full Access 后台并发、不独占、正常无感、不新增 UI 体系。基线 master `0b3dcd36`。本合同对 Full Access 明确取代 #412 的禁止异步及全局写范围互斥；旧沙箱记录保持原合同。

## 验收（实现前）

| 前置与操作 | 可观察结果 | 验证 |
|---|---|---|
| 沙箱 owner 或 blocker 存在，启动已授权 Full Access（阻塞/异步） | 不因沙箱门禁拒绝；不清除沙箱 blocker | runtime 行为回归 |
| Full Access 异步在途，再开同/异 run 命令或 write/edit | 不登记范围预留，不因该任务拒绝写入 | runtime 行为回归 |
| 沙箱异步在途，再开受协调的重叠写入 | 仍拒绝；两种到达顺序一致 | runtime 行为回归 |
| Full Access 取消收尾未知 | 保留真实退出与未知清理事实，不设置 workspace blocker，不取消兄弟任务 | runtime/store 回归 |
| child/reader/租约仍占用 | 继续占应用资源槽；槽满只拒绝新异步，wait/cancel/edit 可用 | runtime 回归 |
| Stop/run 结束/退出 | 关闭该 run 新准入，同时取消其全部任务，在共同有界清理内收束；其他 run 不受影响 | runtime 回归、系统用户手测 |
| 重启未完成 Full Access | 标控制失效与 handle 过期，不重跑、不按旧 PID 发信号、不生成写 blocker | store 回归 |
| 终态提交回执丢失 | 原 operation/完整内容精确读回，不清别的 blocker | store/conversation 回归 |
| UI | 复用原卡结果/错误及取消；无新面板/弹窗/状态栏 | 静态独审、用户手测 |

## 规则

- 启动策略固定为 workspace coordinated / full_access concurrent，不随之后权限切换改变。无沙箱不增加授权；权限、危险命令和资源身份核验仍在启动前。
- Full Access（含旧阻塞）不参加写范围互斥。沙箱任务只与其他受协调操作互斥；不能声称能阻止 Full Access 写入同目录。Read-before-write 与内容身份检查继续，Full Access 开始及完成时失效全范围旧读取依据；这不是锁。
- 应用4活动槽、64保留记录；每run最多4保留会话。Full Access允许多会话；同run沙箱活动仍限制受协调操作，明确豁免Full Access。不淘汰活任务、不排队、不静默改阻塞。
- 实际命令结果、清理事实、写入门禁分别保存。Full Access未知只影响任务：不添加内存/持久写 blocker；不接受风险解锁。真实存储/权限等独立故障仍走原失败规则。
- 单命令取消只影响原 operation。Stop关闭本run并并行启动全部取消，避免逐任务叠加五秒。自然run结束仍收尾，不新增跨run续跑。
- 保留五秒共同预算、身份核验、不复获旧PID权威、有界输出及缺口；不能保证所有后代退出或撤回副作用。
- 可信owner存在时仅重试其资源清理；owner丢失仅观察历史资源，不重新绑定进程、不续跑。任务局部恢复不能清除沙箱blocker。历史未知本身不占槽，真实未收束资源占槽。
- UI不新增体系；Full Access不进入工作区解锁面板，运行/错误更新原命令卡。已有卡内取消/错误路径按真实能力适配，不制造已终止假象。

## 实现顺序与范围

1. store新增递增迁移，持久化不可变调度类别；旧记录默认coordinated；分类终态和重启恢复。
2. runtime分离资源预留与范围预留，双向准入豁免、任务局部未知、并行Stop；tools开放已授权Full Access及全范围read失效。
3. conversation贯穿类别/精确读回、分类恢复；既有卡片适配，不增加产品控件体系。
4. 按风险本地测试与Clippy，独审、开发源码留存、清理Rust测试、最终head required CI，合并后用户手测。

不改MCP/桌面协议、不安装服务或依赖、不引入OS隔离、全局锁或变相排队；不推进原PR #917。
