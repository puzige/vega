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

📋 **S1（脚手架）、S2（侧边栏 & 项目模型）、S3（流式会话渲染）、S4（Runtime 核心）已完成并验收（PR #1-#22）**。全部设计文档在 [`docs/`](docs/)：

| 文档 | 内容 |
|---|---|
| [vega-prd.md](docs/vega-prd.md) | PRD v0.3.1：7 项锁定决策、模块 A1-A12、5 Phase 路线图 |
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

## 关键决策（详见 PRD）

- **GUI**：GPUI（Zed 官方仓库 git rev 锁定起步 → vendor fork 演化）· **平台**：macOS First（Metal）
- **形态**：Agent 工作台，**不是 IDE**——代码只做 viewer + diff 审阅，编辑交接外部 IDE
- **执行模型**：自研 Runtime 优先 + ACP 编排并存 · **Remote/SSH**：Phase 4 再说

## 开发

本项目采用 **SDD（Spec-Driven Development）**：spec 先行，代码不允许先于 spec。
参与开发前必读 [`AGENTS.md`](AGENTS.md) 和 [`docs/vega-exec-guide.md`](docs/vega-exec-guide.md)。

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
