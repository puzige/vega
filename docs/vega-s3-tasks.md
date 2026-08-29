# ✦ Vega — S3 任务卡（Sprint 3 · 流式会话渲染 · W5-6）

**版本** v0.1 · 2026-08-29 · 使用方式：每张任务卡 + [vega-exec-guide.md](vega-exec-guide.md) = 一条完整的执行 prompt
**S3 目标**（phase1-plan §2）：`vega_markdown` 流式 markdown 增量解析；代码块 tree-sitter 高亮；虚拟化长列表。
**Sprint DoD**（ui-spec §5 + tech-spec §5.2）：10k 行会话滚动 ≥120fps；流式追加不跳变、不重排已渲染区（P1/P3/P4 准线）；冻结区零重排；1k delta/s 注入 CPU <30%。
> ⚠️ **S3 是全项目最高风险 Sprint，T14 Spike 是强制前置（3 天时间框）**：spike 结论回写 tech-spec §5 后，T15+ 卡才最终化——不提前细化，因为方案随 spike 结论分叉（mdstream 集成 vs 降级方案 B）。

---

## 卡依赖图

```
T14 Spike（mdstream 尽调 + 增量管线 + 虚拟化滚动实测）
   │ 结论回写 tech-spec §5（架构师）
   ▼
T15 vega_markdown 流式管线（按 spike 结论定实现路线）
 ├──▶ T16 代码块 tree-sitter 高亮
 └──▶ T17 虚拟化会话流 + 锚定跟随（bench render_frame 实装）
          └──▶ T18 会话流 UI 成型 + Composer 最小版【范围待人类确认】
```

---

## T14 · S3 Spike：mdstream 尽调 + 增量管线 + 虚拟化滚动实测（强制前置）

- **前置**：S1/S2 · **参考**：tech-spec §5（全文）；risks §1；ui-spec §5（P1-P8）；phase1-plan §3.1
- **时间框**：3 天（人类节奏；agent 执行压缩为单卡）
- **产出**（全部为报告 + spec PR，**探索代码不入仓库**——探针项目放 /tmp，结论落 docs）：
  1. **mdstream 尽调（spec §5.0 写的 v0.2.0 已过时，crates.io 现为 0.3.0）**：license（可否引入）、0.2.0→0.3.0 变化、committed+pending 模型与 BlockId 稳定性、GFM 表格/tasklist 覆盖、reset 语义、gpui 支持成熟度、版本锁定或 vendoring 预案、维护活跃度
  2. **增量管线探针**（/tmp probe 项目）：TextDelta 高频注入 → committed/pending 分块 → 渲染指令流的最小可行管线；对照降级方案 B（按 block 分段全量重渲染）的简单实现
  3. **虚拟化滚动探针**：gpui 最小窗口 + 合成 10k 行内容 + 程序化滚动；**帧率测量方法 = render 计数器**（render 回调原子计数 + 1s tick 采样，报 fps）；流式注入 = 定时 delta 喂入
  4. **spec 修订 PR**：tech-spec §5 按 spike 结论重写（选定路线 + 实测数据 + mdstream 版本与 license 结论）；**若走 mdstream，同步提交白名单增补（mdstream 精确锁定版本，需人类批准）**
- **验收**（tech-spec §5.2 三条，探针实测）：① 10k 行滚动 ≥120fps；② 1k delta/s 注入 CPU <30%（`top -pid` 采样）；③ 冻结区零重排（渲染计数器分区对照）；外加 mdstream 尽调清单全绿
- **禁区**：探针代码不入仓库；spike 不改 vega 主工程任何 crate
- **出口**：架构师裁决路线（mdstream vs 方案 B）→ spec 合入 → T15 派单

## T15 · vega_markdown 流式管线（按 spike 结论）【骨架卡，T14 后最终化】

- **前置**：T14 出口裁决 · **参考**：tech-spec §5.1 §5.3（无论中间件如何必须自研的部分）
- **产出（预置两套路线，spike 后勾选）**：
  - 路线 A（mdstream）：workspace 白名单加 mdstream（人类批准后）；vega_markdown 封装 append/commit → RenderNode 树 + 按 BlockId 冻结缓存
  - 路线 B（自研）：按 block 分段全量重渲染（pulldown-cmark），块级 diff 更新
  - 共同验收：单测——delta 流 → 指令序列断言（含表格/列表/代码块/行内样式样本）；10k 文档分段解析内存有界
- **验收**：`cargo test -p vega_markdown` 全绿；管线层无 gpui 依赖（vega_markdown 禁 UI crate——比照 vega_runtime 红线）
- **禁区**：不做 UI；不做高亮（T16）

## T16 · 代码块 tree-sitter 高亮【骨架卡】

- **前置**：T15 · **参考**：tech-spec §5.1（committed 块高亮 / 未闭合降级纯文本）
- **产出**：vega_markdown 高亮模块——committed 代码块按语言 tree-sitter 高亮（首批 rust/ts/js/python/markdown 五种 grammar，**每个 grammar crate 入白名单需人类批准**）；未闭合 fence 纯文本等宽降级；闭合后升级高亮
- **验收**：单测（样本代码块 → 高亮 span 序列快照）；未闭合→闭合的升级路径测试
- **禁区**：不做编辑器；不做增量语法解析（以块为单位整块解析）

## T17 · 虚拟化会话流 + 锚定跟随（bench render_frame 实装）【骨架卡】

- **前置**：T15（T16 可并行）· **参考**：ui-spec §5 P1-P5；tech-spec §5.3
- **产出**：会话流虚拟化长列表（10k 行）+ 锚定跟随（贴底自动跟随；上翻 >1 屏不跟随，回底恢复）；流式追加不重排已渲染区；**xtask bench `render_frame` 占位转实装**（`#[gpui::test]` 帧计时或 render 计数器，报 fps）
- **验收**：xtask bench 出 render_frame 实测值；P1（≥120fps）/P3（零重排）/P4（锚定）走查+实测
- **禁区**：不做消息语义（T18）；不做 markdown 解析（T15 已做）

## T18 · 会话流 UI 成型 + Composer 最小版【范围待人类确认】

- **前置**：T15 T16 T17 · **参考**：tech-spec §5.4（final 终结语义）；ui-spec §4.4（Composer）
- **产出（预置，人类确认后最终化）**：
  - 消息流 UI：user/assistant 消息块（ui-spec §4.6 空态已就位）、流式追加动画禁令（§5.4：流式期间禁入场 opacity）
  - final 终结语义：MessageFinished 时作废 pending 补全、整块重解析冻结
  - **Composer 最小版**（phase1-plan S3 三项之外的最小补充——理由：S3 流式追加的演示载体 + S4 Runtime 的输入面；发送 → 本地 user 消息回显，**不接 LLM**）：多行自适应 1~8 行（ui-spec §4.4）、发送按钮 + Cmd+Enter
  - mock 流源：vega_markdown 回放器（读本地 md 文件按 N delta/s 注入）——S3 验收演示与后续 S4 mock provider 的公共基建
- **验收**：mock 流源驱动完整会话流演示（流式追加/冻结/高亮/锚定）；Composer 输入→回显；P2（上屏 <16ms）实测
- **禁区**：不接真实 provider（S4）；不做 @引用/命令/模型选择器（Composer 完全体后置）

---

## S3 完成定义（DoD，Sprint 验收）

- [ ] T14 spike 报告 + tech-spec §5 修订合入（路线裁决留痕）
- [ ] T15-T18 全绿；hooks 门禁 master 绿
- [ ] **ui-spec P1/P2/P3/P4 实测达标**（120fps / <16ms / 零重排 / 锚定），P7/P8 不回退
- [ ] mock 流源端到端演示（10k 行会话 + 流式注入 + 高亮 + 锚定跟随）
- [ ] 红线全过；六表外零 DDL；vega_markdown 无 UI 依赖
