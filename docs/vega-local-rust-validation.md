# 本地 Rust 验证与 Cargo 缓存

## 当前开发流程

当前用户裁决解除 2026-10-13/16 本地执行禁令，适用于主 Agent、子 Agent 与所有 worktree：

**明确验收与风险 → 最小实现 → 本地任务相关编译/测试 → 必要回归与非实现者独立审查 → 开发 Rust 测试留痕并清理 → 最终 head required CI → 合并 → 用户手测。**

本地允许 `cargo check`、Clippy、test / nextest、build；按风险选择，不要求每次全量，也不恢复强制 commit/push hooks。编译耗时或冷缓存本身不是禁令。多 Agent 需协调机器负载，缓存命中不代表 CPU/内存无限；可按负载用 `-j N` 限制当前编译，不把限制写进 CI 或全局配置。

本地通过不替代 required check `check (fmt, clippy, test)`、独立审查或用户手测。正式发布/打包仍走云端；Rust 测试仍只在开发分支辅助验证，合并前清理并保留可恢复源码及实际验证证据。详见 [验证与交付政策](vega-rust-test-delivery-lifecycle.md)。

## huacnlee/cargo 是什么

这是 [Cargo 的第三方 fork](https://huacnlee.github.io/cargo/)，不是测试框架或评审流程。上游说明的主要变化：

- 以 Git common dir 识别项目，将中间产物放入 `~/.cargo/targets/`，复用跨 worktree 依赖与相同内容的 workspace crate；有 build script 或使用自身目录的 crate 可能仍按 worktree 构建。
- 跨项目共享依赖，按版本、features、profile 等编译条件匹配；不同条件不保证复用。
- 自动清理旧构建单元，并限制项目及共享依赖缓存体积；不是无需磁盘空间，也不是永不重编译。
- 细粒度构建锁减少重复编译；**不隔离测试对 HOME、全局 Git 配置、数据库、端口或外部服务的访问**。
- 改变默认 dev 调试信息；nightly 还可使用 Cranelift，tests/release 仍使用 LLVM。Vega 不因采用 fork 而改用 nightly。

上游在 525-crate 项目报告 5 worktree 总构建 8.5 分钟降到 1.6 分钟、33.6 GB 降到 5.1 GB。它们是上游测量，**不是 Vega 的实测收益**；编辑重建也不保证变快。

## 受控启用与兼容性

- 本次选定 `huacnlee/cargo 0.102.0-9`，下载使用该 tag 的安装脚本并校验 release 包 SHA-256。校验和用于校验包与发布方记录一致，不等于独立的供应链审计。
- 安装位置为 `~/.cargo/bin/cargo`，安装器保存此前 Cargo 为 `cargo.rustup`。本机此前实际 PATH 优先选中 Homebrew Cargo，因此需在 shell 路径设置结束后前置 `~/.cargo/bin`，并设置 `CARGO_FORK_AUTO_UPDATE=0`。本次更新登录及交互 zsh 配置；既有终端和已运行 App 的环境不自动刷新，须重开终端/重启相关宿主或在命令中显式设置。
- 先运行 `command -v cargo`、`cargo -V`、`rustc -V`，确认真实工具身份。Vega 的 `rust-toolchain.toml` 继续使用 Rust 1.98.0；fork 的 Cargo 自身版本号不同，不代表已升级 rustc。CI 保持官方工具链和既有 Nextest 版本。
- fork 忽略 `--target-dir`、`CARGO_TARGET_DIR`、`build.target-dir` 和 `build.build-dir`。不能据这些参数假定缓存或输出隔离；需要严格输出目录语义时使用官方 Cargo。
- **不要继续新增共享 `target` symlink**。fork 的中间缓存与每 worktree 的 `./target` 输出分开管理；旧 symlink 会让可执行输出仍指向其他 checkout。本次不批量转换旧 worktree、不删旧缓存；若目标 worktree 已有 symlink 或自定义目录，先确认无运行构建/测试，核对所有权后单独迁移。未迁移时使用官方 Cargo 并协调共享 target 的并发。
- 本次未添加/修改 sccache 配置，不假定历史机器配置仍存在。wrapper、rustflags、profile、build script 和路径依赖都可能影响命中率；不为追求命中率削弱隔离或断言。
- 不自动更新 fork；需要升级时另行授权，记录新版本并重验构建、测试及回退。

## 验证命令示例

在任务 worktree 中先确认工具链，再按实际 crate/行为选择命令；以下不要求每次全部执行：

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_FORK_AUTO_UPDATE=0
command -v cargo
cargo -V
rustc -V
cargo fmt --all -- --check
cargo check -p <affected-crate> --locked -j 2
cargo clippy -p <affected-crate> --all-targets --locked -j 2 -- -D warnings
cargo nextest run -p <affected-crate> -E 'test(<behavior>)' --locked --build-jobs 2
```

Nextest 的 `--build-jobs` 控制编译并发，`-j` / `--test-threads` 控制测试执行并发，两者不能混用；示例不更改默认测试并发。过滤测试前确认目标和发现数量，过滤空集不能冒充行为验证。最终 master 无 Rust 测试的要求不变；`no Rust tests`（0 发现/0 执行）与通过非空测试集分别记录。需要 workspace 或 Python 回归时按任务风险扩大；不新建真实外部系统 E2E 门禁。

交付记录包含内容版本/完整 SHA、工作区是否有未提交改动、实际 Cargo/rustc/Nextest 版本、精确命令、退出码、耗时及发现/执行/失败/跳过数量（未知注明未取得）。保留首次失败和修正原因，不用重跑掩盖原始证据。本地、CI、用户手测分别记录。

## 本次启用验证

基线为 `448a28d27446ef9046b43773d600a7dcf695596b`，本轮仅修改规则、skill 和文档，无 Rust/依赖/CI 变更。

- 新登录和交互 zsh 均解析到 `~/.cargo/bin/cargo`，`CARGO_FORK_AUTO_UPDATE=0`；fork `0.102.0-9`（Cargo `1.101.0 e611c7a87`）。Vega 目录实际 rustc 为 `1.98.0`；`rustup run 1.98.0 cargo -V` 返回官方 Cargo `1.98.0`。
- 隔离的无依赖 Rust 工程：`cargo build --offline -j 2` 成功（0.26 秒），`cargo test --offline -j 2` 1 执行/1 通过，`cargo nextest run --offline -j 2` 1 执行/1 通过/0 跳过。此处 Nextest 的 `-j` 是测试线程，不是编译 jobs。第二 worktree 的 build 0.01 秒，test 1 执行/1 通过，日志指向同一共享产物；这只验证基本复用，不是 Vega benchmark。
- 本机 Nextest `0.9.151`；CI `0.9.146` 不变，不声称二者完全等同。
- Vega 中 `cargo fmt --all -- --check`、`cargo metadata --no-deps --offline --locked --format-version 1`、`cargo check -p vega_credentials --locked --offline -j 2` 均退出 0；后者完成于 15.77 秒。fork 提示现存 workspace dependency `tracing-subscriber` 未使用，未在本任务修改该无关配置。
- 未运行 Vega 全量测试、UI 系统集成、打包或完整本地 Clippy；未测旧 symlink 迁移、并发构建、GC 容量回收和实际卸载。完整 required CI、独立审查和用户手测分别记录在 PR，不用上述 smoke 代替。

## 回退

临时使用官方 Cargo，同时保留项目 pin：

```sh
rustup run 1.98.0 cargo -V
rustup run 1.98.0 cargo check -p <affected-crate> --locked -j 2
```

永久卸载使用已审阅的同版本安装器 `sh install.sh --uninstall`，恢复 `~/.cargo/bin/cargo.rustup`；若要恢复原 Homebrew 优先级，还需撤销本次 shell 中追加的 PATH/自动更新设置。不要删除 `rustup` 本体或手工清空共享缓存。`rustup self update` 可能重新覆盖 cargo proxy，之后须重新确认实际版本，不静默重装或升级 fork。
