# R14 — 模型设置、文件夹与当前分支主 Agent 验收

2026-09-06。本地集成应用代码 `0d7056a`，分支 `codex/vega-review-integration`。三个 Astra medium 专属 executor 完成实现，主 Agent 审查、集成并通过实际 macOS 窗口验收。最新签名副本为 `/Users/puzige/Workspace/vega-review-20260905/native-r14/Vega.app`。

**最终 workspace：988 passed / 0 failed / 0 ignored；fmt、strict all-target clippy、build 均通过。** 打开文件夹自动显露原项目组，输入区自动读取实际当前分支；侧栏收起后保留可见恢复按钮。当前单实例为1280×750浅色工作区，已打开自有 `r14-folder-two` 任务，显示 `main`。

## 范围与实现

- [模型设置规格](vega-r14-provider-management.md)：供应商列表与详情、启停、拖动/按钮排序、真实模型发现及显式追加导入、逐模型连接检查。沿用现有 Chat Completions 格式、表单、模型默认值、思考和定价/集中统计；不恢复对话成本指标。
- [侧栏与文件夹规格](vega-r14-sidebar-folder-fix.md)：顶部统一按钮与 Cmd+B，鼠标、Space/Return 和焦点保持；后台 canonical 路径注册/复用，原子显露项目，自动切回项目视图、展开、滚动定位，两个文件夹中新建任务各归所属项目。重开不重复创建，取消保留导航与草稿。
- [当前分支规格](vega-r14-current-branch.md)：输入区独立读取已注册目录的实际 HEAD，无需打开分支菜单；侧栏隐藏仍刷新，目录/任务变化拒绝旧结果。点击分支名称才进入原有可选切换流程；读取不执行 checkout、不假定 main/master。非 Git 隐藏入口，detached、未诞生与失败状态如实展示。
- 网络检查经过生产 service、R10 本地凭据、实际 reqwest/SSE 路径，固定 `Reply with OK.`、128 tokens、无工具/聊天内容，5秒连接/15秒总时限、1 MiB、无自动重试、禁止重定向。有效完成的文本或 reasoning 才算连通；结果不保存输出内容。发现只返回候选，明确导入才修改配置。
- Provider、默认值/主题、侧栏共享一次读改写锁；保存复验精确字段基线，保留不属于本次操作的字段。异步回调校验目标和 generation，配置变化、取消及离开页面清除旧网络结果。已提交的短原子保存可以完成，不能由晚到回调改掉新的页面或草稿。
- 数据、UI、侧栏各自逐次证据见 [D](vega-r14-data-delivery.md)、[U](vega-r14-ui-delivery.md)、[S/B](vega-r14-sidebar-delivery.md)。没有新数据库迁移或第三方版本；UI 将已有 workspace tokio/tokio-util 用于后台操作。runtime 依赖树无 UI crate。

## 最终代码与门禁

- Head：`0d7056a6ccb553a5ea1c751b528354f8810ff86c`；tree：`d055cdbe1c06a2f43bc49999576ddab52a5d12ae`。
- 检查启动：2026-09-06T08:04:07Z，检查时 tracked diff 为空。此报告及台账属于后续纯文档提交，不改变被测应用代码。
- 二进制 SHA256：`8ef5b1b9a9cfd76ea82c8121e2c4465b55bf21d7bc4c57544bd99ded8327bf43`；ad-hoc 签名及 `codesign --verify --deep --strict` 通过。
- macOS arm64；rustc/cargo 1.98.0、Git 2.55.0。已有依赖 `block v0.1.6` 的 future-incompatibility 提示保留，不是本轮 gate 失败。

| 精确命令 | 最终结果 | 耗时 | 原始日志 / SHA256 |
|---|---|---|---|
| `cargo fmt --all -- --check` | PASS | 1.057s | `/private/tmp/vega-r14-final-2-fmt.log` / `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS | 2.917s | `/private/tmp/vega-r14-final-2-clippy.log` / `fb651de344eeaa7ec664b73d78f43c9fa707376e79acfd55407a4bafc58de6ad` |
| `cargo test --workspace --locked --no-fail-fast` | 988 passed / 0 failed / 0 ignored | 98.419s | `/private/tmp/vega-r14-final-2-workspace-tests.log` / `0b0a91dc2491260adddf907fb565f61ce8256671a0d4625b7136de4d2f7a98a7` |
| `cargo build --workspace --locked` | PASS | 7.228s | `/private/tmp/vega-r14-final-2-build.log` / `52a62e1c163538bee758f6051aaa76fba8ac61150a61ce19a9f4b2cd6f85f159` |

## 原生验收

主 Agent 使用真实 CUA 鼠标、键盘、原生文件夹选择器操作。每次切换构建先退出全部旧 Vega，核对0实例再启动新副本。测试模型指向自有 `127.0.0.1` HTTP 服务；合成凭据通过原生表单输入，没有复制真实凭据、访问 Keychain 或发送收费模型请求。

| 流程 | 实际结果 |
|---|---|
| 侧栏恢复 | 960×600顶部按钮在折叠时可见；鼠标展开后 Space/Return 可连续收起/展开。输入区 Cmd+B 保留原草稿与焦点。窄/宽及浅/深色检查完成。 |
| 文件夹分组 | 从原自定义分组视图经真实 picker 打开两个自有文件夹，自动进入项目视图并展开/定位；分别新建一个任务，真实 project_id 各自正确。最后从设置重开已有 Two，回到原文件夹组，没有重复项目或任务。 |
| 导航取消 | 两任务写不同未发送草稿，实际后退恢复对应草稿；取消 picker 不改变原选择或草稿。窗口内草稿不冒充跨重启持久化。 |
| 当前分支 | 不打开分支菜单，进入 One/Two 任务直接显示实际 `main`。隐藏侧栏后从外部切换自有 One 到 `codex/r14-head`，输入区自动跟随且草稿完整；随后已恢复 `main`。重启后 Two 再次直接显示 `main`。 |
| 供应商启停与排序 | 拖动 Alpha 到末位；禁用后新的可用模型列表仅剩其他供应商，默认值不被静默改写。重新启用，最后通过上移按钮及 Space 把 Alpha 排回首位。原生重启保持 Alpha/Beta/Slow。 |
| 凭据与保存 | 缺凭据测试先显示明确错误且不发 HTTP；原生保存合成 key 后显示已存储。再次测试得到401后编辑保存，旧错误清除，不把保存冒充连接成功。credential 目录0700、文件0600，合成值未进入 config.toml。 |
| 模型发现与导入 | 实际 GET `/v1/models` 得到3个候选；未确认时磁盘仍只有旧模型。勾选新候选后明确添加，磁盘保留旧模型并追加1个。重启保留2个模型。 |
| 模型连接 | 实际 POST SSE 完成显示成功；拒绝端点返回401，UI显示有限错误。慢端点实际15秒超时；另一次同次 CUA 操作立即点击停止，按钮恢复且旧“正在连接”提示清除，后续页面无晚到结果污染。 |
| 设置布局 | 960×600浅/深色完整显示 Base URL，较长内容正常滚动；1280×750浅/深色可见两模型行。成功保存和取消均不残留旧状态。 |
| 重启与原数据 | 最终退出前/重启后 config SHA、ui/defaults/provider顺序与模型、四张组织表完全相等。原7项目、19任务全字段及14消息、24工具、15用量、权限、组和成员记录均与前置SQLite备份相等。仅通过UI新增2自有项目/2任务，0消息。 |

主要文件夹初验为 `af61f03`；键盘与模型链路初验为 `1d17b90`；最终 `0d7056a` 复验当前分支、保存/停止提示、960布局、排序、已有目录去重与重启。没有声称每次构建均重复全部操作。原生截图在当前任务 CUA 输出中，未虚构本地截图文件。

外部证据根：`/Users/puzige/Workspace/vega-review-20260905/native-r14`。`native-initial.json`、`native-models.json`、`native-final.json` 记录分阶段观察；`discovery-before-import.json`、`http-evidence.jsonl` 记录真实网络/磁盘证据；`persist-{before,after}-restart.json`、`final-data-integrity.json`、`final-instance.json`、`provenance.json`、`final-gates.json` 记录最终状态。HTTP 日志只有方法、路径、有限字段和合成鉴权是否匹配，不保存密钥、返回内容或用户草稿。历史 `provider-before-final-restart.json` 的空 `credential_files` 是旧非递归扫描限制，凭据是否存在及权限以最终递归元数据为准。

## 首次失败与修复

- 首次统一 workspace 为 **982 passed / 1 failed / 0 ignored**：`production_root_palette_escape_preserves_composer_and_settings_action` 仍用临时 `/var` 原路径查库，而生产注册已经按本轮规格保存 canonical `/private/var`。只把测试查找路径改为 `canonicalize()`；身份、数量、取消、草稿断言保留。原日志 `/private/tmp/vega-r14-final-workspace-tests.log`、SHA256 `fcec08094b2dcf3c489e58f7568dbed2ff15e86bcd008494516ee56bd97f7986` 与 `first-gates.json` 保留。这不是生产注册失败或已证明的 Git 时序问题。
- 原生发现鼠标展开后 Space 不工作，实际根窗口回归先复现后修复持久焦点句柄；最终原生连续鼠标/键盘通过。
- 原生发现保存凭据后旧错误还在、取消后底部仍显示“正在连接”；成功配置变更与网络失效统一清理旧结果。最终真实401→编辑保存、慢请求→立即停止均复验通过。
- 原生960窗口导入第二模型后 URL 值消失；渲染测试重现高度0px，改为独立滚动视口与不收缩内容流，恢复完整20px文字行。首次布局断言的小数高度问题按像素取整修正，未改字体或放松可见性要求。
- 用户指出无需选择分支后，源码确认输入区只在打开 selector 后初始化 current label；改为独立只读 HEAD 投影。程序化实际根窗口覆盖 linked worktree、detached、非Git和晚到owner；原生覆盖普通仓库、隐藏侧栏外部变化与返回任务。
- 其他首次编译、fixture 非阻塞 socket、旧表单未挂载与 lint 失败保留在 executor 报告。没有以只保留绿灯或延长产品时限掩盖问题。

## 边界与下一轮

- 原生网络证据是生产入口到实际 loopback HTTP，尚未验证真实收费供应商。5秒 TCP connect stall 未单独原生制造；超大/畸形/无DONE/重定向/竞态等由生产 service 实际 transport 回归证明，不等同于逐项原生操作。
- 配置锁是进程内写入协调，任意外部进程不受同一锁保护；磁盘基线复验和原子替换不等于跨进程 CAS。凭据和配置是两个原子文件，补偿写失败仍可能需要用户修复。
- R12 Git stdout-overflow/TimedOut 偶发失败本轮未复现，runner未变，不能宣布根因修复。性能 bench/soak 按用户要求继续延期。
- M02完成本轮范围；M03连接检查完成，视觉/上下文能力编辑与真正能力验证仍缺少。下一轮优先外观字号/代码显示与通知设置，其余按103项矩阵推进。不是103项全部完成，也未宣称整套像素一致。
- 当前 bundle 使用自有测试配置；验收结束停止自有 loopback 服务，测试模型不会被描述为真实可用供应商。没有 push、合并 master、发布或替换安装版。
