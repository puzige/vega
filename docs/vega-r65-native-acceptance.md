# R65 实机验收报告（滑块填充胶囊形状）

> 日期：2026-09-15
> 受测构建：`dist/Vega.app` → `/Applications/Vega.app`，`md5 f896b992421b81b2da6d98b56e8837e4`
> 基线：`master @ e10e47c`（R65 首轮 `79269a7` + R65b `e10e47c`）
> 签名：`codesign --verify --deep --strict` 通过

---

## §1 结论

**两个缺陷都已修复并实机验证通过。**

| 缺陷 | 修复前 | 修复后 | 结论 |
|---|---|---|---|
| 最低档 / 关闭：左端漏蓝 | 填充左端方角，戳出轨道 6px | 轨道内**无任何蓝色**（填充 = 端帽，被圆点覆盖） | ✅ |
| 最强档：渐变右端方角 | 每行右界固定 613，戳出右帽 12px | 两端各内缩 18px，**对称弧形** | ✅ |

---

## §2 两个缺陷的来源

**这不是一个缺陷的两个面，是两个独立的缺陷**，用户 2026-09-15 的两张复验截图分别暴露了它们。

### 缺陷 1：最低档左端漏蓝 —— **我 R65 规格里的算术错误**

首轮规格 §4b 我写了：

> "fill = 12.0 时半径钳到 6.0，在 12px 宽的方块上视觉上就是左半圆"

**这是错的。** 真半圆需要半径 `H/2 = 12`；半径 6 时填充在 y=0 处左界是 x=6，而圆点左界是 x=12 —— **露出 6px 蓝**。

算术（填充宽 12、高 24、左角半径 r，圆点圆心 (12,12) 半径 12）：

| 行 y | 半径 6 时填充左界 | 圆点左界 | 露出 |
|---|---|---|---|
| 0 | 6.00 | 12.00 | **6.00** |
| 1 | 2.68 | 7.20 | **4.52** |
| 2 | 1.53 | 5.37 | **3.84** |
| 6 | 0.00 | 1.61 | **1.61** |
| 12 | 0.00 | 0.00 | 0.00 |

半径 12 时每行露出均为 0。

**修正**：填充宽度先撑到 `THINKING_TRACK_HEIGHT`（24）再上半径，钳制上限才够 12。撑宽最多 12px，圆点（直径 24、后绘）正好覆盖。

**副产物**：这消解了一个我差点去问用户的二选一。位置 0 的圆点圆心 x=12、直径 24，与轨道左帽**完全重合**——所以"左端做成真半圆"和"该档不填充"是**同一个形状**。规格 R4c 明确写了不要加 `is_off` 分支。

### 缺陷 2：最强档渐变右端方角 —— **我 R65 的遗漏**

首轮只给了第一段 `rounded_l`，**漏了最后一段**。`TRACK_GRADIENT` 铺满 `0..203`，最后一段结束在轨道右缘且无圆角，上下行戳出 12px。

用户第二张图实测：轨道行每行都结束于 x=613，而右帽在该行应止于 601。

**修正**：最后一段（`index == TRACK_GRADIENT.len() - 2`）加 `rounded_r`。中间段不动。

---

## §3 门禁与证伪（我本人独立执行）

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | 退出 0，无输出 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出 0 |
| `cargo test -p vega_ui` | **289 passed / 0 failed** |
| `cargo test --workspace` | 1154 passed / 0 failed（首次运行 1 例已知负载敏感测试失败，见 §5） |

### 证伪：两个缺陷的测试都能抓住回归

**证伪 1** —— 禁用渐变右端帽（`gradient_segment_has_right_cap` 恒 false）：

```
r65_the_gradient_caps_its_first_and_last_segments_only ... FAILED
assertion failed: gradient_segment_has_right_cap(last)
```

**证伪 2** —— 移除填充撑宽（`fill_effective_width` 去掉 `.max(THINKING_TRACK_HEIGHT)`）：

```
r65_the_fill_is_padded_to_the_track_height_before_the_cap_radius ... FAILED
assertion `left == right` failed: R4b: the narrow fill is padded to the track height
r65_only_the_padded_fill_takes_a_right_cap ... FAILED
```

两次证伪后均已还原（`git status` 干净）。

---

## §4 实机像素验证

### §4.1 最强档（`high`）：两端对称弧形

逐行轮廓（`/tmp/accept-r65b-gradient.png`，捕获像素）：

| y | 填充左界 | 填充右界 |
|---|---|---|
| 1510（上边） | 1598 | 1957 |
| 1533（中线） | 1580 | 1937 |
| 1557（下边） | 1598 | 1957 |

**左端内缩 18px、右端内缩 20px —— 两端对称弧形。**

修复前对比：右端**每一行都是 613**（垂直直边）。

### §4.2 最低档（关闭）：轨道内无蓝色

同一扫描下 `fill(sat)` 整列为 `-`；全轨道范围（x 1560..2010）无任何饱和色像素。

视觉（`/tmp/accept-r65b-lowest-crop.png`）：白色圆点贴轨道左端，圆点之外是灰色轨道，**无蓝色残角**。

这正是规格 R4c 的预期：该档位的填充在几何上等于端帽，端帽整个被圆点覆盖。**不是"蓝色丢了"，是"填充恰好等于端帽"。**

---

## §5 已知的无关失败

首次 `cargo test --workspace` 有 1 例失败：

```
git_workspace::trusted_git::tests::commit_proof::commit_proof_rejects_parent_tree_and_final_ref_faults_after_one_commit
proof checklist: ProcessControlFailed
```

**判定为既有的负载敏感测试，与本次改动无关**：

- 在干净 `master @ 183b004` 上单跑：**通过**（37.49s）
- 在 R65b 上单跑：**通过**（37.60s）
- 仅在全量并发运行时偶发

R65b 改的是 UI 滑块圆角，与该测试（git 进程控制）无任何代码路径交集。此测试已在 `AGENTS.md` 与既有交接记录中登记为负载敏感。

---

## §6 我在本轮的两处错误（记录以免重犯）

1. **规格里的算术错误**（缺陷 1 的成因）。我写"半径钳到 6 就是半圆"时**没有验算**，只做了直觉判断。这个错误直接导致首轮修复无效，浪费了一整轮实现 + 打包 + 验收。

   **教训**：规格里凡是出现数值断言（半径、坐标、差值），必须当场算一遍并写进规格。R65 §4b 修正版已附上完整的逐行算术表。

2. **遗漏了对称的另一端**（缺陷 2 的成因）。我修左端时，没有问"另一端呢"。渐变分支和 flat 分支是两段独立代码，我只改了前者的一半。

   **教训**：改"形状/对称性"类缺陷时，必须把**所有**产生该形状的代码路径列出来（这里是 flat + gradient 两个分支 × 左右两端 = 4 个角），逐一核对。R65 §6 的 A3 判据已加入"最强档右端不得戳出"。

---

## §7 证据文件

| 文件 | 内容 |
|---|---|
| `/tmp/before-r65b-lowest-leak.png` | 修复前：最低档左端漏蓝（用户截图） |
| `/tmp/before-r65b-gradient-square.png` | 修复前：最强档右端方角（用户截图） |
| `/tmp/accept-r65b-lowest.png` / `-crop.png` | 修复后：最低档无蓝 |
| `/tmp/accept-r65b-gradient.png` / `-crop.png` | 修复后：最强档两端弧形 |
