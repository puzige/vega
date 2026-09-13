# R58 · 滑块加 Off 档位（规格冻结）

> 状态：**SPEC FROZEN**
> 基线：`master @ 9e41d6a`
> 用户决策（2026-09-14）：**不做重置按钮**；**滑块最左加一个 Off 档位**
> 前置：R57（滑块已实现并接线）

---

## §1 背景：Vega 的 `disabled` 建模与参考实现不同

| | 参考实现 | Vega |
|---|---|---|
| `none` / `off` | `efforts` 序列的**一档**（序列为 `none,minimal,low,...,ultra`） | **独立的** `ReasoningChoice::Disabled`，不在 `efforts` 里 |
| wire 发送 | 作为 effort 值 | 走 `disabled_wire`：`thinking:{type:"disabled"}` 或 `reasoning_effort:"none"` |

**这是必须尊重的差异**：不能为了 UI 方便把 `disabled` 塞进 `efforts`，那会破坏 wire 契约与 `reasoning.toml` schema。

**正确做法**：滑块**视觉上**在最左渲染一个 Off 位置，但选中时映射到 `ReasoningChoice::Disabled`，而不是 `Effort("off")`。

## §2 契约

### R1（必须）Off 是滑块的**第 0 档**，位于最左

档位序列（视觉）：
```
[Off] [档位1] [档位2] ... [档位N]
  0      1        2          N
```
其中 `档位1..N` = `ReasoningProfile.efforts`，N = `efforts.len()`。

**点数** = `N + 1`（Off 也算一个点）。

### R2（必须）Off 只在模型支持时显示

依据 `ReasoningProfile.supports_disabled: bool`（`vega_store/src/reasoning.rs:161`）与 `disabled_wire: Option<String>`。

- `supports_disabled == true` 且 `disabled_wire` 已声明 → 显示 Off
- 否则 → **不显示 Off**，滑块档位就是 `efforts` 本身

理由：`reasoning.rs:213-221` 已强制「`supports_disabled` 必须搭配匹配协议的 `disabled_wire`」。UI 必须尊重同一约束，否则会发出 provider 不接受的请求。

### R3（必须）选中 Off 走 `Disabled`，不走 `Effort`

- 选中 Off → `ReasoningChoice::Disabled`
- 选中档位 i → `ReasoningChoice::Effort(efforts[i])`

**不得**把 `off`/`none`/`disabled` 等字符串塞进 `efforts` 或作为 `Effort` 值发送。

### R4（必须）当前值解析

`ComposerDefaults.thinking` 的字符串编码与 `ReasoningChoice` 之间**已有完整双向映射**，无需新建：

| 位置 | 内容 |
|---|---|
| `core.rs:950-951` | `ProviderDefault → "provider_default"`、`Disabled → "disabled"` |
| `core.rs:980` | `provider_default` / `disabled` 不属于档位（无档位名） |
| `core.rs:986` | `"disabled" => ReasoningChoice::Disabled`（字符串 → 枚举） |

**本轮要做的**是把 `"disabled"` 映射到滑块的 **Off 位置**（现在它无档位名，因此不在滑块里）。

- `"disabled"` → Off 位置
- `"provider_default"` → 保持现状（第三种状态，见 R6）
- 其余 → 对应 `efforts` 里的档位

### R5（必须）Off 的视觉

按参考实现 `none` 的语义（最低档），但**具体视觉未实测**。要求：
- Off 位置在滑块最左，与档位点同规格
- 选中时 knob 停在 Off
- 档位名显示为 `关闭`（Vega 既有文案，见 `render.rs` 原 thinking chip 的 `"disabled" => "关闭"`）

**未实测项**：Off 位置的填充色（是否与低档同色）、档位名颜色。实现者按「与最低档同视觉」处理并在报告中标注为待校准。

### R6（必须）不改 `provider_default`

`ProviderDefault` 是 Vega 独有的第三种状态（不发送任何 thinking 字段）。**本轮不把它加进滑块**——它是"交给 provider 决定"，与"关闭"语义不同。

若当前值为 `provider_default`，滑块应显示为**无选中**或按实现者判断的最小侵入方式，并在报告中说明。

### R7（必须）重置按钮

**不做。** 移除滑块卡片右上角的圆形箭头图标（若已实现）。规格 R57 §3.4 R11 关于重置的条款**作废**。

理由：用户明确不做；且 Vega 只有一个 `preference` 字段，重置在语义上是空操作（P3 已发现）。

## §3 验收

| # | 证据 | 判据 |
|---|---|---|
| A1 | 生产测试 | `supports_disabled=true` 的模型：点数 = `efforts.len() + 1`，最左是 Off |
| A2 | 生产测试 | `supports_disabled=false` 的模型：点数 = `efforts.len()`，无 Off |
| A3 | 生产测试 | 选中 Off 后，持久化的值与发出的 wire 走 `Disabled` 路径（**不是** `Effort`） |
| A4 | 生产测试 | 选中档位 i 仍走 `Effort(efforts[i])`，回归不受影响 |
| A5 | 门禁 | `scripts/cargo-lock.sh test --workspace` 0 失败 |
| A6 | 原生截图 | 滑块最左出现 Off 位置（若模型支持） |

## §4 明确不做

- 重置按钮（R7）
- `provider_default` 进滑块（R6）
- 改 `reasoning.toml` schema
- 改 wire 编码（`openai/mod.rs:281-308`）
- 改 `efforts` 的语义

## §5 待实测（不得凭空指定）

| # | 项 |
|---|---|
| M1 | Off 位置的填充色（与最低档同色？独立色？） |
| M2 | Off 选中时档位名的颜色 |
| M3 | 参考实现 `none` 在滑块里的确切位置（是否真的最左） |

> 依据：用户截图里的两个状态（Ultra 全填、Medium 2 档）**都不含 `none`**，所以 `none` 的视觉无实测样本。源码显示 `none` 在序列第 0 位（`['none','minimal',...]`），本规格据此定为最左，但**视觉细节待补测**。
