# 会话式 Shell 实施检查点（未完成，禁止合并）

## 状态

用户已确认最小设计并授权实施；原设计中“待确认/仅设计”的阶段描述不是当前授权状态。行为合同仍采用设计提交 `f28d8e2ab21869a71f30a7ed0dc959fb196d8035` 的 [规格](vega-long-command-waiting-design.md)，本记录不放宽验收或安全边界。

本检查点保存未完成的实现，不是可用功能，不是独立审查批准，不是 CI 通过。未建立实现 PR、未合并、未发布或修改用户数据库。分支保持 `feat/412-shell-design`。

## 已写入但未验证的产物

- `vega_tools`：显式 wait_ms、进程 owner、输出窗口/cursor、观察与取消；保留缺省阻塞协议。
- `vega_runtime`：wait/cancel schema 与路由、句柄注册表、工作区互斥、持久化 hook、run 收尾。
- `vega_store` / `vega_conversation`：追加 operation 迁移、审计身份、恢复未知结果与权限接线。
- 应用关闭 barrier 接线；开发辅助回归源码共 11 个测试入口（静态数，不是发现或执行数）。

待完成：UI/历史真实状态投影、scoped 流式脱敏、steering 后审计身份、启动清理失败隔离、生命周期/持久化故障回归、格式整理、云端编译和验证、完整非实现者独立审查。开发 Rust 测试必须在实际验证和固定证据后依仓库规则清理，不能现在删掉以规避失败。

## 阻塞：规格 §5.3 进程控制启用前提未满足

以下为静态证据，不是已执行的逃逸试验：

1. `sandbox.rs` 的 BASE_PROFILE 允许 process-fork、process-exec 及 self process-info-setcontrol；启动通过 process_group(0) 创建原进程组。现有代码未提供后代不可脱离原进程组的保证。
2. `bash/mod.rs` 的 settle_exited_group / confirm_group_stopped / wait_group_gone 只确认原 PGID；会话退出、取消或超时路径据此可能得到 cleanup_safe=true。子进程回收和 reader 排空不能证明已关闭输出并离开原组的后代不再写工作区。
3. `agent/shell_sessions.rs` 的 process_prerequisite 只检查 `/bin/kill`、`sandbox-exec` 可执行和当前 PID 的 kill -0，不能建立支持命令的后代收尾保证；Reservation::finish 在 cleanup_safe=true 后释放互斥。
4. 因此，现有代码不能支持“安全收尾后自动放行冲突写入”的启用结论。仅延长等待、按 PID 重绑、提示模型别后台化、或把进程组消失等同全部后代退出均不是修复。

依冻结规格 §5.3，不能满足支持命令的安全收尾时不得开放会话模式。当前没有可在既定边界内证明满足该前提的机制；新增 daemon、全系统进程管理或缩窄支持命令集合均不能擅自纳入本次实现。需要先明确平台安全机制与允许的验证路径，必要的产品/范围变化须用户裁决。恢复前保留检查点，不安装/启用它。

## 实际验证及版本身份

- 已 fetch；ff-only 更新结果为 Already up to date。设计 HEAD 为上述完整 SHA，远端 master 为 `037031f3352a87fe8f0e546aa652e1251d39f8a1`。
- `git diff --check` 退出 0。
- 只读 rustfmt --check 报告格式差异，未通过；输出被截取，不能以管道最后命令的退出 0 冒充格式通过。
- 未本地编译、Clippy、运行 Vega 或执行测试；未取得云端编译/测试结果。
- 新的独立安全核查代理启动失败，没有审查结论；不计作独立审查。进程 owner 补充阶段同样失败且无未知副作用记录，原实现保留；无活跃写入者。
- 文档写入前的完整未提交实现快照摘要（tracked binary diff 加按 git ls-files 顺序拼接的 untracked 文件）为 SHA-256 `0fe7584483feba45711eab5dae677c03f71df0cc206fcb05eb9504f8a5a01e71`。它不是 Git commit，也不是远端可访问证明。
- 关键文件 SHA-256：
  - `crates/vega_tools/src/bash/mod.rs`: `0d520a372fc1c33d588bb4b70c5b9c3b8f91329c54b976ad3f21e7d7caa3f875`
  - `crates/vega_tools/src/bash/session.rs`: `8ca44a8daf7008fabc82c3239351d04f8b791191a94293d51182549915e8d2ba`
  - `crates/vega_tools/src/sandbox.rs`: `54fbfe3cf018d20689e48c35b31f768b46bfdd7e2b29a7630a12214f43700256`
  - `crates/vega_runtime/src/agent/shell_sessions.rs`: `dfced68c9c9b3ee9c3cd2cd51fe9c5e1fb10be493b85ab035cfb4cfac1e07411`

## 恢复条件

先解决上述平台安全前提，保持不确定结果隔离与“不重放”合同；不能仅靠普通 CI 绿色认定 OS 边界已证明。批准的边界及验证路径确定后，从本检查点继续，不重建或丢弃现有产物。随后补完集成和必要回归，在 Actions 执行开发验证，固定源码与实际结果；完成非实现者独立审查、测试清理及最终 head required CI 后才可合并。
