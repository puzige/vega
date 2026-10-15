# Issue #397 UI 交付

## 实现

- 压缩行进入现有 process spacing 投影；运行中 `Compacting context`。成功/取消保留记录，但渲染、row count、估高和历史恢复不再显示临时行。新一代成功解决同模型较早压缩失败，不清除独立 run failure。
- 失败按所属回合底部投影为可换行的 danger 文本，无背景、边框、圆角、警告图标。独立手动失败保留原位置；恢复按钮仍可用。重连保留原有临时提示样式和估高。
- 不同 typed 压缩/运行失败保留在同一底部区域两行，同一 OverLimit/ReasoningLimit 去重，不读取 raw provider body。
- 无具体失败的 Interrupted 不另显示错误，运行头保持 Stopped/Stopped after 与冻结计时。真实迟到错误允许同回合 Interrupted → Failed；Completed 不受迟到错误污染，另一回合错误不写当前 composer。Runtime/Persisted 泛称不覆盖已有具体原因。
- RunFailureKind 安全展示文案与压缩错误统一英文；模型、命令、工具和 provider raw fixtures 不翻译。

## 文件

UI：conversation_stream 的 context_control、content、core、phase、model、message_anchors、render、render_rows、run_activity、run_notice。
共享展示：vega_conversation/src/types/events.rs 的 RunFailureKind.message 及直接断言。
测试：conversation_stream/tests/context_control、hydration、issue146_run_activity、issue296_stream_polish；vega/src/tests/r69。

## 验证

已做定向 rustfmt、git diff --check 与全仓旧文案/Interrupted/compaction 可见性静态搜索。未执行本地 compile/build/check/test/clippy。
新增回归覆盖：浅深主题压缩行间距、成功/取消清空与恢复隐藏、底部不同原因合并/去重、新一代成功解决旧失败、迟到错误归属和时钟冻结、Completed 终态保护、泛错误不覆盖具体错误。既有长错误换行、隐私 sentinel、脱离尾部滚动、历史顺序及恢复按钮断言保留。编译与GPUI/nextest结果待云端CI，不宣称已通过。

## 边界

无新增取消来源字段。历史无具体错误的中断保留中性 Stopped，不追溯猜测用户或系统原因。Completed 后迟到 Error 不升级已完成回合；仅 active/Interrupted/Failed 接受同回合错误。后端取消竞态和pipeline typed Cancelled处理由独立executor交付，不在本UI报告声称运行验证。

## PR #398 第二轮 CI 修复

云端 Clippy 通过；Nextest 2710 passed / 7 failed / 5 skipped。本 UI/App 切片负责其中 6 项，pipeline 取消失败由后端 executor 处理。

- `issue397_new_success_resolves_old_compaction_but_preserves_run_failure` 暴露真实投影遗漏：过程行已判 resolved，但通知收集仍纳入旧 Failed 记录。通知收集现复用 `compaction_failure_resolved`，保留原新成功/独立运行失败断言，并补缓存通知为空的断言。
- App reopened compaction 测试原先等待已被契约移除的取消行，改等待已有真实异步 load 成功探针；保留持久化 Cancelled、unknown usage、非 busy 和新 stream 验证，明确行与错误不出现。
- 两个 desktop UI 测试及 retry 测试沿用旧错误图标期待，改验证可见纯文本、无图标、长文本换行与全宽/区域内几何；增加无卡片边框断言，权限图标、首行对齐、按钮与键盘决策覆盖不变。
- stream polish 的新回合 Interrupted 已验证中性状态，却残留 notice 可见断言；改断言其不可见，保留旧回合具体错误、边界、折叠/展开和归属覆盖。

编辑前 fetch 后 HEAD 与 upstream 为 0/0。已扫描全部同类选择器期待，定向 rustfmt 与 diff 静态检查通过；未运行本地编译/测试，修复待下一轮云端 CI。

## PR #398 第三轮 CI 修复

运行 38766283475：2717 passed / 1 failed / 5 skipped。唯一剩余失败是 stream polish 在 prepend 历史页之后仍期待两个通知；该用例此前已正确验证只有旧 Failed 回合有通知、新 Interrupted 回合没有通知，prepend 用户历史不应增加通知。最小修复仅更新测试：期待一个通知，同时明确验证其 message_id、具体 quota 文案、entry identity 均归属 old，new 仍为 Interrupted。保留原 controller 错误独立性与后续具体错误断言，不更改 production 或 Completed 保护。fetch 后与 upstream 0/0，定向格式及差异检查通过，未本地编译/测试。
