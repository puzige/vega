# Shell 修复交付

## 实现前回归设计

- 生成凭据保护 profile：保留 protected root 文件拒绝、其他进程信息拒绝、taskgated 拒绝及启用代理时的 network 限制；仅自身 process-info-pidinfo 例外。
- 使用进程内字节流替身覆盖启动 stderr、命令 stdout、任一流先 EOF、取消、输出上限。
- 使用 ExitStatus::from_raw 覆盖 exit 0、非零 exit、SIGTRAP 与 SIGTERM；仅真实 signal 状态补充终止说明，不把普通 exit 误作信号。
- 所有 Rust 回归待云端 PR Check，不运行本地编译或真实进程自动测试。

## 无害诊断 probe

2026-10-15，本机 sandbox-exec + /bin/zsh -fc 'echo vega-probe'，使用 Python subprocess 捕获退出状态与两条输出流，单次 timeout 3 秒。

| profile 差异 | returncode | stdout | stderr |
|---|---:|---|---|
| allow default | 0 | vega-probe | 空 |
| deny process-info* | -5 | 空 | 空 |
| deny taskgated mach-lookup | 0 | vega-probe | 空 |
| 两项 deny | -5 | 空 | 空 |
| 两项 deny + allow process-info* target self | 0 | vega-probe | 空 |
| deny process-info* + allow process-info-pidinfo target self | 0 | vega-probe | 空 |
| deny process-info* + 仅其他 self 子操作例外（setcontrol/dirtycontrol/codesignature/listpids） | -5 | 空 | 空 |

Python returncode -5 表示 SIGTRAP；Rust 原有 status.code().unwrap_or(-1) 将此映射成 UI 的 exit -1。

## 安全边界无害 probe

Python ctypes 调用 libproc 的 proc_pidinfo(parent, PROC_PIDTBSDINFO=3)，只记录返回字节数与 errno，不输出进程数据。baseline 返回 136 / errno 0；deny process-info* 加自身 pidinfo 例外后返回 0 / errno 1 (EPERM)。

临时创建合成 protected 文件，并用同一 profile 执行 echo 与 cat：echo 成功，cat exit 1，stderr 为 Operation not permitted。未触碰真实凭据。该结果是本机 OS 边界探测，不替代真实 Vega 应用验收。

## 最终实现

- sandbox.rs：仅新增 target self 的 process-info-pidinfo 例外；所有其余保护原样保留。将启动 stderr 接入 pipe。
- bash/mod.rs：stdout 与启动 stderr 进入同一有界 collector，分别处理 EOF 与错误，共享取消与 drain 生命周期；shell 的 exec 2>&1 保留，因此命令主体仍由同一流维持字节顺序。两个独立启动流间不承诺绝对时间顺序。
- build_output：真实 signal 终止时追加 signal 数值诊断，exit_code -1 协议不变，无需存储迁移，也不从历史 -1 推断 signal。诊断额外开销至多 42 字节（换行、固定英文前缀、i32 信号数与尾换行），原 collector 内容限额不变。high_water_bytes 仍仅是原 collector 的计数，不能当作双流总内存上限；另一个固定读取 buffer 为 16 KiB。
- bash/tests.rs 与 sandbox.rs：进程内覆盖 profile 保留、正常/非零/signal、stderr、两流 EOF、取消、每流顺序、共同输出上限、signal 固定额外开销；没有新增真实进程自动测试。

## 验证

- verified_at_utc: 2026-10-15T02:28:08.006780+00:00
- branch: feat/tool-block-codex-parity
- implementation_content_sha256: 63c8d764b8c32b9de0415d85e06006638cadb974b42d9f8f5a864d5b51f48593
- `rustfmt --edition 2024 --check crates/vega_tools/src/bash/mod.rs crates/vega_tools/src/bash/tests.rs crates/vega_tools/src/sandbox.rs`：exit 0，stdout/stderr 为空。
- `git diff --check`：exit 0，stdout/stderr 为空。
- 云端 `cargo nextest run -p vega_tools`、Clippy 与全 workspace pr-check：NOT RUN，待主 agent 提交触发 CI。
- 本地未执行编译、Rust 测试、构建或打包。

## 残余

真实 Vega 应用运行、取消、超时、凭据代理与安装后的界面由用户手测。无新依赖，无 runtime/conversation 接口改动，无 spec 偏离。诊断明确到 signal 数值，不从 SIGTRAP 泛化推断全部失败皆由 sandbox 造成。
