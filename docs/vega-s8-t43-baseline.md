# ✦ Vega — S8-T43 冻结基线（P7/P8/P2/P1 · A2-04）

**状态**：基线冻结（本 PR `feat/s8-t43-instrumentation`）· 证据 cutoff：UTC `2026-08-31T12:35:36Z` / local `2026-08-31T20:35:36+0:800`
**契约**：[vega-s8-sdd.md](vega-s8-sdd.md) C1/C2/C3/C6（T42 冻结，本文只消费，不改判据）
**状态词汇使用**：P7 `performance gate failed`；P8 `performance gate failed`；P2 `engineering fixture passed`（10s 短跑，非终证）；P1 `hardware pending`。无漂白、无同义替换（SDD §1）。

---

## 1. 与 T41/S7 的基线衔接（有效性说明）

- **T41 是 docs-only 卡**（S7 收口报告，`docs/vega-s7-report.md`），对生产代码与 xtask 埋点零改动。因此 **当前 `master` `852e52d`（S7 终态 + T46 合并）的生产代码即 S7 终态，T43 在其上重测的基线有效**，无需等待其他卡。
- T46（A2-17 Stop/repair/Resume E2E，#47）在 T43 采样前合并；它新增的是测试与 E2E 覆盖，不改变本文测量的启动/空闲/流式路径。若 T48/T49 认定该合并影响基线路径，以 T43 冻结 schema 原样复测（schema 不变）。
- 历史 S6 数字（108.7MB、`spawn_to_exit` 冷启动、硬编码 60Hz、~500δ/s 流）全部 **noncomparable**（SDD §0 已核实其测量失真），本文不再引用其任何值作对照。

## 2. 测量命令与环境（C3 provenance）

命令（在 worktree `/Users/peanut996/Workspace/vega-s8-t43`，分支 `feat/s8-t43-instrumentation`）：

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo xtask bench          # 全量：C1+C2 20 进程 → C6 P2 10s@1000δ/s → C6 P1 margin
# 分场景复核：cargo xtask bench-p7（C1+C2）/ bench-p2（P2）
```

- **provenance（JSON 原样）**：git HEAD `aa2d52d694043cb27f6b5f2a23a7857e5c14ecaf`（git_dirty=false）；release profile；构建命令 `cargo build --release -p xtask -p vega` exit 0；`rustc 1.98.0 (88d9e12ae 2026-08-18)`；OS macOS 27.0；CPU/GPU Apple M4；**显示器 60.0 Hz（CoreGraphics 实测，非硬编码）**；arch arm64。
- **被测二进制**：`target/release/vega` 26,715,776 B，SHA-256 `947310f000d2bf7697ba59c0c121e42ae9cab3573937ce42a4bfa57c0d0294df`；探针子进程 `target/release/xtask` SHA-256 `8f6ff530e67b162c629137f20366496cdea347ef4bed8e5a3222a5ce228a8396`。
- **隔离（C3）**：每轮全新临时 `HOME`（macOS 数据根 `$HOME/Library/Application Support/ai.vega` 随之隔离），预 seed 项目+线程行；子进程 attestation 由父进程机械校验（temp HOME/data_root/provider=none/network=none/keychain=not-exercised/first_frame_source=gpui_next_frame_callback）；零真实 profile/Keychain/provider/network。
- **原始 JSON**（repo-external，不入库）：`/private/tmp/vega-s8-t43-baseline.json`（canonical 40 轮报告快照，SHA-256 `459ebb2dee38a38c7b8178e41bd2d8e2e78378487cd843f9e7fc6561905fce11`）；运行时原生输出目录 `$(TMPDIR)/vega-t43-reports/`（本轮 `1788179736199-baseline.json`，同 hash）。

## 3. P7（C1）首帧可交互 —— `performance gate failed`

指标 `process_start_to_first_rendered_interactive`（SDD §2，schema 永不更名）。20 个全新 release 进程，隔离子进程构建真实根视图（`Sidebar` + 路由 `ConversationStream` + 空 Composer，生产 boot path），pinned GPUI next-frame 回调内 flush 恰一条严格 JSON milestone 后正常退出；父进程 spawn 前取 timestamp，整数微秒。无任何一轮 FAIL（milestone 恰一条、exit=CleanExit、drift 合规）。

| 统计 | 值（µs） | 阈值 | 判定 |
|---|---|---|---|
| p50 | 166,166 | — | |
| **p95** | **1,027,906** | < 50,000 | **FAIL** |
| p99 | 1,273,207 | — | |
| max | 1,273,207 | — | |
| min | 119,488 | — | |

20 样本（µs，升序）：`119488 128259 143102 149804 154952 156216 157025 163386 166065 166166 168382 169923 172076 178673 190049 481272 918992 955005 1027906 1273207`。4 轮长尾 > 480ms（进程冷启动调度/GPU 首次管线建链量级），p50 ≈ 166ms 亦远超 50ms 判据——P7 调优归 T48（对本冻结 schema），本卡不调优。

## 4. P8（C2）release 空闲 RSS —— `performance gate failed`

20 个全新 release 进程，C1 milestone 点后 +5/+10/+15s 各采一次 raw bytes（`proc_pidinfo pti_resident_size`，入库零换算），per-process median，nearest-rank p95。漂移轮（+15s−+5s > 2 MiB）**0**；p95 不在灰区 `[98,000,000, 102,000,000)`，未触发 +20 轮扩展。**单位裁决 OPEN(OWNER: human)**：判据按卡面字面权威 decimal MB `100,000,000` B 执行；双口径并报如下，**裁决 A/B 任一候选下均未达标**（裁决不改变本次判定方向）。

| 口径 | p95 | 阈值 | 判定 |
|---|---|---|---|
| **raw bytes（入库权威）** | **109,084,672** | < 100,000,000 | **FAIL** |
| decimal MB（候选 A） | 109.085 | < 100.000 | FAIL |
| MiB（候选 B 显示） | 104.031 | < 100.000（=104,857,600 B） | FAIL |

20 个 per-process median（B，升序）：`91389952 94502912 94699520 97583104 98156544 99860480 102481920 104398848 106184704 108298240 108331008 108347392 108412928 108429312 108544000 108609536 108658688 108904448 109084672 109084672`。中位 108,314,624 B；min 91,389,952 / max 109,084,672。空闲 RSS 调优（eager/stale view state、队列界）归 T48，判据/schema 不得动。

## 5. P2（C6）流式上屏 —— 10s 短跑 `engineering fixture passed`（非终证）

生产 controller 入口 `ConversationStream::apply_event`（parser/enqueue/build 不替代 receive-to-render）；10 s @ 1,000 deltas/s，恰 10,000 deltas + 1 MessageStarted 全部经 bounded 消费（producer 先计数后发送，`run_completed=true`），每批关联最高 sequence 与首个包含帧。5 分钟 release soak 终证归 T48/T49（同 schema）。

| 统计 | 值（µs） | 阈值 | 判定 |
|---|---|---|---|
| p50 | 6,822 | — | |
| **p99** | **14,406** | < 16,000 | **PASS（短跑口径）** |

1,441 批；`queue_max_depth = 0`（有界）；598 帧；UI 线程零 DB/定价 IO（apply_event 内存态）。注意：p99 短跑通过 ≠ 5 分钟 soak 终证通过，状态词不升级。

## 6. P1（C6）万行滚动 margin —— `hardware pending`

生产 S3-T17 render probe（`vega --vega-bench-render`，生产代码零改动），11,451 行合成文档；**真实刷新率 CoreGraphics 实测 60.0 Hz**（硬编码 60Hz 已消除）。字面 120fps 判定需 ≥120Hz provenance（T50 ProMotion），60Hz 主机只证 CPU/build margin：

| 字段 | 值 |
|---|---|
| frame_build p50 / p99 | 6.541 µs / 25.833 µs（预算 8,333 µs @120Hz；margin 达标） |
| fps per second（vsync 截断） | 61,60,60,60,48,61,…,34（median 61） |
| any_second_meets_fps_floor | `null`（<120Hz 不可判，不写 false 不写 true） |
| frozen remat | 0 |
| 判定 | `hardware pending`（`literal_120fps = hardware pending (host below 120 Hz; literal 120fps is T50)`） |

## 7. 新旧埋点对照（失真消除记录）

| 项 | 旧（`b96fcef` 核实，SDD §0） | 新（本卡冻结） |
|---|---|---|
| 冷启动 | `spawn_to_exit`（进程退出 ≠ 首帧） | `process_start_to_first_rendered_interactive`：隔离子进程真实根视图 + next-frame 恰一条严格 JSON milestone；20 进程 nearest-rank p95；九类 FAIL 机械判定 |
| 内存 | `memory_idle` 单次 debug 5s 采样；`rss_mb` 实为 bytes/2^20（MiB 冒充 MB） | C2：release 20 进程 +5/+10/+15s median p95；raw bytes 入库（`pti_resident_size`），MB/MiB 仅显示字段（`rss_mb` 命名已清除）；灰区合并 + 2MiB 漂移护栏 |
| 渲染刷新率 | 硬编码 60Hz | CoreGraphics 实测（`display_refresh_hz`）；60Hz 只报 margin + `hardware pending` |
| 流式 | ~500 deltas/s、parser 级 | 生产 controller 入口 timestamp；1,000 δ/s；每批关联最高 sequence 与首个包含帧；10s 短跑（soak 归 T48/T49） |
| 二进制来源 | 接受既有 target 二进制（文件存在 ≠ provenance） | 无条件 release 重建 + 双二进制 SHA-256/size/mtime + rustc/OS/CPU/GPU/Hz provenance + 本地/UTC cutoff + 结果文件 SHA-256 |
| 输出 | 入 repo | repo-external `/tmp`（产物不入库），status word 如实（`performance gate failed` 不漂白） |

## 8. 冻结 schema（T48/T49/T50 只消费）

- schema tag `vega-s8-t43.baseline.v1`；字段名/单位/轮次/percentile 数学（nearest-rank `ceil(pct×n)`，整数域）/threshold 全部写死于 `xtask/src/contract.rs` + `xtask/src/report.rs`，单元测试 pin 死行为。
- `P8_THRESHOLD_BYTES = 100_000_000`（OPEN(OWNER: human)：人类裁决若改 `104,857,600` 须在 schema 冻结前经 docs 勘误落入该常量；**测后永不换单位**）。
- 状态词汇常量（SDD §1）逐字冻结于 `contract.rs`。

## 9. 偏离清单（如实记录）

1. **探针子进程 = release `xtask` 二进制（`__probe` re-exec），非 `vega` 主二进制**：生产 `vega` 无 milestone seam，本卡红线禁止改 crates/。子进程以生产 boot path 组合真实 `vega_ui`/`vega_theme` 组件（非空壳、非 test-only 视图），`VegaWindow` 外壳（agent/commit/branch controllers）不在空闲首帧场景内。消除该偏离需要上游 seam（T48+ 事项）。
2. **P2 短跑（10s）非 5 分钟 soak 终证**：契约允许的日常 PR 反馈口径；终证归 T48/T49。
3. **P1 场景沿用 S3-T17 行模型 fixture**（11,451 行等高列表）：C6 要求的 10k 语义项变高场景归 T44；本卡交付的是真实刷新率检测 + margin/hardware-pending 判定框架。
4. **首轮长尾干扰记录**：bench-p7 试运行（cutoff `2026-08-31T11:46:07Z`，report SHA-256 `8bd328106b4ec392bee09dfb019cc187e2d1bd139db338005f57995b9ccfb921`）曾出现单轮 1,693,149 µs 长尾；canonical 40 轮报告为本文 §3-§6 数字。两次运行方向一致（P7/P8 FAIL、P2 PASS），门禁判据不受影响。
5. **P2 计数器加固**（`run_completed` 字段 + producer 先计数后发送 + 饱和队列深）：首次实现存在 produced/drained 计数口径差（TextDelta vs 全事件）导致 queue depth 下溢；已修复并以 `run_completed=true` 短跑复核。该字段为 schema 一部分，T48 只消费。
