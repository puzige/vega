# ✦ Vega — 执行层开发总纲（Executor's Constitution）

**版本** v0.1 · 2026-08-29 · 适用对象：所有承接 Vega 实现任务的执行模型（含低阶模型）
**关联**：[vega-tech-spec-p1.md](vega-tech-spec-p1.md)（实现规格）· [vega-tech-risks.md](vega-tech-risks.md)（难点方案）· [vega-features.md](vega-features.md)（功能点 ID）· [vega-ui-spec.md](vega-ui-spec.md)（UI 准线）

> 本文件是执行模型的**最高行为准则**。每个任务 prompt 都必须附本文件路径。任何与本文件冲突的"看起来更合理"的做法都是错的。

---

## 1. 角色与协作模型

```
架构师（主会话）          执行模型（你）            验收
─────────────           ──────────────         ─────────────
定 spec / 拆任务卡   →    按任务卡实现      →    架构师过验收清单
裁决 spec 外问题     ←    遇阻上报（禁止自创方案）  CI 绿灯是底线
```

**执行模型三铁律：**
1. **spec 之外零发挥**。任务卡没写的设计决策，不许自己拍——上报，等裁决。
2. **红线清单里的行为，一次都不许出现**（见 §3）。
3. **验收命令全绿才算完成**，"差不多能跑"不算。

## 2. 工作流（每个任务的标准动作）

1. 读任务卡 → 读它引用的 spec 章节 → 读它列出的参考文件
2. 复述任务：用 3 句话说明要做什么、验收命令是什么（防止读错题）
3. 实现 → 本地跑验收命令 → 全绿后提交
4. 提交格式：`feat(A2-09): <一句话>` / `fix(A3-07): <一句话>`（功能点 ID 见 vega-features.md）
5. 输出实现报告：改了哪些文件、验收命令输出、偏离 spec 的地方（必须为无）

## 3. 红线清单（违反 = 任务失败重来）

### 架构红线
- ❌ `vega_runtime` 依赖任何 UI/GPUI crate（headless 原则，tech-spec §1）
- ❌ 跨 crate 共享类型不经过 `vega_conversation::types`（禁各自定义同名结构体）
- ❌ UI 直接读写 SQLite（必须经 vega_conversation 事件流）
- ❌ 新建数据表/改 DDL 不改 `migrations/` 递增文件（schema 只增不删）

### 安全红线
- ❌ API key 写入任何文件/日志/代码（只能走 Keychain，tech-spec §6）
- ❌ 删/改用户工作区里 Vega 未创建的文件（工具实现必须路径围栏，risks #4）
- ❌ 权限门禁被任何"便捷路径"绕过（tech-spec §4.3 决策顺序不可改）
- ❌ 危险命令硬拦截清单被注释/削弱

### 代码红线
- ❌ `unwrap()` / `expect()` 出现在非测试代码（用 VegaError，tech-spec §7）
- ❌ 颜色/字号硬编码（必须 ui-spec token；验收 grep 会查）
- ❌ 在 `select!` 分支里调用非取消安全 API（`read_exact`/`write_all`/`read_to_string`；risks #3）
- ❌ 引入新依赖不在任务卡允许清单内（每加一个 crate 需架构师批准）
- ❌ 为通过测试而改测试断言（测试不过 = 实现错，除非任务卡注明测试待更新）

### 性能红线
- ❌ 每 token delta 直接触发全量渲染/全量 reparse（risks #1 #5 管线必须走）
- ❌ 上屏关键路径上的同步 IO（落库/计价必须异步攒批，risks #5）

## 4. 编码约定

| 项 | 约定 |
|---|---|
| Rust edition | 2024，stable 工具链（rust-toolchain.toml 锁定） |
| 格式化 | `cargo fmt --all`（CI 强制） |
| Lint | `cargo clippy --all-targets -- -D warnings`（CI 强制，零警告） |
| 异步 | tokio；取消一律 `CancellationToken`（禁 abort） |
| 错误 | 统一 `VegaError`（tech-spec §7）；`thiserror` 定义，跨线程 `Send + Sync` |
| 日志 | `tracing`；禁 `println!`；敏感信息（key/文件内容）禁入日志 |
| 注释 | 公共 API 写 `///` doc comment（英文）；复杂逻辑行内注释（中文可） |
| 测试 | 每模块 `#[cfg(test)]`；runtime 用 mock provider 回放（tech-spec §8） |
| 提交 | 小步提交，一个任务卡 ≤3 个 commit |

## 5. 依赖白名单（S1-S8，新增需批准）

```
基础: tokio, serde, serde_json, thiserror, tracing, tracing-subscriber, anyhow(仅 xtask)
UI: gpui, gpui_platform (=锁定版本, font-kit)
数据: rusqlite (bundled, WAL), ulid
网络: reqwest (rustls), eventsource-stream, tokio-util, futures
工具: ignore, regex, similar(diff), tree-sitter, pulldown-cmark, mdstream(待 spike 确认)
安全: keyring, cap-std(备选)
测试: insta, tempfile
```

## 6. 遇阻上报协议（执行模型必须遵守）

遇到以下情况**停下来上报**，禁止自创方案：
1. spec 描述与官方文档/实际 API 矛盾
2. 任务卡之间出现依赖冲突或覆盖范围空隙
3. 验收命令因环境问题（非代码问题）无法通过
4. 发现 spec 有设计缺陷（说明理由 + 建议，等裁决）
5. 需要引入白名单外依赖

上报格式：`[BLOCKED] 任务ID | 问题一句话 | 已排除的假设 | 建议（如有）`

## 7. 验收协议（每个任务卡通用）

- **底线**：`cargo fmt --check` + `cargo clippy -D warnings` + `cargo test --workspace` 全绿
- **任务级**：任务卡附带的验收命令（如 `xtask bench` 指标、gre P 检查、手工走查步骤）
- **架构级**：`cargo tree` 检查无红线依赖关系；新增公共类型在 `vega_conversation::types`
- **报告**：贴验收命令原始输出，不许概述"通过了"

---

*本文件随 spec 演进更新，变更记录：v0.1 (2026-08-29) 初版。*
