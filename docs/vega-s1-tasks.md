# ✦ Vega — S1 任务卡（Sprint 1 · 脚手架 & 外壳骨架 · W1-2）

**版本** v0.3 · 2026-08-29 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt
**S1 目标**（phase1-plan）：workspace 可编译运行、本地门禁绿、bench 骨架可报数、schema/keychain 落地、主题 token 就位。
> v0.2 变更（2026-08-29，人类决策）：T03 由 GitHub Actions 云端 CI 改为**本地 Git Hooks 质量门禁**（防 macOS runner 费用；产品稳定后再评估上云）；DoD 对应调整。
> v0.3 变更（2026-08-29，人类批准）：T02 GPUI 依赖来源改为 **zed 官方仓库 git rev 锁定**（`gpui_platform` 无 crates.io 发布版，详见 phase1-plan E1 修订）。

---

## 执行者 Prompt 模板（派活时复制，替换 ⬇️ 部分）

```
你是 Vega 项目的执行工程师。先完整阅读：
1. docs/vega-exec-guide.md（最高行为准则，红线必须遵守）
2. 本任务卡（下方全文）
3. 任务卡"参考"列出的 spec 章节与文件

项目仓库：<REPO 路径>。规范：spec 之外零发挥；遇阻按 §6 上报协议停止并报告；
unwrap/expect 禁止出现在非测试代码；验收命令全绿才算完成。

【任务卡】
<粘贴下方某一张 T 卡全文>

完成后输出：① 变更文件清单 ② 每条验收命令的原始输出 ③ 与 spec 的偏离（必须为无）。
```

---

## T01 · 仓库脚手架

- **功能点**：A1-01 前置 · **前置**：无
- **目标**：可 `cargo build` 的空 workspace，crate 结构与 tech-spec §1 一致
- **产出**：
  - `Cargo.toml`（workspace members：`crates/{vega,vega_ui,vega_conversation,vega_runtime,vega_tools,vega_markdown,vega_store,vega_token,vega_theme}`, `xtask`）
  - `rust-toolchain.toml`（stable 锁定当前最新版）
  - 每个 crate 的最小 `Cargo.toml` + `src/lib.rs`（含一句 doc comment 说明职责）；`crates/vega/src/main.rs` 打印 "vega boot" 并退出
  - `.gitignore`（target/、.DS_Store、*.log）
  - `rustfmt.toml`、`clippy.toml`（默认即可）
  - workspace 级 `[workspace.dependencies]`：把 exec-guide §5 白名单基础项统一声明（版本锁定，crate 内用 `workspace = true` 引用）
- **验收**：
  - `cargo build --workspace` 成功
  - `cargo fmt --all -- --check` 通过
  - `cargo clippy --all-targets -- -D warnings` 通过
  - `cargo tree | grep -c gpui` = 0（本卡不引入 gpui）
- **禁区**：不实现任何业务逻辑；不添加白名单外依赖

## T02 · GPUI 空窗口（A1-01）

- **前置**：T01 · **参考**：tech-spec §1；zed 仓库该 rev 下 `crates/gpui/examples/` 与 `crates/gpui_platform/src/`（入口/open_window/WindowOptions 实际用法）；ui-spec §1
- **目标**：Metal 渲染的原生窗口，标题 "Vega"，最小尺寸 960×600，深色背景填充（token 值）
- **产出**：
  - workspace `[workspace.dependencies]` 增加 `gpui` + `gpui_platform`：git 依赖 `https://github.com/zed-industries/zed`，**rev 锁定**（见 phase1-plan E1 修订），`gpui_platform` 带 `features = ["font-kit"]`（不开 runtime_shaders）；`crates/vega` 引 gpui + gpui_platform、`crates/vega_theme` 引 gpui（均 `workspace = true`）；Cargo.lock 提交
  - `main.rs`：gpui 应用入口（以该 rev 实际 API 为准——先读该 rev 的示例代码再写）打开主窗口，渲染一个居中文本 "✦ Vega"
  - `crates/vega_theme/src/lib.rs`：定义 ui-spec §2 全部色彩 token 的结构体（Light/Dark 双套常量），本卡先用 `bg-base` 填窗口
- **验收**：
  - `cargo run -p vega` 窗口打开、可关闭、Cmd+Q 退出
  - `cargo tree | grep gpui` 仅显示单一 gpui 来源（同一 git rev，无第二个发行版）
  - `rg "#[0-9a-fA-F]{6}" crates/ --glob '!vega_theme'` 零命中（色值只许在 theme crate）
- **禁区**：不引入任何第三方 gpui 发行版（gpui-box/gpui-standalone/unofficial 等）；不写布局组件
> v0.3 修订（2026-08-29，人类批准）：`gpui_platform` 无 crates.io 发布版，依赖来源改为 zed 官方仓库 git rev 锁定（见 phase1-plan E1 修订）。

## T03 · 本地质量门禁（Git Hooks）

- **前置**：T01 · **参考**：phase1-plan §3.5（修订注）；exec-guide §7（验收协议）
- **目标**：无云端依赖的本地质量门禁——git hooks 在 commit/push 时强制执行验收底线四条（2026-08-29 决策：暂不上 GitHub Actions，防 macOS runner 费用；产品稳定后再按 phase1-plan §3.5 原案上云）
- **产出**：
  - `.githooks/pre-commit`：`cargo fmt --all -- --check`（秒级快检查）
  - `.githooks/pre-push`：`cargo clippy --all-targets -- -D warnings` → `cargo test --workspace` → `cargo build --workspace`（push 前全量门禁）
  - 安装机制：README「开发」节写明一次性执行 `git config core.hooksPath .githooks`；未安装时无提示风险写明（本地纪律 + 架构师验收兜底）
  - README 更新：hooks 安装步骤 + 前置环境说明（完整 Xcode——Metal 着色器编译所需、Rust 工具链、gpui git 依赖首次拉取耗时提示；支撑 DoD「新机器 5 分钟跑通」）
- **验收**：
  - 安装 hooks 后，故意引入未格式化代码 → `git commit` 被拒绝；修复后可提交
  - 故意引入 clippy warning → `git push` 被拒绝；修复后可推送
  - `git config core.hooksPath .githooks` 后 hooks 生效（可用临时提交验证后还原）
- **禁区**：不创建 `.github/workflows/`；不做云端 CD/notarize（Phase 5 前重新评估）

## T04 · xtask bench 骨架（E4）

- **前置**：T01 T02 · **参考**：phase1-plan §3 / ui-spec §5（P1-P8 指标）；risks #5
- **目标**：`cargo xtask bench` 输出三类基准的占位测量框架
- **产出**：
  - `xtask/src/main.rs`：clap 或手写 arg 解析，子命令 `bench`
  - `cold_start`：spawn 5 次 vega 进程测到首帧时间，报 p50/p99（允许本卡先测到进程退出，标注 TODO 接首帧埋点）
  - `memory_idle`：启动 5s 后读 `mach_task_basic_info` RSS（可用 `sysinfo`——**需批准加入白名单**）
  - `render_frame`：占位（输出 "not implemented"，S3 接 `#[gpui::test]` 帧计时）
  - 输出格式：table + JSON（`bench/results/<ts>.json`），供 CI 趋势对比
- **验收**：`cargo xtask bench` 退出码 0，产出 JSON 含三个指标键
- **禁区**：本卡不追求指标达标，只要测量管线通

## T05 · SQLite schema v1 + 迁移（A11-01）

- **前置**：T01 · **参考**：tech-spec §2（DDL 全文）
- **产出**：
  - `crates/vega_store/migrations/0001_init.sql`：tech-spec §2 六表 DDL 原样落地（含索引）
  - `crates/vega_store/src/lib.rs`：
    - `Store::open(path) -> Result<Self>`：`PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;`
    - `migrate()`：读 `PRAGMA user_version`，按序执行 migrations/*.sql，事务包裹，失败回滚
    - `Store` 持有单连接；公开 `conn()` 仅供其他 crate 的 actor 使用（doc comment 注明"禁在 select! 分支使用"）
  - 测试（tempfile）：建库→六表存在→重复 migrate 幂等→user_version=1
- **验收**：`cargo test -p vega_store` 全绿
- **禁区**：不改 DDL 任何字段（spec 定稿）；不引入 sqlx/diesel

## T06 · 配置与 Keychain（A1-10 / A11-05）

- **前置**：T01 · **参考**：tech-spec §6
- **产出**：
  - `crates/vega_store/src/config.rs`：
    - `AppConfig { providers: Vec<ProviderConfig>, defaults: Defaults, ui: UiPrefs }`（serde，TOML）
    - `ProviderConfig { name, base_url, models: Vec<String>, key_ref: String }`（**key_ref 只是引用名，不是 key**）
    - `load()/save()`：路径 `~/.vega/config.toml`，不存在则生成默认模板
  - `crates/vega_store/src/keystore.rs`：`set_key(ref_name, secret)` / `get_key(ref_name)` / `delete_key(ref_name)`，用 `keyring` crate（service = `ai.vega`）
  - 测试：config round-trip；keychain 测试用 `keyring` 的 mock keystore feature
- **验收**：`cargo test -p vega_store` 全绿；`rg "api_key|secret" crates/vega_store/src/config.rs` 无明文字段（只有 key_ref）
- **禁区**：key 永不进 config 文件/日志/错误消息

## T07 · 主题 token + Light/Dark（A1-09）

- **前置**：T02 · **参考**：ui-spec §2 §3
- **产出**：
  - `crates/vega_theme/src/lib.rs`：
    - `ThemeColors`：ui-spec §2 全部 14 个 token（bg_base…code_bg）
    - `Typography`：§3 字号/行高常量
    - `Theme::light() / Theme::dark() / Theme::system()`（读 macOS 外观，GPUI 有对应 API 则用，没有则先默认 light + TODO）
    - `Theme` 注册为 GPUI global，组件经 `cx.theme()` 取用
  - 主窗口背景改为 token 驱动 + 一个临时快捷键（Cmd+Shift+L）切换 light/dark 验证
- **验收**：切换主题窗口背景/文字色即时变化；`rg` 色值硬编码检查仍零命中（theme crate 除外）
- **禁区**：不做组件库，只建 token 机制

## T08 · 设置页骨架（A1-10 UI 部分）

- **前置**：T06 T07 · **参考**：ui-spec §1 §4
- **目标**：Cmd+, 打开设置视图：provider 列表（显示 name/base_url/models）、[+] 添加 provider（表单：name/base_url/key 输入）、默认模型/权限模式下拉
- **产出**：`crates/vega_ui/src/settings.rs` + 主窗口增加"会话/设置"视图切换
  - key 输入框提交时调 `keystore::set_key`，**界面上永不回显**（显示 `•••••••已存储`）
  - 保存 → config.toml 更新；重启后配置恢复
- **验收**：手工走查——添加 provider → 重启 → 配置仍在；keychain 里能查到 key；`cargo test --workspace` 绿
- **禁区**：不做校验连通性（S4 再做）；不做定价表编辑（A1-12 后置）

---

## S1 完成定义（DoD，Sprint 验收）

- [ ] T01-T08 全绿；本地 hooks 门禁在 master 上可用（云端 CI 延后，见 T03 v0.2 变更）
- [ ] `cargo xtask bench` 出数（占位指标允许 not implemented）
- [ ] 新机器 clone → `cargo run -p vega` 5 分钟内跑起来（README 写清 Xcode CLT 前置）
- [ ] exec-guide §3 红线全过（架构师走查）

> S2 任务卡（侧边栏/项目模型/会话列表）在 S1 验收后按同规格拆。每个 Sprint 开工前一周拆下一 Sprint 的卡——不提前全部细化，因为 spec 会随 spike 结论修订。
