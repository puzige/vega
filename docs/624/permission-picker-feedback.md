# 权限选择器手测反馈修订

2026-10-27 用户反馈：权限菜单文案冗余，重复确认打断操作；参考所提供截图的模式名称与简短能力说明。本增量接续 #624 / #653，优先于 `permission-ui-completion.md` 中“同名重新授权”作为普通选择器默认动作的要求。

## 验收

| 前置状态与操作 | 可观察结果 | 验证 |
|---|---|---|
| 打开权限菜单，鼠标或键盘选择当前已授权的完全访问 | 名称始终为“完全访问”；关闭菜单，不请求确认、不写入、不轮换 grant 或 revision | 静态事件链检查；必要开发回归交 Actions |
| 新草稿继承已确认的未来默认完全访问 | 不误报缺少授权，再选相同模式不额外确认；实际创建仍核对配置来源、revision、epoch 与 grant | 静态来源检查；Actions；用户手测 |
| 旧会话无 grant、grant 格式/版本/作用域错误，或默认来源不匹配 | 保留缺少授权状态及选择完全访问后的首次确认入口，不凭模式名称补权 | 必要开发回归交 Actions |
| 首次从其他模式启用完全访问，或撤权/重置后启用 | 保留明确确认；取消无写入；原持久化与 reset 门禁不变 | 静态调用链检查；用户手测 |
| 查看中英文菜单 | 每项只有模式名称和一句能力/审批说明；没有“重新授权”常驻名称；完全访问描述互联网及电脑文件访问能力 | TOML、键一致性和差异检查；用户手测 |

## 实现与边界

1. 普通选择器名称不随选中状态改为重新授权；确认和自动模式使用面向用户的审批动作名称。
2. 共用一处 UI 选择判断：线程 grant 必须格式/版本及 thread scope 匹配；继承默认仅认可合法新草稿、与当前 `NewTaskDefaults.permission` 完全一致的默认来源，以及相同模式和配置身份的 future-threads grant。默认模式、revision 或来源变化后，旧草稿必须恢复缺少授权提示与显式确认入口。该判断只影响提示与同值选择，不是执行权限凭据；不在渲染阶段做 IO。
3. 选中已授权的当前模式为无副作用操作；未授权的同名完全访问仍走既有准备/确认服务。真实执行、草稿创建继续验证持久来源与当前 epoch，不能用 UI 判断替代授权。
4. 受限模式的沙箱、网络单次扩围、危险命令审批、自动化的独立授权及 reset 行为不变。不将默认授权补给既有会话，不修改用户配置或数据库。
5. 本地只做快速静态/格式检查；编译和自动回归交既有云端 PR Check。Rust 开发回归若使用，记录可恢复来源及实际结果后清除，最终 head 保持无 Rust 测试。非实现者审查本次差异，最新 required CI 通过后合并，仍待用户手测。

## 开发验证状态

最小实现及两项开发事件回归已写入：通过真实 ConversationStream 的订阅观察权限请求，检查已授权会话/默认草稿无请求和无状态修改，缺失/错误 scope/错误配置/损坏授权/分叉/已变 revision 仍请求授权，从受限模式首次进入仍请求确认。未恢复历史测试集合。

本地 `cargo fmt --all -- --check`、`git diff --check`、中英文 UI TOML 解析及键集合一致性检查通过；没有运行 Vega、编译或执行测试。

独立审查在 `eab5c46313482b465c0db75db5598382929ccde2` 发现 P2：草稿创建后在设置里修改未来默认，会保留旧草稿快照；若只检查快照内部一致性，同名完全访问不再发出确认请求，创建服务却因当前配置已变拒绝物化。修正当前默认来源比较，并增加设置模式或 revision 改变后的真实全局状态/事件回归，不绕过创建服务来源检查。

## 云端开发结果与清理

修正版 `a30f01a6e42a3dfdecdcb90dc2c253698b9c8e68` 已获同一非写入者增量复核，P2 解决，无新增可行动发现。[Actions 40534487506](https://github.com/puzige/vega/actions/runs/40534487506) 对应本精确 head：Clippy job `122296385309`、Nextest job `122296386235`、required 汇总 `122298881914` 均成功。

- `cargo nextest list --workspace --message-format json` 实际发现 2 项；`cargo nextest run --workspace` 实际执行 2、通过 2、失败 0、跳过 0。
- 三组现存 Python 检查实际 5 + 15 + 40 = 60 项通过。
- 原始两项测试源码：[picker_tests.rs](https://github.com/puzige/vega/blob/a30f01a6e42a3dfdecdcb90dc2c253698b9c8e68/crates/vega_ui/src/conversation_stream/thread_permissions/picker_tests.rs)，SHA-256 `75b250ae44dafc7ce3d72277800903dec2922297132f7f51f660eddc001c13a2`，Git blob `33615065f2a0aa305ea0db64d69048daae824740`；已从远端读回核对哈希。
- [完整开发差异](https://github.com/puzige/vega/compare/b95a9c507f2149e2f6bc9a988965783ef0fa8560...a30f01a6e42a3dfdecdcb90dc2c253698b9c8e68.diff) 保留测试及其接线，不从旧 ref 恢复历史集合。

取得上述真实结果后，按交付政策移除这一个测试文件、专用模块声明及 `vega_ui` 的 `test-support` dev dependency。本次生产行为不因清理改变；CI、Python/fixture、benchmark、迁移、Cargo.lock 未改动。最终 head 仍须完整 required CI 确认 `no Rust tests`（0 发现/0 执行），不以开发 2/2 替代。

最终精确 head、清理增量审查及门禁结果固定于 [PR #691](https://github.com/puzige/vega/pull/691) 和 [Issue #624](https://github.com/puzige/vega/issues/624) 交付记录。云端通过与合并不代表原生显示、键盘或真实系统集成手测通过；用户明确结论前保持 Issue 开放。
