# Bash 内独立 Brave 启动兼容（C2）

## 状态与变更依据

- Issue：<https://github.com/puzige/vega/issues/619>。
- 分析基线：`23cb3a48d5d5c981096ac1a4f6fa6b31e1596177`。
- 用户已批准：在凭证保护 profile 中增加自身 `process-info-setcontrol` 例外，走云端 CI 后由用户真实手测。
- 本文取代 C1（专用外层沙箱 + 私有控制传输 + 派生 agent-browser）。C1 的前提经实测不成立，见下文“方案收敛”。

## 方案收敛：为什么只剩一行

C1 曾假定需要新建专用浏览器能力。2026-10-26 实测推翻了该前提：

| 观察 | 结论 |
|---|---|
| `--args "--no-sandbox"` 下，Brave stderr 不再出现 `sandbox initialization failed` / `Failed to initialize sandbox.` | 关闭 Chromium 内层沙箱确实消除了嵌套 Seatbelt 冲突，无需 pipe、无需派生 AB、无需鉴权桥 |
| 同一进程仍报 `NSInternalInconsistencyException: pthread_setname_np (new) failed`，退出 SIGTRAP | 剩余唯一阻塞是外层 profile 拒绝自身线程控制 |
| 同一边界内自身线程命名探针返回 `EPERM` | 与本机故障一一对应，且可由 profile 直接解释 |
| 普通 Terminal 同版本 AB + Brave 全链路成功 | 差异只在执行边界，不在 AB 或 Brave |

因此本次修复只处理“自身线程控制被拒”，不引入新能力、新依赖或新进程模型。

## 改动

`crates/vega_tools/src/sandbox.rs` 的 `protected_profile`：

```lisp
(deny process-info*)
(allow process-info-pidinfo (target self))
(allow process-info-setcontrol (target self))
```

## 授权边界

- 该规则只允许进程对**自身**执行 setcontrol 类操作，仍保留 `(deny process-info*)`，仍不能查询或控制其他进程。
- 凭证根 `(deny file-read* file-write* (subpath ...))` 与 `(deny mach-lookup (global-name "com.apple.taskgated"))` 不变。
- 代理存在时的 `(deny network*)` 收窄不变。
- 通用 Bash 的 `PROFILE` 写入围栏、`.git` 保护、临时目录策略均不变。
- 不修改系统 TCC、系统代理、日常 Brave 或全局 agent-browser。

## 明确不覆盖的内容

- 本改动不自动给任何浏览器加 `--no-sandbox`。该参数仍须由调用方显式传入，且只应作用于本次独立测试实例。
- 本改动不提供“专用收紧的外层隔离”。当前 Bash 策略允许网络、允许项目写入，不是为不可信网页设计的安全边界。若后续要做专用浏览器能力，仍须独立设计。
- 本改动不解决 agent-browser 本地控制接口（daemon socket、stream listener、CDP 端口）对同机同 UID 进程可达的问题。在本方案下这不是新增缺陷，而是既有 Bash 能力的延续；若将来要求“通用 Bash 不得绕过专用工具控制浏览器”，须另开卡。
- 本改动不验证 helper 进程整树退出，也不改变现有清理语义。

## 实施步骤

1. 修改 `protected_profile`，仅增加一行。
2. 提交到 `feat/619-browser-compat`，推送并等待云端 required CI。
3. CI 通过后由用户在 Vega 内真实手测。
4. 手测通过后回写 Issue、合并、置 In review 收尾。

## 验收

| ID | 必须观察到的结果 | 验证层 |
|---|---|---|
| V1 | `pthread_setname_np` 在 Vega Bash 内不再返回 EPERM | 用户平台手测 |
| V2 | Vega Bash 内独立 session + 全新 Profile 能启动 Brave 并建立 CDP | 用户平台手测 |
| V3 | snapshot 读到真实元素引用 | 用户平台手测 |
| V4 | 点击真实 ref 后 URL 与目标正文均核对通过，空正文不冒充成功 | 用户平台手测 |
| V5 | 仅关闭本次实例，日常 Brave 未受影响 | 用户平台手测 |
| V6 | 凭证根、`.git` 写入围栏、项目外写入拒绝行为不回归 | 用户平台手测 + 云端回归 |
| V7 | 其他进程查询/控制仍被拒绝 | 用户平台手测 |

自动回归仅交 Actions；本地不运行 Vega 代码、编译或测试。真实平台行为由用户手测。

## 回滚

删除该行即可恢复原行为。不涉及数据迁移、依赖或运行时配置。
