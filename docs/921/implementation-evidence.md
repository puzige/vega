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
- 非实现者静态独审已覆盖本次差异及受影响合同；真实系统/原生 UI 集成仍由用户手测。未将发送信号当作完整进程退出证明。
- 初版 `51c3744a627485ab11f7aa9a3dbb55dde4b0ef2e` 开发 CI [42591766443](https://github.com/puzige/vega/actions/runs/42591766443) 已通过：Clippy job 129074825714、Nextest job 129074825808、required check 全成功；实际 4 执行/4 通过/0 跳过，Python 5/15/40。不是当前修复增量的 CI。
- 初版非实现者独审发现两处 P2：重试清理可能提前释放带未确认终态的 owner；只读轮询可能吞掉原卡取消并清除失败反馈。增量修复：取消前先读回精确原提交，清理后再次确认才释放；卡片取消可替代只读轮询并通过新序号丢弃旧回执；普通投影不清除未解决的控制失败或在途状态。
- 新增原卡纯状态回归已本地执行；`cargo test -p vega_runtime -p vega_store -p vega_ui --lib --offline --locked -j 2` 实际 5 发现/5 执行/5 通过/0 忽略/0 过滤，编译 9.02 秒，执行 runtime <0.01/store 0.25/UI 0.02 秒。增量全 workspace/all-target 严格 Clippy 3.96 秒、fmt/diff 检查通过。原卡测试单独冷编译 2 分 22 秒，执行 1 项通过；没有运行应用。
- 修复版 `176c6778edbc98794a157b82a6948e7e70cfd727` 非实现者增量审查结论：两项 P2 均关闭，无新发现。只读静态审查，不替代真实系统或用户手测。
- 修复版开发 CI [42593773123](https://github.com/puzige/vega/actions/runs/42593773123) 全成功：Nextest job 129081405787、Clippy job 129081406262、required job 129086163472。实际 5 发现/5 执行/5 通过/0 跳过；Python 5/15/40 全通过；Nextest 执行 0.473 秒。
- 可恢复开发源码已固定远端标签 [`shell-921-dev-176c6778`](https://github.com/puzige/vega/tree/shell-921-dev-176c6778)，精确 commit `176c6778edbc98794a157b82a6948e7e70cfd727`，已读回远端 ref 与 GitHub 文件树：runtime 测试 blob `673594e52331df7cd3352b4aab80b193c386b807`；store `ce2bbb58bdb86835192e81b0bef2571f61208c25`；UI `4fe54cb89f95d88abd442077108b97e703b31920`。
- 本次单独清理仅删除上述三份测试及对应 cfg/path/module 接线，不改生产逻辑/安全断言/依赖/CI/Python。清理三处混合文件移除接线后的生产文本与 `176c6778edbc98794a157b82a6948e7e70cfd727` 逐字相同（仅忽略文件末尾空行）；静态扫描 crates 未发现测试属性/cfg(test)，无新依赖或可执行 doctest。清理后 workspace/all-target 严格 Clippy 7.40 秒、fmt/diff 检查通过。最终 head 仍待完整 required CI 确认 Rust 0 发现/0 执行；空集不是行为验证通过。
- 未合并、未发布、未安装、未用户验收。Issue 保持 In progress。
