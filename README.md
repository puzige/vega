# ✦ Vega

**Native AI Agent Desktop** — Rust + GPUI 原生 Agent 工作台

> The agent command center that flies: native speed, any model, every token accounted for.

<p>
  <img src="assets/logo/vega-icon-f1-light.svg" width="128" alt="Vega logo">
</p>

Vega 是一个对标 WorkBuddy / Codex Desktop / Antigravity / ZCode 的 AI Agent 工作台，但**不是 Electron 套壳**——用 Rust + GPUI（Zed 的 GPU 框架）+ Metal 原生渲染，万行会话流 120fps，内存仅为 Electron 同类产品的 1/10。

**三根支柱：**

- 🏎️ **原生性能** — GPUI + Metal，冷启动 <50ms，长任务挂一天不膨胀
- 🧠 **Runtime 自主 + 生态开放** — 自研 Vega Runtime（直连模型 API）+ ACP 编排外部 agent（codex / claude-code），不被任何一家绑定
- 💎 **Token 透明 + Harness 质量** — API 调用级真实成本、跨模型性价比、预算硬控制；Agent 行为可审计、可复现、可回滚

## 状态

📋 **S1（脚手架）、S2（侧边栏 & 项目模型）、S3（流式会话渲染）、S4（Runtime 核心）、S5（写工具、权限门禁与三模式）、S6（Diff 审阅 & 产物）已完成并验收；S7（Token 经济：定价目录、API 校准、流式计数与任务成本汇总）已完成自动化/Mock 验收（mock 账单零误差；真实账单 dogfood 为 `real provider/billing pending`）；S8（打磨 & 里程碑：性能埋点真值化、分页水合、Stop/Resume、P0 收口、1000 行重构）已收口为 `engineering fixture passed`——性能 gate（P7 首帧 / P8 空闲 RSS）按 T43 冻结基线如实 `performance gate failed`，与 T44 虚拟化、T48 调优一并 **deferred-to-final-optimization**（期末统一优化批，主人决策 2026-08-31）**。真实账单 <5%、ProMotion 120fps、真实仓库任务、7 天 dogfood 为 `human/hardware pending`，由 T50 人类收口。真实 API/dogfood、人工 UI 与未达性能项边界见各 Sprint 验收报告。全部设计文档在 [`docs/`](docs/)：

| 文档 | 内容 |
|---|---|
| [vega-prd.md](docs/vega-prd.md) | PRD v0.3.3：7 项锁定决策、模块 A1-A12、5 Phase 路线图 |
| [vega-feature-teardown.md](docs/vega-feature-teardown.md) | 五家竞品功能矩阵与取舍依据 |
| [vega-features.md](docs/vega-features.md) | 96 个功能点全表（Phase 1 P0 ×38） |
| [vega-ui-spec.md](docs/vega-ui-spec.md) | UI 规格与可测量验收准线 |
| [vega-tech-spec-p1.md](docs/vega-tech-spec-p1.md) | Phase 1 SDD 技术规格（DDL/trait/状态机） |
| [vega-tech-risks.md](docs/vega-tech-risks.md) | 五大技术难点攻坚方案 |
| [vega-exec-guide.md](docs/vega-exec-guide.md) | 执行宪法（红线/白名单/验收协议） |
| [vega-phase1-plan.md](docs/vega-phase1-plan.md) | Phase 1 八 Sprint 计划 |
| [vega-s1-tasks.md](docs/vega-s1-tasks.md) | S1 任务卡 T01-T08（已完成） |
| [vega-s2-tasks.md](docs/vega-s2-tasks.md) | S2 任务卡 T09-T13（已完成） |
| [vega-s3-tasks.md](docs/vega-s3-tasks.md) | S3 任务卡 T14-T18（流式会话渲染，已完成） |
| [vega-s4-tasks.md](docs/vega-s4-tasks.md) | S4 任务卡 T19-T22（Runtime 核心，已完成） |
| [vega-s4-report.md](docs/vega-s4-report.md) | S4 验收报告（DoD、门禁、红线与偏离） |
| [vega-s5-tasks.md](docs/vega-s5-tasks.md) | S5 任务卡 T23-T29（写工具、权限门禁与三模式，已完成） |
| [vega-s5-report.md](docs/vega-s5-report.md) | S5 验收报告（mock E2E、DoD、UI/性能边界与偏离） |
| [vega-s6-tasks.md](docs/vega-s6-tasks.md) | S6 任务卡 T30-T35（Diff 审阅、产物、Open in、分支与 commit，SDD 已定稿） |
| [vega-s6-report.md](docs/vega-s6-report.md) | S6 验收报告（production E2E、DoD、红线与人工/硬件边界） |
| [vega-s7-tasks.md](docs/vega-s7-tasks.md) | S7 任务卡 T36-T41（Token 经济：定价、校准、流式计数与任务汇总，已完成） |
| [vega-s7-report.md](docs/vega-s7-report.md) | S7 验收报告（mock 账单零误差 E2E、DoD、carryforward 核销与真实账单 KPI 边界） |
| [vega-s8-tasks.md](docs/vega-s8-tasks.md) | S8 任务卡 T42-T50（性能调优、硬件实测与 Phase 1 收口，SDD 已冻结） |
| [vega-s8-sdd.md](docs/vega-s8-sdd.md) | S8 验收 SDD（C1-C8 冻结契约、七状态词表与证据基线） |
| [vega-s8-report.md](docs/vega-s8-report.md) | S8 验收报告（T43 冻结性能基线、逐卡 DoD、carryforward 核销与期末优化批偏离） |
| [vega-packaging.md](docs/vega-packaging.md) | macOS 打包与分发（`cargo xtask package`、.app 结构、其他 Mac 安装与 Gatekeeper、公证 HUMAN 模板） |

## 关键决策（详见 PRD）

- **GUI**：GPUI（Zed 官方仓库 git rev 锁定起步 → vendor fork 演化）· **平台**：macOS First（Metal）
- **形态**：Agent 工作台，**不是 IDE**——代码只做 viewer + diff 审阅，编辑交接外部 IDE
- **执行模型**：自研 Runtime 优先 + ACP 编排并存 · **Remote/SSH**：Phase 4 再说

## 开发

本项目采用 **SDD（Spec-Driven Development）**：spec 先行，代码不允许先于 spec。
参与开发前必读 [`AGENTS.md`](AGENTS.md) 和 [`docs/vega-exec-guide.md`](docs/vega-exec-guide.md)。
验收统一采用 **E2E-first**：真实 production 入口为主证据，安全内核回归为辅；分级与留存规则见 [exec-guide §7](docs/vega-exec-guide.md#7-验收协议每个任务卡通用)。

### 前置要求（macOS）

- **完整 Xcode**：GPUI 编译 Metal 着色器需要 `metal` 工具，仅 Command Line Tools 不够。安装 Xcode 后切换开发目录：

  ```sh
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
  # 若安装的是 Xcode-beta，则对应 /Applications/Xcode-beta.app/Contents/Developer
  ```

- **Rust（rustup）**：通过 [rustup](https://rustup.rs/) 安装；进入仓库后会自动按 [`rust-toolchain.toml`](rust-toolchain.toml) 下载并使用 1.98.0 工具链。

### 安装本地质量门禁（每次新 clone 后执行一次）

```sh
git config core.hooksPath .githooks
```

commit / push 时自动执行验收底线（见 [exec-guide §7](docs/vega-exec-guide.md)）：

| Hook | 检查 |
|---|---|
| `pre-commit` | `cargo fmt --all -- --check`（秒级快检查） |
| `pre-push` | `cargo clippy --all-targets -- -D warnings` → `cargo test --workspace` → `cargo build --workspace` |

**必须手动安装**：git 无法自动强制仓库内的 hooks。未执行上面这条命令时，commit / push 不会做任何检查，也没有任何提示——目前靠本地纪律 + 架构师验收兜底（云端 CI 延后引入，见 [phase1-plan §3.5](docs/vega-phase1-plan.md)）。

### 构建与运行

```sh
cargo run -p vega
```

首次构建会通过 git 依赖拉取 Zed monorepo（约 1–3 GB 进入 `~/.cargo/git` 缓存）并编译 GPUI 依赖链，耗时 10 分钟量级，属正常现象；之后为增量构建。

## License

TBD（私有开发中）
