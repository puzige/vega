# 追加消息凭据事务边界验证

## 实现与审查

- 开发提交：`7bd0598667bf84b2bccd4a62e386786f883cc55a`，基线 `80eaba474df5a23c3a7849e11b39a0864b7e0ad6`；[PR #583](https://github.com/puzige/vega/pull/583)，[Issue #581](https://github.com/puzige/vega/issues/581)。
- 生产仅两个文件，12 行新增 / 4 行删除；完整候选在锁外准备，短事务内重验后领取。无新依赖、schema、UI、协调器或模型/工具执行修改。
- 专职实现者完成后停写；非实现者独立审查上述完整 SHA，覆盖两生产文件、完整开发回归、spec、调用接线和安全边界，未发现阻塞问题。静态审查不等于行为验证。
- 本地 `cargo fmt --all -- --check`、`git diff --check` 均退出 0。未本地编译、执行 Vega 或运行任何测试；未修改已安装应用、凭据或 live DB。

## 开发回归来源

- [原始测试源码](https://github.com/puzige/vega/blob/7bd0598667bf84b2bccd4a62e386786f883cc55a/crates/vega_conversation/tests/steering_credentials.rs)，SHA-256 `4167383f9c52087ffd71664d5e87804b24758ac729aa5986aec9791cd80a73bd`。
- 10 个 Rust 开发测试：真实 authority 刷新（未变 / 轮换）、精确脱敏及例外失效、单次领取、10 种候选变化、真实双消费者竞争、空队列 / 入口取消 / 准备取消 / SQLite BEGIN 等待取消、真实凭据缺失、引用 / 图片 / 序号、无效引用 / 图片。
- 队列变化通过受控回调发生于准备和 claim 之间；双消费者用同步信号确认都已准备；SQLite 等待取消通过现有无内容诊断观测取得许可后取消。没有模型请求、网络服务或外部应用替身验证声称，没有改变生产 API 或 CI。
- 等待进程写许可后取消由事务内同一检查的静态审查覆盖；云端等待用例实际覆盖 SQLite BEGIN 等待。取消与 commit 不具原子同步保证。

## 云端记录

开发 head [Actions 39994860416](https://github.com/puzige/vega/actions/runs/39994860416) 的 Nextest、Clippy 和 `check (fmt, clippy, test)` 全部 SUCCESS：Rust 10 发现 / 10 执行 / 10 通过 / 0 失败 / 0 跳过，Nextest Summary 实际执行耗时 1.864 秒；Python 5 + 15 + 40 = 60 个全部通过。云端执行现有完整 `pr-check.yml`，未更改命令或断言。

[开发证据](https://github.com/puzige/vega/pull/583#issuecomment-6370846613) 已写入并读回。GitHub Contents API 确认原始测试 Git blob `5663c097fe1264bbe5404115b0c5559c3617d24f` 与本地相同；完整开发提交的 [patch](https://github.com/puzige/vega/commit/7bd0598667bf84b2bccd4a62e386786f883cc55a.patch) 可恢复。仅在上述成功及证据固定之后移除开发测试；无新增测试依赖或生产接线需要保留。

最终候选保留相同生产补丁，移除唯一开发测试文件；完整 required CI 另行运行，待取得 Rust 0 发现 / 0 执行的结果。最终门禁结果以 PR 对应精确 head 的回执为准，空集不能替代上述 10 个回归的实际成功。

真实安装版本交互由用户手测；CI、合并与应用已安装分别记录。
