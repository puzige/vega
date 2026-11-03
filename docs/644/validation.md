# #644 实施与验证记录

## 当前阶段

开发验证中，未合并、未安装、未用户验收。功能 PR：[#874](https://github.com/puzige/vega/pull/874)。Issue 保持 In progress。

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
