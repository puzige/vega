# #743 会话级思考档位

## 契约与范围

本规格取代 R2 中 Composer 直接共享会话偏好的契约；Settings 编辑新会话默认模板，能力声明和上下文策略仍为全局。#767 修订：Composer 选择除更新目标会话外，也更新未来新会话的默认模板；其他已初始化会话不受影响。详细契约见 [#767](../767/reasoning-defaults.md)。当前轮 FrozenReasoning 不变。

每个 thread 保存独立的偏好快照，键为精确 provider/model。初始化时冻结当时全部已配置模型的默认偏好；缺失键使用 provider_default，不动态回退到全局。切换模型后恢复同一会话该模型的偏好，不向其他模型套用档位。能力变更后非法的旧选择拒绝发送，用户可重新选择合法值。

新增可空 JSON 列：NULL 表示尚未初始化，空数组表示已初始化且全部使用 provider_default。草稿在首个有效模型目录加载后冻结默认模板（此前禁止发送），物化时原样保存。之后目录刷新及 Settings 修改不覆盖快照。旧持久会话首次后台加载时，以当时 `reasoning-preferences.toml`（`REASONING_FILE_NAME`）一次性初始化，数据库 NULL 条件写入避免覆盖并发初始化；初始化失败保留错误，不将其当成空快照。无旧独立值可恢复，不声称恢复历史上未存储的档位。

Composer 保存更新目标 thread 的精确键，在后台事务内合并最新值，不替换其他键；#767 起同时持久化未来新会话的默认模板。保存协调器串行化并合并拖动意图；回执绑定操作及来源 thread/stream，路由改变不使旧回执投影到另一会话。失败恢复已确认选择并显示可恢复错误，部分成功须如实反馈。草稿保存更新同一 thread 快照并同步窗口草稿，之后物化不丢值。

会话投影始终用共享能力加会话偏好；模型选择回执、缓存会话切换、Settings 返回和 pending 模型恢复均不再读取全局偏好作为现有会话选择。新选择只影响后续运行；保留请求模型匹配、能力合法性、准入和权限边界。

## 验证

开发 Rust 测试在 Actions 验证后固定完整 SHA/源码/实际结果，合并前按现行政策清理测试及独占接线。最终 head 另跑完整 required CI。禁止本地运行、编译或测试 Vega。

覆盖：同 provider/model A/B 独立保存与冻结请求；修改全局默认后 C 继承新值、A/B 不变；空快照不回退；重新加载/重启持久恢复；旧会话一次初始化；草稿物化；切模型再返回；错误及晚到回执、连续修改；staged 模型与当前运行冻结值隔离。用户手测验证 UI/重启及真实请求；CI 不替代用户验收。

## 版本

基线：bdf9b8b2（实现提交固定完整 SHA）。无新增依赖。不修改上下文预算、权限或 provider 能力协议。

## 审查修订

保存回执确认已持久值，但滑块显示保留更新的同模型意图；精确 revision 确认才能清除意图。能力收窄后的旧非法选择不映射成虚假合法选中态，点击唯一合法档位仍必须发出保存事件。Codex 草稿和 Native 草稿同样序列化初始化快照，保留空数组和 NULL 的区别。补充回执交错、唯一合法档位重选及 Codex 物化行为测试。


## 开发验证与交付证据

- 基线完整 SHA：`bdf9b8b217fc8529b6c97f3f54ef9a3e0d62d76d`。
- 带测试版本：`f2e428ed85c835a7c6e7a4a0636d2e7db67477a2`。
- [Actions 40801699612](https://github.com/puzige/vega/actions/runs/40801699612)：Clippy、Nextest、required check success；Nextest job `123211062943` 原始日志确认 **11 discovered / 11 executed / 11 passed / 0 skipped**。
- [6 个持久化测试原始源码](https://github.com/puzige/vega/blob/f2e428ed85c835a7c6e7a4a0636d2e7db67477a2/crates/vega_conversation/src/session_reasoning_tests.rs)，SHA256 `0f4673dc5b42d3b91fc1d4d4777c83fcb149a21ac5d131e80a6172fe59946137`。
- [5 个 UI 状态测试原始源码](https://github.com/puzige/vega/blob/f2e428ed85c835a7c6e7a4a0636d2e7db67477a2/crates/vega_ui/src/conversation_stream/session_reasoning_tests.rs)，SHA256 `ce9115744ba30d8eb541aade481363268d60a004ffced17d315290be96504ef0`。
- [完整原始补丁](https://github.com/puzige/vega/compare/bdf9b8b217fc8529b6c97f3f54ef9a3e0d62d76d...f2e428ed85c835a7c6e7a4a0636d2e7db67477a2.patch)，已读回的 [Issue 证据](https://github.com/puzige/vega/issues/743#issuecomment-6485596789)。完整提交经 GitHub API 确认可达，不仅依赖临时 artifact。
- 独立审查在上述精确版本无阻断发现；先前发现的旧回执、新能力下非法选择恢复、Codex 独立 INSERT 遗漏均已修正。
- 失败记录：首轮 `40799225647` 在测试执行前因 xtask 夹具漏字段编译失败；第二轮 `40801060428` 的 5 UI passed / 6 存储 failed / 0 skipped，原因是测试夹具漏建表。修正责任层、保留全部断言后，第三轮取得上述 GREEN。
- 按仓库政策仅清理两份开发测试、两个模块接线和 `vega_ui` 的独占 dev `test-support`，生产行为不变。最终 head 静态清点及 required CI 另行记录于 PR；空集写 `no Rust tests`，不替代上述 11 项行为验证。
- 本地仅执行格式、差异及静态检查。未本地运行 Vega、编译、测试或打包。测试使用真实 SQLite 和 stream/滑块事件，未执行真实 provider/Codex 进程，也未声称窗口异步 worker 全链路 E2E；真实 UI/重启/服务请求等待用户手测。
