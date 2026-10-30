# Shell 实时耗时验证与交付记录

## 范围与用户裁决

核心行为：收起显示安全描述或既有回退；展开在真实管理进程启动后显示实时经过时间；真实终态使用真实结果耗时。

本轮用户选择取消后统一隐藏实时耗时，取代按此前是否真正显示过计时区分的规则。取消不伪造终态，不改变审批、scope、进程取消或回收；UI 只读所属运行的取消状态。标题读取是逻辑排序点，不保证读取后并发取消与本帧像素呈现原子。父子令牌传播沿用运行时语义，后台在下一次轻量观察清理并通知。

集成基线：`ab85d3a427219b2e44ef2d56b6c7c92d199b5bdb`。已保留 #772 历史读取导出与 #771 思考结束后的类别聚合。思考标题优先，实时耗时不能覆盖它。

## 开发期云端证据

开发源码版本：`4abde250e8a4fd62cac5427bc350aae91d495ba8`。

可恢复远端标签：[`evidence/736-shell-clock-tests-4abde250`](https://github.com/puzige/vega/tree/evidence/736-shell-clock-tests-4abde250)。该标签已推送并读回完整 SHA；三份原始测试和测试专用接线均可从这个提交恢复。

[Actions run 41009767610](https://github.com/puzige/vega/actions/runs/41009767610)：Format、Clippy、Nextest 及 required 汇总全部成功。Nextest 原始输出：46 discovered；46 tests run，46 passed，0 skipped。16 个 Shell 执行器回归、6 个 scope/审批回归、24 个进程内 UI 回归。UI 使用令牌层级模拟取消和 GPUI 测试上下文，不代表实际 App 控制器、系统辅助技术、原生像素、焦点或剪贴板手测；Shell/scope 测试在云端使用临时目录和离线 provider，不能称为真实模型验证。

源码 SHA-256：

| 文件 | SHA-256 |
| --- | --- |
| `crates/vega_tools/tests/bash_start_clock.rs` | `929760211aded980bf1737f20bb2114d5c7fc6d2749818a451c16ddcc019547e` |
| `crates/vega_conversation/tests/bash_scope_start_clock.rs` | `16c31d40978b0d88d7e47ccf775dd116798323a9a2ea5880e188ce3ce971b720` |
| `crates/vega_ui/src/conversation_stream/run_activity_tests.rs` | `f6c413d4666bc04996b3f7ea69b7604d464cfb2bf7fe1a0a23cad0a0fb250ba1` |

### 已解释的失败与修正

- [41007440696](https://github.com/puzige/vega/actions/runs/41007440696)，`975be20f55419ee7defb07da5c216cc59c35bda4`：Clippy 成功，46 执行、42 通过、4 失败、0 跳过。失败均为取消清理后通知数量的旧断言。修正为要求恰好一次清理通知，然后新基线下不得有持续计时通知；中性标题、真实 Running、终态和 sibling 断言保留。没有重试换绿或删掉失败用例。
- `d0fabad229c6b19a6dcbe4b28ce8288ab15b0f5e` 未产生 Actions run：主线 #771 导致 PR 合并冲突。解决集成冲突后才继续验证，未将缺少 run 记作通过。
- [41009028212](https://github.com/puzige/vega/actions/runs/41009028212)，`f9561714f24ad76447b9968263d03b3bcfed5db3`：46 执行、45 通过、1 失败、0 跳过，Clippy 成功。唯一旧预期为思考结束后 `Thought`；依据 #771 已合入规格，改为精确的工具类别聚合，并额外检查旧 Shell 刷新一秒后仍不得覆盖聚合。生产代码没有因此改动。

## 独立静态审查

- `975be20f55419ee7defb07da5c216cc59c35bda4`：未发现生产增量回归；指出通知断言与清理通知不一致。静态审查不是执行证据。
- `f9561714f24ad76447b9968263d03b3bcfed5db3`：未发现生产集成回归；确认思考/聚合优先级、图标与必要状态提示保留，指出旧 `Thought` 测试预期。随后仅修测试。
- 最终测试清理增量独立审查：待取得，不把前两次审查当作清理后的完整结论。

## 最终交付清理与验证

已移除三份临时 Rust 测试、UI 测试模块/观测器、`test-support` feature 与构造器、专用 dev dependency；锁文件恢复集成主线版本。没有删除现存 Python 测试或 fixture，没有改变通用 CI 或 benchmark。

本地只执行快速静态检查：`git diff --check`、指定文件 rustfmt、`cargo fmt --all -- --check` 和测试接线清点；均通过。没有本地运行 Vega、编译、Clippy、测试或打包。

最终无 Rust 测试 head 的 required CI：待取得。预期记录为 `no Rust tests`（0 发现、0 执行），不是 Rust 测试通过。真实 UI、系统辅助技术与系统集成仍待用户手测；Issue 不关闭、不置 Done。
