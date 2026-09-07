# R1 — 当前任务模型选择补修（A2-14 / S8-T47）

版本：v0.5 · 2026-09-05
Owner / reviewer：Codex；executor：Luna / max（用户指定，不再使用 pi）
状态：规格已冻结，待实现与独立验收
关联：`vega-s8-tasks.md` T47；`vega-exec-guide.md`；`vega-features.md` A2-14

## 问题与语义勘误

当前 Composer 模型选择改变 UI 和 config.defaults.model，却未改变当前 threads.model。
实际发送与定价读取 durable Thread.model，导致“界面 B、当前任务请求 A”。配置写入错误又被忽略。
本补修明确：**会话内模型选择作用于当前任务的下一次运行**；Settings 中的新任务默认模型维持原语义。
thinking 的请求/持久化不在本卡，原 T47 报告对 thinking 的表述不构成本卡实现依据。

## 冻结契约

1. UI 发出选择意图，不先声称保存成功。由 app/conversation/store 服务更新既有 threads.model，再回读权威 Thread 并更新对应 UI。
2. 会话内选择不改 config.defaults.model；不新建列/表，零 migration、零新依赖。
3. 模型必须精确存在于已配置 provider，provider 解析须 unique，且存在于 Ready 定价 authority。unknown/unpriced/ambiguous 按现有规则拒绝；不得读 Keychain 或发 discovery 请求来检查选择。
4. run 使用的 model、provider 选择和 run-start pricing 必须一致。选择只影响下一次运行；已开始的 run 及历史 usage/cost 不能被重定价。
5. active run、pending plan/review/permission、受信动作或 stale route/entity 请求不得改变运行配置。复用现有 fail-closed guard；多次请求只允许单个 owner。
6. 在异步保存开始前安装 busy/single-flight owner；完成前不能 submit、启动已批准 plan 或开始冲突的受信动作。所有成功/失败/丢失结果路径都要释放精确 owner，不能由晚到 A 释放 B。
7. 文件/SQLite IO 在 worker 上执行，不进入 render 关键路径。选择服务的失败必须保留旧权威值并显示有界错误；UI 不得吞错误回显成功。
8. stale callback 不得写入新 route/entity 的 UI，不得覆盖较新的选择；失败后需要重新加载权威状态时也必须受同一身份/generation 约束。
9. 关闭重开与重启后显示 durable Thread.model，下一次发送捕获到的请求 model 仍与之一致。
10. 精确匹配与定价责任留在现有层级；不在 UI 实现成本公式、SQLite 访问或 provider 协议。
11. 配置 provider 修改后，回到任务时刷新可选模型投影；不能永久缓存启动时的空列表或旧列表。异步 catalog 刷新也须拒绝旧 generation 结果。
12. 模型保存期间模式 / 权限的变更属于冲突设置，真实 app handler 与 UI 都应拒绝；避免晚到 Thread ack 把已更新的模式 / 权限投影覆盖。A→B→A 后需对当前 A entity 重新应用 durable authority，原 entity 精确 owner 仍须收尾，不能留下永久 pending。
13. 模型保存的精确 owner 必须与通用 trusted-action busy 分开判定。已有分支 / 提交 / Artifact 操作时模型入口拒绝创建新 pending；即使已进入 app handler，也须明确拒绝并清理本次 pending，不能把通用 busy 当成重复模型请求而静默遗留 owner。拒绝模型请求不得释放其它受信操作的 lease 或清除其 busy。
14. 模型 ack 与 Settings deferred 刷新只拥有本次模型字段，不能用延迟保存的完整 Thread 覆盖较新的标题 / pin / status / mode / permission 等投影。Settings 打开期间侧栏重命名仍是合法操作，关闭时须保留该新标题并应用已落库的模型；一般晚到 ack 与 A→B→A 也遵守相同字段归属。不得通过 render 同步 SQLite 回读解决。

## 实现归属

- `crates/vega_store/src/threads.rs`：既有 model 列更新。
- 必要的 `crates/vega_store/src/config.rs`：供 worker 使用的最小路径参数化只读配置入口；生产配置根沿用 paths::config_dir，测试传入 owned 文件，不改 HOME/XDG、不把数据库父目录当配置根、不在选择时补写配置。
- `crates/vega_conversation/src/threads.rs` / `types/`：权威服务与必要共享类型。
- `crates/vega/src/window/{mod,session,render,agent,pricing}.rs`、`app_agent.rs`：控制器、worker、ack 与 gates；pricing 仅将 Ready authority 的变化投影给模型选项，不改定价算法或语义。
- 必要的 `crates/vega/src/trusted_action.rs`：只可增量纳入 model-selection owner，不改变其它受信动作的语义。
- `crates/vega_ui/src/conversation_stream/{core,mod,render,composer,content}.rs`：意图、pending/错误显示与权威值应用；composer 接入 pending submit / 冲突设置 guard，content 仅导出既有 permission / plan / busy 状态的守卫与必要 owner 观察。
- 上述模块原有测试目录及相邻新测试模块；仍保留 1000 行限制。
- 本文、`vega-review-r1-delivery.md` 与 T47 的补修引用。

不改 runtime/provider 协议，不做 thinking，不做 Git 来源/兼容策略，不做性能批次，不新增依赖/DDL，不删安全断言。

## E2E-first 验收

主干：owned temp profile/SQLite + 已有 production app handler + MockProvider request recorder。

1. 创建模型 A 的任务，选择配置且已定价的 B。
2. 验证 pending 期间无法发送；成功 ack 后 UI 与 durable Thread.model 都是 B，全局默认不受影响。
3. 通过真实 submit handler 执行，recorder 捕获 ChatRequest.model == B；定价仍使用相同的 exact-model authority。
4. 关闭重开/模拟重启后，显示与下一次请求仍是 B。

最小补充：一个代表性持久化失败证明不假成功、零错误请求；一个 active/stale 保护回归证明不能改错任务/运行；复用现有 unknown/unpriced/权限安全回归，不补笛卡尔矩阵。

v0.5 独立 review 回归：在现有 owned app-handler E2E 内验证通用 trusted busy 下选择模型不会遗留 pending / 释放其它 owner，以及 Settings 中模型完成后重命名、关闭 Settings 仍同时保留新标题与已保存模型。保留原重复投递 / late callback 保护，不造颜色或纯实现镜像测试。

禁止只测 label/config.defaults 来代替请求入口。允许 owned 路径传参和 MockProvider 边界，不得为覆盖率扩张生产 public API 或引入大批测试专用状态机。

## 门禁与交付

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
cargo build --workspace --locked
git diff --check
```

先跑本卡聚焦 E2E，再跑全量。原始日志留仓库外，交付报告按 exec-guide §7 格式记录测试树、命令/时间、结果与残余，不记真实 key/provider 正文/私人 workspace 数据。

已知 R0：当前机器固定 Git 无 check-attr --source 能力，基线存在分支 E2E 失败。允许独立完成本卡实现及聚焦 review，**不放行最终集成**；全量失败必须保留与基线区分，不删检查、改断言或只重跑至绿。

executor 不 push/merge/release；最多三个本地 commits。完成返回交付文档与真实 diff 给 Codex，Codex 独立复核后决定下一步。

## 变更记录

- v0.1（2026-09-05）：Codex 在用户授权直接派发后冻结 R1 补修；明确 pending 期间禁止发送及精确 owner 清理。仅修当前任务模型选择，不扩展 A2-14 的 thinking/provider 语义。
- v0.2（2026-09-05 11:14）：Codex 明确将现有 submit 入口所在的 composer.rs 纳入实现归属，仅用于契约 6 已冻结的 pending gate，不扩大产品语义或其它功能范围。
- v0.3（2026-09-05 11:33）：依用户最新要求停止 pi，交 Luna / max 完成同一张 R1；明确异步配置只读入口及独立配置/数据根，保持产品契约和验收门槛不变。
- v0.4（2026-09-05）：主 Agent 二轮 review 明确 exact-owner 收尾、A→B→A durable 刷新、模式 / 权限冲突设置与 Settings 后模型列表失效；补齐 content / pricing 两处投影归属，仍不改 provider / pricing 协议。
- v0.5（2026-09-05）：主 Agent 核实独立 review 两项缺陷，冻结模型 owner 与通用 busy 的区分，以及 ack / deferred 刷新的字段归属，防止 pending 永久卡住和旧 Thread 覆盖较新侧栏编辑。
