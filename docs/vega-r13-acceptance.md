# R13 — 侧栏组织主 Agent 验收

2026-09-06。ZCode 侧栏组织功能已在本地集成，最终应用代码 `60bd44a`，分支 `codex/vega-review-integration`。新版签名副本为外部验收目录 `native-r13/Vega.app`，当前单实例显示1280×750浅色三栏与真实README预览。

**最终统一检查：964 passed / 0 failed / 0 ignored。fmt、strict workspace all-target clippy、workspace build均通过。** 本轮原生清单12组通过。尚未push、合并master、发布或替换安装版；不是103项ZCode功能全部完成或整套像素一致。

## 范围与执行

- [R13规格](vega-r13-sidebar-organization.md)、[数据交付](vega-r13-data-delivery.md)、[UI交付](vega-r13-ui-delivery.md)为实现及逐次失败记录。两名Astra medium分别实现数据与UI，主Agent负责参考端点验、契约审查、集成和真实客户端验收。
- 已实现分组/项目切换、按项目/时间线、更新时间/创建时间排序、每项目初始5任务及每次增加5行、全部/单项折叠、项目手动排序。
- 自定义组支持跨项目成员、命名、七种主题颜色、组顺序和组内任务顺序；支持拖入、拖出及菜单替代。组内新建任务与关联原子提交；解散保留任务，归档恢复保留成员关系。
- 四张新增组织表由0004迁移建立。组织查询/写入走后台单一请求通道、一致快照、revision校验；精确创建结果与导航/编辑器所有权避免晚到确认覆盖后续操作。R12任务菜单、真实访问、草稿/导航及实时分支保留。
- 未改供应商、凭据、Keychain、运行时或Git runner；没有新增依赖，`cargo tree -p vega_runtime --edges normal --prefix none --locked`确认无UI依赖。依赖清单及lockfile与R12基线相同。

## Freeze与统一门禁

- verified_at_utc: 2026-09-06T07:01:04Z（最终签名）；原生复验及进程读回在此之后完成，时间保留在外部JSON。
- git_head: `60bd44a8de1df0effbe8222caf0d5c47c06fc09a`；tree: `b158574f47512f8a7e35be3bb8134468e9f5c8c3`。
- tracked_diff_sha256: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`（测试时干净）。本报告及状态台账是随后文档提交，不改变已验收产品代码。
- macOS15.7.9 / arm64；rustc1.98.0、cargo1.98.0、Git2.55.0。统一检查时所有Vega实例已退出，executor已停止测试。

| 精确命令 | 最终结果 | 耗时 | 原始日志 / SHA256 |
|---|---|---|---|
| `cargo fmt --all -- --check` | PASS | 1.016s | `/private/tmp/vega-r13-final-2-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS | 2.446s | `/private/tmp/vega-r13-final-2-clippy.log` / `d91b1a4107eaa36e75a1f4471e69c814b2a9f42a228737ef821436ad93a20174` |
| `cargo test --workspace --locked --no-fail-fast` | 964 passed / 0 failed / 0 ignored | 92.726s | `/private/tmp/vega-r13-final-2-workspace-tests.log` / `a703a206f632212ac71cd1fa1b3f6a93d7065de62d5ac502d054c283f3d58e2d` |
| `cargo build --workspace --locked` | PASS | 3.284s | `/private/tmp/vega-r13-final-2-build.log` / `d135cc68f7aa801e417f73e91d2b7d590993a49ef27b51e25e6053443568250b` |

最终之前，主Agent独立数据复验为Store93/0/0、组织service11/0/0、S7 E2E1/0/0；UI executor最终侧栏20/0/0。service本地日历边界在UTC及America/New_York单独进程检查，覆盖23/25小时的昨天。具体命令、首次失败与hash见交付报告及外部`root-data-verification.json`，不把这些程序化结果代替原生操作。

## 真实客户端证据

所有项目和任务均由实际文件夹选择器、Cmd+N、任务/分组菜单及鼠标键盘建立；没有向真实数据库注入fixture。创建两个自有Git项目、8任务（Alpha6、Beta2），0消息。每次换构建先Cmd+Q并核对0实例；最终读回仅一个进程且二进制摘要与签名副本一致。

| 验收组 | 实际结果 |
|---|---|
| 项目分页 | Alpha初始显示5行，点击显示更多后第6行实际出现；两个项目归属正确。 |
| 视图与排序 | 项目/分组、按项目/时间线、更新/创建均从原生入口切换；时间线显示任务原项目，Beta草稿与非当前Alpha6未读保持。 |
| 分组编辑 | 新建、Escape取消不落库、菜单重命名和Enter保存；七色菜单可见并实际应用蓝色；组可独立展开/折叠。 |
| 任务菜单移动 | 在Beta当前草稿中把非当前Alpha6标为未读并移入Focus；未读和真实项目保留，没有误打开菜单背后的任务。 |
| 拖放 | 任务拖入空组、跨组、组内排序、组排序、项目排序均实际执行并读DB；修复后拖至未分组标题和已有自有任务行均成功解除成员关系，不改当前草稿、任务/项目身份或未读。 |
| 组内新建/解散 | 在选中Beta项目时从Later菜单新建，任务确实属于Beta和Later；随后解散Later，任务仍存在并出现在未分组。另建一个Later供持久化验收。 |
| 归档恢复 | Alpha6归档时DB仍保留Focus成员和unread1；通过统一归档入口恢复后回原组。 |
| 导航草稿 | 真实VegaWindow输入Alpha/Beta不同未发送草稿；实际Back/Forward一组往返分别恢复，非独立TextInput fixture推断。 |
| 折叠 | 时间线/分组/项目分别收起全部，再单独展开；当前任务和输入保持。 |
| 重启 | 重启后四张组织表逐行与退出前全等，包括视图、创建时间排序、名称、颜色、组/任务/项目顺序及折叠状态。 |
| 三栏与菜单 | 960×600、1280×750浅/深色及真实README右栏可用。最终版筛选/分组菜单分别实际Escape关闭，Beta草稿和Alpha6未读保持。 |
| 数据完整性 | schema3→4；原有11任务全字段未变化；原有14消息、24工具记录、15用量记录与迁移前SQLite备份逐行完全一致。 |

主要流程初验于`8c7137a`；拖出与重启复验于`5549463`；最终`60bd44a`完成应用启动、筛选/分组Escape及三栏复验。后续改动均按窄差异复核，没有声称每次重启重做全部流程。原始图像保留在当前任务CUA工具输出；外部`native-checklist.json`、`native-mid-state.json`、`persist-{before,after}-restart.json`、`drag-out-{title,row}.json`、`final-data-integrity.json`和`final-instance.json`保留实际状态。

签名二进制SHA256：`5d8ede8ee76516b19d24fa18e1b22984b5819911ca8df4b6970dc62c2aebfae0`；`codesign --verify --deep --strict`通过。配置仅复制自有config/reasoning文件，无凭据复制，未访问Keychain或发送真实供应商请求。

## 首次失败与修复

- 审查复现组内创建晚到确认替换设置背后任务：把设置页面状态纳入导航所有权；编辑器确认同时核对实体、组身份和提交文本。原19/1失败及后续20/0保留。
- 原生发现未分组标题在落点容器外、未分组行消费拖放payload：统一标题/区域落点，仅组内行安装排序drop监听。先扩展原真实指针测试复现，再原生复验标题和已有行两种落点。
- 原生发现组织菜单Escape被应用全局CloseSettings抢先消费：补局部action处理，并让既有mounted fixture加载真实应用级快捷键以先复现；最终原生筛选/分组菜单均关闭。没有用移除全局快捷键掩盖问题。
- 首次统一workspace为**962 passed / 2 failed / 0 ignored**，108.770s。`stream_persistence`和`todo_e2e`两处旧完整表清单仍为六表；经规格明确授权，只增加四张组织表，完整集合全等和消息/工具/费用断言全部保留。原始日志`/private/tmp/vega-r13-final-workspace-tests.log`，SHA256`43c94bbfdaa5c4985e736a673e52b46de47b756c6bfa87f7eaf4e9bae09f222d`，没有被最终日志覆盖。
- 编译、lint、最初拖放及同毫秒fixture排序失败继续保留在各executor交付文档，不只保留最终绿色结果。验收追加窄修复提交经主Agent授权，未扩展产品范围。

## 边界与下一轮

- R12的Git stdout-overflow/TimedOut偶发失败本轮没有复现；Git runner未改，不能据本次全绿宣称根因已修复。
- 时间线过去日期/DST由service测试验证，原生没有伪造历史日期；七色入口均展示，原生实际选蓝色，未声称逐色点击。菜单跨异步竞争由真实worker/handler回归补证，不冒充用户速度的原生复现。
- 草稿/导航历史是窗口内状态，重启持久化证据只针对组织数据；没有声称草稿跨重启恢复。组织最多10000任务/128组，超限显式报错；分页更多行数可窗口内保存。
- N09–N11完成本卡对应能力，N15补项目/组/任务拖放及更多；ZCode独立顶层分区拖动和无项目任务不等同于Vega现有模型。其余附件、编辑分叉、MCP/技能/插件、自动化、辅助对话、浏览器与远程仍在后续矩阵。
- 下一轮优先M02–M03模型发现/连接检查及外观、通知设置；继续主Agent把关、Astra medium实现、真实客户端验收。真实供应商请求和性能bench/soak本轮NOT RUN，性能按用户裁决延期。集中使用统计与移除对话成本指标保持。
