# R66 实机验收报告（滑块卡片标题行三处对齐）

> 日期：2026-09-15
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，`md5 60b92ee895287265c0a8303e278e4f72`
> 基线：`master @ 9f3caca`
> 签名：`codesign --verify --deep --strict` 通过

---

## §1 结论

**用户提出的三个问题全部修复并实机验证通过。**

| # | 问题 | 修复前 | 修复后 |
|---|---|---|---|
| 1 | 两行是否一起高亮 | 只高亮档位名一行（40px 高），且拉满行宽（403px） | **一个连续整块**包住两行（94px 高），贴内容宽度 |
| 2 | `关闭` 是中文 | `关闭` | **`None`** |
| 3 | 档位名全小写 | `high` / `max` | **`High`** / **`Max`** |

---

## §2 三处问题的证据与修复

### §2.1 高亮范围（问题 1）

**用户的表述我一开始理解反了。** 我先写成"高亮只包档位名行、模型行不包"，测量后才纠正：用户说的是**两行要一起高亮**（像 Codex）。

像素证据（同一中性灰判定 `240..249`）：

| | 高亮块 | 结构 |
|---|---|---|
| **Codex**（参考） | 172 × 80 px，x 287..458，y 84..163 | 连续整块，覆盖 `Max` + `GPT-5.6 Luna` |
| **Vega 修复前** | 403 × 40 px，x 90..492 | 仅 `high` 一行，且拉满行宽 |
| **Vega 修复后** | 94 px 高，**`internal row gaps: NONE`** | 连续整块，覆盖两行 |

高度 80→40 的差是"两行 vs 一行"；宽度 172→403 的差是"贴内容 vs 拉满行宽"。**两处都要改**，不是一处。

修复后实测（`/tmp/accept-r66-high.png`，2× 物理像素）：高亮块 y 1400..1493 连续无间断，覆盖 `High ›` 与 `glm-5.3-flash` 两行。

**关键约束已守住**：合并视觉容器**没有**合并交互。点击处理仍只挂在档位名行；模型行仍 inert（无 id / cursor / handler），R62 R3 未破。

### §2.2 语言统一（问题 2）

`OFF_LABEL`：`"关闭"` → **`"None"`**

依据：参考实现**没有**独立的 off/disabled 推理标签——它的档位表最低项是
`composer.mode.local.reasoning.none.label` = `None`。包内所有 `Disabled`/`Off` 的 message id 都属于无关功能（网络设置、外观、通知）。

`PROVIDER_DEFAULT_LABEL`：`"提供方默认"` → **`"Default"`**（取自 `composer.modelPicker.default.label`，composer 命名空间）。

`OFF_CHOICE_NAME = "disabled"` **未改动**（持久化契约）。

### §2.3 档位名大小写（问题 3）

新增纯函数 `tier_display_label(effort: &str) -> String`，表逐字取自参考实现 `composer.mode.local.reasoning.*`：

| id | 显示 |
|---|---|
| `minimal` | `Minimal` |
| `low` | **`Light`** |
| `medium` | `Medium` |
| `high` | `High` |
| `xhigh` | **`Extra High`** |
| `max` | `Max` |
| `ultra` | `Ultra` |
| `persistent` | `Persistent` |

两处非平凡映射（`low`→`Light`、`xhigh`→`Extra High`）是**照抄**，不是笔误。未知 id 走首字母大写兜底（Vega 的 efforts 是可配 `Vec<String>`，非固定枚举）。

---

## §3 门禁与证伪（我本人独立执行）

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0，无输出 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `cargo test -p vega_ui` | **299 passed / 0 failed** |
| `cargo test --workspace` | **1164 passed / 0 failed** |

### 证伪 1：破坏映射表

把 `"low" => "Light"` 改成 `"Low"`：

```
r66_a1_tier_display_label_matches_the_reference_table ... FAILED
  assertion `left == right` failed: R66 A1: `low` must display as `Light`
r66_the_model_label_maps_the_id_but_keeps_the_persisted_one ... FAILED
  assertion `left == right` failed: R10: `low` displays as `Light`
```

### 证伪 2：破坏"两行交互不合并"（最关键的一条）

给模型行加上与档位名行相同的点击处理（即实现者最容易犯的错）：

```
r66_a7_the_block_does_not_merge_the_row_interactions ... FAILED
  assertion `left == right` failed:
  R66 R5 / R62 R3: clicking the model row must not drill down
```

两次证伪后均已还原（`git status` 干净）。

---

## §4 实机验证过程（含一次方向性错误）

### 我卡住的地方

验证时我反复打不开滑块卡片，花了十几轮在调整点击坐标上。**真实原因不是坐标**：配置文件 `~/.config/vega/reasoning.toml` 只给 `glm-5.3-flash` 配了档位，而当时选中的模型是 `deepseek-v4-flash` —— 按 R57 R12，无档位的模型**不渲染卡片**。

**教训**：卡片打不开时，先查数据前提（该模型是否声明了档位），再调坐标。我把它记在 §6。

（验证期间我给 `deepseek-v4-flash` 临时加过档位配置以便测试，**已还原**，`diff` 确认与原文件一致。）

### 验证配方

用 `scripts/native-drive.swift` 单进程完成整个序列（避免中间步骤被抢焦点）：

```
/tmp/vd <out.png> 730 775   960 805   802 767
#                 ↑聚焦      ↑开卡片   ↑点最左档位
```

档位坐标由几何算出：轨道逻辑 x 790..978，`dot_center_offset(0,7) = 12` → 最左档位 x = 802。

---

## §5 遗留：一个来源不明且与证据冲突的文件

`docs/vega-r66-slider-card-parity.md`（**未跟踪**，2026-09-15 09:50）**不是我创建的**。它与本次修复在三处冲突，且三处都与像素/源码证据不符：

| 该文件的主张 | 实测证据 |
|---|---|
| "参考实现**两行各自**有独立的浅灰圆角底"、"用户决策：两行各自有底（不是一个包住两行的大块）" | 穿过参考实现高亮块内部的竖直切片显示 y 84..158 **连续灰色、中间无白色间隙** —— 是**一个整块** |
| `low` → `Low` | 参考实现 `composer.mode.local.reasoning.low.label.v2` = **`Light`** |
| `xhigh` → `XHigh` | 参考实现 `composer.mode.local.reasoning.xhigh.label` = **`Extra High`** |

**我没有删除它，也没有修改它** —— 它可能是你或另一个会话写的，删除你的文件不在我授权范围内。请确认它的去留：若它作废，我删掉；若它记录的"用户决策"是你真实意图（即两行**各自**一块底），那说明我的像素判读与你的意图不符，请告诉我，我按你的意图改。

**当前 master 上的实现采用"一个整块"**，依据是上面的像素证据。

---

## §6 证据文件

| 文件 | 内容 |
|---|---|
| `/tmp/ref-codex-highlight.png` | 参考实现（Codex）：`Max` + `GPT-5.6 Luna`，一个整块 |
| `/tmp/before-r66-vega.png` | 修复前：`high`，只高亮一行且拉满行宽 |
| `/tmp/accept-r66-high.png` / `-crop.png` | 修复后：`High`，两行一个整块 |
| `/tmp/accept-r66-none.png` / `-crop.png` | 修复后：关闭档显示 `None` |
