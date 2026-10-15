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
