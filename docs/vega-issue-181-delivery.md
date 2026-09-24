# Issue #181 交付与手测记录

关联：[实现规格](vega-issue-181-auto-update.md) · [实现卡](https://github.com/puzige/vega/issues/181)。

## 当前证据边界

本轮未新增或运行本地测试（会话约束）。静态审查、编译、云端 check 与真实签名安装分别报告；任何一个不替代其他证据。真实 Developer ID、公证与 UI 手测尚未执行。仓库凭据查询为空，本轮不配置凭据、不发 tag、不更改日常应用。

## 用户手测步骤（合并后）

1. 从固定 `~/Documents/Vega/Vega.app` 打开应用，在设置中找到更新区域，确认版本与实际 bundle 的 `CFBundleShortVersionString` 一致。
2. 关闭自动检查并重新打开应用，确认选项保留；手动检查仍可用。断网时手动检查显示错误与重试入口，不影响现有会话。
3. 当当前版不低于最新 stable 时显示已是最新；当前为 ad-hoc 且存在新版时提供官方发布页入口，不替换应用。开发实例应明确显示开发版本。
4. 首次正式签名包需手动安装。由负责人完整配置发布凭据并发布后续较新版本，再验证自动下载、版本说明、稍后和重启安装。
5. 运行任务或准备启动任务期间，安装应被拒绝；稍后回到更新入口时已准备版本仍可安装。没有任务后主动重启，确认新版本号和原有配置/会话。
6. 使用受控异常包验证摘要、身份、Team ID、版本、签名、公证失败均拒绝；仅在专用副本验证替换失败和启动失败恢复，不对日常应用制造故障。

## 恢复语义

安装在目标 app 的父目录内创建独占 `.vega-update-*` 暂存目录，旧包保存为其内部 `previous.app`。正常启动确认后才清理。启动失败恢复旧包；新版仍存活但未及时确认时不强杀，保留旧包与记录。断电或强杀可能中断两次目录重命名；不宣称跨断电原子性。

若发生中断，先确认没有相关 Vega 进程/任务正在运行，核对暂存目录的 `install.json` 与旧包身份，再将对应 `previous.app` 恢复为原 `Vega.app`。不要批量删除 `.vega-update-*`，不要删除配置或 `ai.vega` 数据目录。恢复后保留该次记录供定位。

## 状态

- 实现：代码完成，主 agent 与独立审查完成。
- 编译：`cargo check -p vega -p xtask --bins`，exit 0，3.41s；日志 SHA-256：`66ab1b97530a826e4f94a7b597e843e2f96e727eb359d0014260659cc0fe4319`。
- 本地测试：NOT RUN。
- 云端 check / PR / merge：待记录。
- 真实签名安装与用户手测：NOT RUN。
- 日常安装：未更新。

## 编译输出（有界原文）

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.41s
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
note: to see what the problems were, use the option `--future-incompat-report`, or run `cargo report future-incompatibilities --id 1`
```

## 已知限制

- 自动安装只允许固定 `~/Documents/Vega/Vega.app`；其他 bundle 保留版本检查与发布页。
- “稍后”保留当前会话中的已下载包，不承诺跨退出恢复下载。普通退出或崩溃可能留下未安装的 `.vega-update-*`；本期不扫描删除历史目录，避免误删仍有 helper/恢复责任的备份。需要清理时逐个核实，不自动批量删除。
- 发布凭据未配置，签名成功路径、Gatekeeper 实机表现与更新后的首次启动均为 NOT RUN。

## 受影响 binary 静态 lint

`cargo clippy -p vega -p xtask --bins -- -D warnings`：exit 0，6.88s。未包含 test targets 或运行测试。日志 SHA-256：`7b1b4cc1ad034a5396eb1871b340d702a2984a352343348032c595b97bebb3b2`。

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.88s
```
