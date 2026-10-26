# 停止后的任务在 composer 显示播放键以继续

- Issue：[#591](https://github.com/puzige/vega/issues/591)
- 生产基线：`1ced1f10`（master）
- 参照：本机 ChatGPT 桌面版解包源码（`codex-extract/webview/assets`）
- 规则：[AGENTS](../../AGENTS.md)、[执行总纲](../vega-exec-guide.md)、[设计守则](../vega-design-guidelines.md)

## 1. 问题与现状证据

用户手动停止任务后，Vega 的 composer 与“从未运行过”完全同形：`render.rs:340-386` 在 `!running && !submit_pending` 时一律渲染 `Icon::ArrowUp` + `aria_label("发送")`。用户无法从主按钮判断这轮是被中断的、可以续跑。

中断事实本身已持久化：

| 事实 | 位置 |
|---|---|
| `AssistantStatus { Done, Interrupted, Failed }` | `crates/vega_conversation/src/history.rs:150-163` |
| `interrupted` 是合法落盘终态 | 同上，`parse` 接受 `"interrupted"` |
| `AssistantStatus::Interrupted → RunActivityStatus::Interrupted` | `crates/vega_ui/src/conversation_stream/content.rs:7` |
| 运行指示渲染 `Stopped after {duration}` / `Stopped` | `run_activity.rs:1122,1133` |

缺的不是数据，而是把该终态暴露给 composer 主按钮。

停止链路现状：

- `composer_actions.rs:256-268` `request_composer_stop`：只置 `stopping = true` 并 emit `ComposerStopRequested`。
- `composer_actions.rs:228-243` `finish_composer_run`：`running = false`、`stopping = false`，`stopped` 布尔返回后 UI 不留痕。

## 2. ChatGPT 对照（解包源码，事实）

composer 提交按钮组件 `plr`（`app-primary-d63a2421d501.js`，偏移 ≈4826338）把主按钮做成多态，其中 Resume 分支：

```js
const Qe = intl.formatMessage({
  id: `composer.submitButtonTooltip.resume`,
  defaultMessage: `Resume`,
  description: `Accessible label for resuming an interrupted task`,
});

} else if (k || ae != null && ge && !E && !_ && !A && P === `empty-message`) {
  ct = jsx(T4, {
    supportsMorphing: true,
    ariaLabel: Qe,
    disabled: !d,
    isLoading: k,      // isResumePending
    Icon: OUe,         // 实心播放三角
    onClick: M,        // onResume
  });
}
```

图标（`app-initial` 偏移 6488905，导出 `Lwa as wW` → `wW as OUe`）是 20×20 实心播放三角：

```js
Lwa = e => jsx(`svg`, {width:20, height:20, viewBox:`0 0 20 20`, fill:`none`, ...e, children:
  jsx(`path`, {d:`M6 14.7227V5.27693C6 4.29057 7.08894 3.6928 7.9211 4.22235L15.3428 8.94526C16.1147 9.43645 16.1147 10.5632 15.3428 11.0544L7.92109 15.7773C7.08894 16.3069 6 15.7091 6 14.7227Z`, fill:`currentColor`})})
```

可继续判定 `Jsr`（`app-primary` 偏移 4769163）：

```js
function Jsr(e, t, n) {
  if (n) return false;
  if (e === `interrupted`) return true;
  if (e !== `failed` || t == null) return false;
  const { codexErrorInfo: r, message: i } = t;
  return r === `serverOverloaded`
      || r === `internalServerError`
      || (typeof r === `object` && r && (
            `httpConnectionFailed` in r
         || `responseStreamConnectionFailed` in r
         || `responseStreamDisconnected` in r
         || `responseTooManyFailedAttempts` in r))
    ? true
    : r === `other` && !Xsr.test(i) && Ysr.test(i);
}
```

其中 `Xsr` 排除配额类错误（`usage|quota|rate limit|billing|credits`），`Ysr` 匹配“模型暂不可用 / 流提前断开”这类瞬时错误。

三点可直接采纳的结论：

1. Resume 是**主按钮位替换**，不是并排新增小按钮。
2. 图标是实心播放三角，Vega 已有同族 `Icon::Play`（`icons.rs:240`，`automation.rs:458` 已用）。
3. 判定覆盖 `interrupted` 与**可重试**失败；不可重试失败不给 Resume。

## 3. 本卡范围

### 目标

本次用户范围裁决：仅支持 Native 任务，A01–A10 均按 Native 验收。Codex ACP 保持现状，不显示本卡 Resume；其已有会话续跑不在本卡范围。静态核对基线 `b03d0d5f2c7da0efab147a4fec0d26e2adefc4e3`：`submit_codex_composer` 仅支持新建任务，Codex worker 总是新建会话并插入 user 行，不能作为本卡续跑路径。

1. `Interrupted` 后 composer 主按钮显示播放三角 + `Resume`（中文「继续」），点击续跑。
2. 续跑不新增 user 消息行，复用既有历史末尾；`messages.seq` 连续；已终态工具不重复执行。
3. 输入框有内容或附件时，主按钮优先显示发送；清空后回到 Resume。
4. 续跑进行中显示 loading，重复点击不产生并发 run；运行中仍显示停止按钮。
5. 依据持久化 `interrupted` 终态判定，重启应用后仍显示 Resume。

### 非目标

- 不改失败态的重试/恢复交互（`Failed` 不显示 Resume）。
- 不引入自动续跑、后台重连或新重试策略。
- 不改停止的取消语义、工具终态或持久化 schema。
- 不改 Plan 审批路径既有的 `uses_existing_user` 约束。

## 4. 验收矩阵

| ID | 前置与操作 | 必须观察 |
|---|---|---|
| A01 停止后可继续 | 运行中点击停止，等待终态 | 主按钮变为播放三角，`aria_label` 为「继续」/`Resume`；运行指示消失；不再显示发送箭头 |
| A02 点击续跑 | A01 状态点击该按钮 | 立即进入 running 并显示停止按钮；**不新增 user 消息行**；模型从既有历史继续；原 `interrupted` 助手行与工具终态不变 |
| A03 输入优先 | A01 状态输入文字 | 主按钮立即变回发送箭头；发送走既有新消息路径，不触发续跑 |
| A04 附件优先 | A01 状态添加附件（输入为空） | 同 A03 |
| A05 幂等 | A01 状态快速连点 | 只启动一次 run；无重复 assistant 行；loading 期间不可重复触发 |
| A06 重启一致 | 停止后重开应用，打开同一任务 | 仍显示 Resume（依赖持久化终态，不依赖内存标志） |
| A07 无中断不变 | 从未运行 / 正常 `Done` 结束 | 主按钮为普通发送箭头，与当前行为一致 |
| A08 失败不误判 | 因配额/鉴权等不可重试错误失败 | 不显示 Resume；现有错误提示与恢复入口不变 |
| A09 键盘与读屏 | A01 状态 Tab 到主按钮 | 焦点环可见；Enter/Space 触发续跑；读屏播报「继续」 |
| A10 草稿不残留 | A01 状态输入后清空 | 重新显示 Resume；草稿不残留、不自动发送 |

## 5. 实现方向

主按钮三态，优先级固定：

```
running                                  → Stop
!running && (输入非空 || 附件非空)        → Send
!running && 本轮以用户中断结束            → Resume
其余                                      → Send
```

- 展示态来源复用既有 `terminal_cancelled` 与持久化 `AssistantStatus::Interrupted`，不新造第二份真相。
- 新增 `ComposerResumeRequested`（与 `ComposerStopRequested` 对称），由 `vega/src/window/agent.rs` 接管并走既有 run 启动链路。
- `pipeline.rs:912` 的 `uses_existing_user` 当前仅接受 `plans::APPROVAL_INSTRUCTION`。续跑需要新增一条受控分支，复用 `credential_continuation` 的“不插入新 user 行”语义；保留既有身份、seq、CAS 校验，不得放宽为任意复用。
- UI 与启动入口均限制 Native；不得将 Codex 任务路由到 Native 或新建替代 ACP 会话。
- 中断候选必须对应持久化历史实际末尾，不得从旧分页中的任意 Interrupted 行推断。启动事务重新核对线程、候选身份、末尾序号与无活动 run，并在同一事务插入新 assistant；旧请求不得重复启动。
- Resume 不接管 pending message，也不绕过 credential continuation 的身份、source fence 或未知效果保护。历史投影、工具终态结果映射与 call ID/输入一致性校验保持原有契约；不宣称跨不同 call ID 的外部副作用 exactly-once。
- 图标用现有 `Icon::Play`。#589 已合入，主按钮、loading 态及 tooltip 使用 `ui.composer_resume` 国际化 key（Resume / 继续），不修改其他恢复动作的文案。
- 2026-10-26 同步 master：保留 ordinary submission 的准备、取消、恢复与用户回显契约；Resume 不接管 ordinary pending/recovery，也不创建 ordinary user 行。历史末尾候选从 composer 使用的当前历史投影与队列恢复路径刷新，不能取分页内旧行；未接入 composer 的历史 fork 投影不在本卡扩展。

## 6. 验证与交付

- 按 `AGENTS.md`：本地不运行 Vega、不编译、不测试、不打包，仅做静态与格式检查。
- 自动回归交云端 PR Check；原生观感、按钮态切换与键盘体验交用户手测。
- 代码改动走 worktree + PR，单一实现写入者；合并前确认最新 head 的 required check。
- 合并不等于已安装，不操作用户的生产数据库或运行中应用。
