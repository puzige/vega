# #644 实施与验证记录

## 当前阶段

原始功能已通过 [PR #874](https://github.com/puzige/vega/pull/874) 合并，合并提交 `28859f6f00b674141c6222172c8c1f1e3c18588a`。追加修复 [PR #875](https://github.com/puzige/vega/pull/875) 已合并为 `3cf6573b9fdbcbf59de9d5ba3b46defab836401f`，最终 [Actions 41678421619](https://github.com/puzige/vega/actions/runs/41678421619) required CI 成功：60 条 Python 通过、Rust `no Rust tests`（0 发现 / 0 执行）；首次 Clippy 因 hosted macOS 容量不足未分配运行器而取消，失败任务重跑通过。当前进程运行 `/Applications/Vega.app` 0.1.384，release ref 指向该合并提交。用户第二轮手测未通过，继续修复，Issue 保持 OPEN / In progress。

## 第二轮反馈：有历史却禁止手动压缩

- 截图提示由未配置改为 `No history to compact`，裸 `$` 仍无候选。不能再按未更新或上一轮回归已通过推断本轮成功。
- 确定源码缺陷：容量投影要求最后用户消息位于 index > 0；默认空闲手动 worker 又未初始进入响应组规划且 owner 为 None，因此单用户多完整工具响应组也被拒绝。修复入口和执行器共用 checkpoint-aware 响应组规划，保持 NoContinuation/来源围栏/配对/缩小/安装门禁。
- 独立审查发现初稿会在常规刷新中逐组深复制整份来源；范围投影改为先筛选再复制所需文本片段、工具和图片，增量静态审查确认原 P2 已解决，校验与字段语义不变。仍有元数据扫描，不宣称所有规划复杂度为线性。
- 只读元数据：当前目录根身份匹配且启用，逐项批准表为空；15 个子目录项为符号链接，发现器按既有安全合同拒绝；剩余 1 个普通子目录没有批准。未读取私有消息/工具输出或凭据，未写用户配置/数据、未自动授权、未放宽链接边界。最新会话元数据已是 4 条已完成用户消息且无 checkpoint，不等同于原截图时刻，不能据此声称复现该 UI 快照。
- 本轮开发回归正在补充；仅本地格式/差异静态检查，尚未执行云端行为回归，未合并、未安装。

## 追加反馈：空查询技能与已配置容量

- 裸 `$` 展示当前可用技能；裸 `/` 保留默认命令并追加技能，后续输入过滤。未扩展授权、Codex 或输入排除边界。
- 已配置容量误报的确定根因：失败诊断查询仅返回 36 列，共用解码器需要 39 列；补齐协议/凭据分类尾列。菜单区分加载中、读取失败和真正未知容量，不修改用户设置或数据。
- 开发 head：`df6a0a4776b0b45fc38bd681ad1bba61039b2cf5`；基线：`280c2822c01d922f88a7327d8067c1dfa7156c86`。
- [Actions 41675962031](https://github.com/puzige/vega/actions/runs/41675962031)：11 发现 / 11 执行 / 11 通过 / 0 失败 / 0 跳过；其中 6 条技能查询 GPUI、3 条真实 Store 容量投影、2 条菜单状态/错误恢复 GPUI。Clippy、60 条 Python 测试和 required gate 通过。容量断言包括输入 300000、输出预留 128000、总容量 428000 及失败归属/分类。
- 独立静态审查：空查询 `280c2822..4efb7fb0`、容量 `4efb7fb0..df6a0a47` 均无可操作发现。没有本地运行、编译、构建或测试。
- 源码已固定至标签 [`evidence/644-followup-development-df6a0a47`](https://github.com/puzige/vega/tree/evidence/644-followup-development-df6a0a47) 和 [PR 留证评论](https://github.com/puzige/vega/pull/875#issuecomment-6609996241)。基线至开发 head 的 `git diff --binary` SHA-256：`12d199aa728fe4dd89c18c413f9a3a9c6ed9ab9570430af061ee4e35be8f00c5`。

| 临时测试源码 | SHA-256 |
|---|---|
| `crates/vega_ui/src/conversation_stream/composer_actions/empty_query_tests.rs` | `98868431fd8d5bc32756a84be9067385b7beae506454b5e5191edbacf44375d1` |
| `crates/vega_conversation/src/capacity_projection_tests.rs` | `6910c3e0a0394793be6dc0da7587aec9db2bb1e8fdfd0b60cb4609e5b4be37b5` |

成功留证后删除上述临时源码、独占模块接线、GPUI test-support 和独占锁依赖，生产行为不变，锁文件恢复为基线生产依赖图。本地 `cargo fmt --all -- --check`、`git diff --check` 通过，静态清点无临时测试属性/模块及 test-support 残留。清理后的独立审查及最终完整 required CI 尚待完成；Rust 空集必须记为 `no Rust tests`（0 发现 / 0 执行），不能替代上述开发回归结果。实际桌面交互及真实模型压缩仍待用户手测。

实施规格：[composer-entry-implementation.md](composer-entry-implementation.md)。用户追加裁决：立即引导允许在安全消费边界新增已校验技能；已激活技能的冻结快照不可替换；失败保留消息与绑定并明确提示。

## 开发版本

首个开发提交：`66b0e48d`；同步 master 后的开发 head：`e45838d27149d12c8cd88c5aee4b677c0cc1e635`。`git range-diff` 确认任务补丁内容等价；整合带入最新凭据错误分类与文本显示/选择更新。基线整合不等于新 head 行为验证通过。

开发版本共 41 条回归：17 个编辑器 GPUI 回归，覆盖精确范围绑定、Unicode、重复提及、撤销/重做、复制粘贴隔离、IME、查询反例和整份草稿重置；24 个后端回归，覆盖身份/范围、冻结字节、整批失败、预算和授权拒绝、确认失败保留队列，以及原子持久化拒绝/回滚和运行态提交顺序。测试调用真实方法及进程内存储事务，不代表原生事件分发或完整 provider-loop 端到端验证。成功执行证据见下文；已按生命周期清理临时源码及独占支持。

用户已批准仅开发期启用既有 GPUI 的 `test-support` 和必需传递依赖。锁文件增加 8 个测试支持包；未删除既有包，既有版本及校验和不变。离线完整 metadata 在解析并写出锁文件后因既有 `cc 1.2.67` 缓存缺失失败，不是编译或测试结果。未通过本地构建绕过。

## 本地静态证据

- `cargo fmt --all -- --check`：开发提交及基线整合后均通过。
- `git diff --check`：通过。
- 本次界面差异的 18 个字面 UI 文案键在中英文目录中均存在。
- `scripts/check_i18n.py` 全仓扫描：**未通过**，存在未改 config/updater 分类失效及既有未审查文本；没有擅自清理卡外分类。该补充扫描不属于现有 required workflow。
- 不运行 Vega，不本地编译、构建、Clippy、测试或打包。

## 独立静态审查

- Composer/设置/文案范围：发现三个 P2（禁用原因缺失键、加号关闭后重开、候选失败无法重新请求），已修复；增量审查未发现剩余可操作问题。
- 对应停写差异 SHA-256：`41290700721769c9311e267573f456241cdfa94bc4cddb7250eca3efc1293528`。这是同步基线前的范围差异身份，不冒充最终提交身份。
- 编辑器及 17 条临时回归：独立静态审查无可操作发现；未运行测试。当时九文件有序哈希清单摘要为 `7c4c6f6b6e65ae3ca9c33660f6b0580647b76077083d2dad981aea77503aa498`，不是 diff 哈希。
- 后端：独立审查发现 P1，接受引导消息后再分开持久化技能快照/审计，撤权或存储失败会丢失消息绑定。已在 `6ad56de59a6a6b02c02d545cfd7221183f4afbae` 修复，独立增量静态审查确认原 P1 关闭。消息送达、下一助手消息、完整快照和激活审计同事务提交，两处消费点等待持久化回执后才替换 live run，失败释放本次 claim；6 条新增回归覆盖实际事务拒绝/回滚和运行态提交顺序，但不冒充完整 provider-loop 端到端测试。审查前后 17 个后端文件哈希相同，确认基线整合未改变该范围；有序清单摘要 `a5edaf524d607db94dafb618dcf92986cd166c14594a348ad9e69fbb3a80c709`。

## 云端与最终门禁

首轮 Actions [41616743508](https://github.com/puzige/vega/actions/runs/41616743508)（head `e45838d27149d12c8cd88c5aee4b677c0cc1e635`）失败：E0599（非 Stateful Div 上调用 role）、PreparedSkills 接口可见性、临时测试未使用导入；Rust 测试构建失败，发现未完成、没有执行结果，不能称为测试通过。编辑器修正通过添加稳定 element id 建立 Stateful 无障碍接口，移除未使用导入；断言保留。后端可见性与 P1 修复已整合至下一版本。

第二轮 Actions [41623277273](https://github.com/puzige/vega/actions/runs/41623277273)（head `6ad56de59a6a6b02c02d545cfd7221183f4afbae`）：41 发现 / 41 执行，34 通过 / 7 失败 / 0 跳过，编辑器 17 条全部通过。7 个 conversation 回归在共享夹具 `capture_fence` 阶段失败 `InvalidTaskIdentity`，因为任务字符串不是合法 ULID，尚未到达核心断言。生产权限检查未放宽，测试夹具统一使用合法 ULID。Clippy 另报 RuntimeEvent 大枚举载体，技能包改为 `Option<Box<RuntimeSteeringSkills>>`，原子行为不变。修复发布为 head `a3d036bb7cc416e5bd5ad22aa9cb24606c340ba0`。归一差异核对确认仅夹具身份、Box 构造和 rustfmt 尾逗号变化，所有断言保留。

第三轮 Actions [41625526950](https://github.com/puzige/vega/actions/runs/41625526950)（head `a3d036bb7cc416e5bd5ad22aa9cb24606c340ba0`）：Clippy 通过；41 发现 / 41 执行，40 通过 / 1 失败 / 0 跳过。剩余权限回归未注入故障：SQL 仍使用旧任务字符串，更新零行，接受成功导致 `unwrap_err` 失败。仅夹具 SQL 改为绑定 `original.thread_id` 并新增 `assert_eq!(changed, 1)`；生产代码和原回滚断言不变，独立增量静态审查确认是加强故障命中的夹具修正。修正发布为 head `bcdac1fe6b8e87fe8084c353edf17a9d6ea3fa48`。

第四轮 Actions [41626920054](https://github.com/puzige/vega/actions/runs/41626920054)（head `bcdac1fe6b8e87fe8084c353edf17a9d6ea3fa48`）全部成功：Nextest job `125886467748` 实际 41 发现 / 41 执行 / 41 通过 / 0 失败 / 0 跳过；Clippy job `125886467342`、required gate `125888445798` 成功；发布脚本 Python 套件 5 + 15 + 40 条通过。

清理前已推送可恢复源码标签 [`evidence/644-composer-development-bcdac1fe`](https://github.com/puzige/vega/tree/evidence/644-composer-development-bcdac1fe)。完整开发差异可从真实基线 `9b84112097597f49537994f7de1a508c9e374b75` 与上述绿色 head 重新生成；`git diff --binary` SHA-256 为 `25e3e9ded806a0ac82c127b25f31673f1ff95e050145a73fdf77d4a0a156d0a5`。源码与结果已固定到 [PR 证据评论](https://github.com/puzige/vega/pull/874#issuecomment-6602627086)。

| 临时测试源码 | SHA-256 |
|---|---|
| `crates/vega_ui/src/text_input/tests.rs` | `91ee08379148b16ee90ed6e4e8143cf5d945bcdd713043298d6302bc2433d075` |
| `crates/vega_conversation/src/development_regressions.rs` | `b0fe2dbee190e25ac52dd1cc9bd467020bc6f3de845292d012c7df6c3576f29a` |
| `crates/vega_runtime/src/skills/development_regressions.rs` | `2b00c7d4eea1c02fb17426e89842fa2f75e758ff6dca215037c7a5fb282fc8c4` |
| `crates/vega_runtime/src/agent/steering_development_regressions.rs` | `f79b9134548a83147875326eb189f904f47cb32d4d9acc696608819634ea9851` |

成功留痕后，仅删除这四份测试及模块接线、GPUI dev-only test-support 和 8 个独占传递包；生产实现不变。锁文件逐包核对后恢复到当前 master 的同一生产依赖图（同时移除测试开启的 backtrace/proptest 边与重复版本名消歧）。清理后的最终 head 仍需完整 required CI 和清理增量审查；`no Rust tests`（0 发现/0 执行）不冒充开发回归通过。

## 用户手测边界

原生鼠标/键盘、IME、屏幕阅读器、浅深主题和窄窗口、系统电脑接管、真实模型压缩及实际技能运行仍待用户纯手动验收。静态检查、Actions 或合并均不等于用户验收；确认前不关闭 Issue 或置 Done。
