# 开发验证记录

## 范围与版本

- 基线：`0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`。
- 分支：`feat/921-full-access-background`；本记录随开发实现提交，精确版本以该提交完整 SHA 为准。
- 非独占 Full Access 后台、沙箱双向交界、分类持久恢复、并行 run 清理、原 Shell 卡取消/重试；不推进 #917，不安装或运行应用。
- `git fetch origin` 后基线与 origin/master 相同（0/0）。无新增依赖、workflow 或机器配置修改。

## 本地验证

工具链：官方 `cargo 1.98.0 (797e8a9bc 2026-08-05)`；`rustc 1.98.0 (88d9e12ae 2026-08-18)`，aarch64-apple-darwin，LLVM 22.1.8。命令使用 `CARGO_FORK_AUTO_UPDATE=0 rustup run 1.98.0 cargo`，构建并发 2。

- `cargo test -p vega_runtime -p vega_store shell_concurrency_tests --offline --locked -j 2`：4 发现、4 执行、4 通过、0 失败/忽略/过滤；8.18 秒编译，runtime 执行不足 0.01 秒，store 0.25 秒。
- 回归涵盖：两种到达顺序、保留沙箱 blocker、Full Access 多槽和容量耗尽不锁编辑、真实资源未关闭继续占槽、身份/ReadOnly/关闭后准入、不可变分类、精确终态读回、清理观察幂等及退出事实保留、重启控制失效无写 blocker/沙箱恢复版本变更。
- `cargo clippy --workspace --all-targets -j 2 --offline --locked -- -D warnings`：通过，最新增量 7.45 秒。
- `cargo fmt --all -- --check`、`git diff --check`：通过。
- `python3 scripts/release_tests.py`：5 执行通过；`python3 scripts/prepare_computer_use_tests.py`：15 执行通过；`python3 -m unittest discover -s scripts/tests -v`：40 执行通过。

## 限制与后续门禁

- 上述 runtime 回归不启动真实命令，不是系统进程/后代清理或应用运行证据。
- Stop 并行清理、兄弟任务局部未知、原卡取消/重试、权限变更及真实生命周期仍需独立静态审查；真实系统/原生 UI 集成由用户手测。未将发送信号当作完整进程退出证明。
- 本版本独立审查、开发 CI 尚待取得。Rust 开发测试暂留分支；保留可访问完整源码和实际证据后再独立清理，最终 head 必须经 required CI 确认 Rust 0 发现/0 执行。
- 未合并、未发布、未安装、未用户验收。Issue 保持 In progress。
