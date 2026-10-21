# #515 Copy 字形光学归位

关联：[#515](https://github.com/puzige/vega/issues/515)，父卡 [#498](https://github.com/puzige/vega/issues/498)。方案 v1。生产基线 `e542175a`；任务分支 `feat/515-copy-optical-weight`。

## 1. 目标与范围

将全局 `Icon::Copy` 的 16px 框内几何墨迹宽度从约 14.667px 收至 13.333px（占比约 0.8333），保持约 1.333px 描边，与 Vega 中位图标 / GitBranch 配对。依据是 Vega 自身光学规范和本卡已冻结的用户目标，不使用第三方私有资产。

现状：`icons.rs::icon_name` 把 Copy 映射为 gpui-kit 的 Lucide Copy；`sized_icon` 走 KitIcon。消费者为 `conversation_stream/render_rows.rs` 的消息复制按钮与代码块复制按钮；两者均使用共享 `icon_button`。本卡只换共享字形。

代码实现只改 `crates/vega_ui/src/icons.rs`：新增 `COPY_SVG`；`sized_icon` 中 Copy 转向 `inline_icon(COPY_SVG, color, size)`；`icon_name` 的 Copy 转向已有 inline 图标的 unreachable 分支。保持现有其它图标与调用方不变。

## 2. 验收矩阵（先于实现冻结）

| 对象/场景 | 必须保持或达到的结果 | 验证边界 |
| --- | --- | --- |
| Copy 全局路由 | 消息与代码块均使用新 Copy，不产生第二种复制图标 | 静态调用链 + 用户实际界面 |
| SVG 几何 | 16px 框内墨迹约 13.333px，描边约 1.333px，居中 | 现有真实 SVG 渲染接口的 Actions 先行回归；Retina 用户手测 |
| 分组变换 | usvg 对正 viewBox 与 g 平移的解析/渲染符合目标，无裁切 | 真实 SVG 渲染回归 + 用户手测 |
| 主题 | 浅/深主题的颜色仍由 `text_secondary` 传入 | 静态颜色链 + 用户手测 |
| 消息复制按钮 | 命中框 24×24、Tab 可达、Enter/Space 触发、tooltip、aria-label 不变 | 生产差异审查 + 用户手测 |
| 代码块复制按钮 | 只随全局换字形，位置/显隐/复制行为不变 | 生产差异审查 + 用户手测 |
| GitBranch 配对 | Retina 下 Copy 不比 Fork 更重或更轻 | 用户手测；源码算术不能替代观感验收 |

禁止修改 `Layout::ICON_SIZE`、`sized_icon` 签名、`icon_button`、调用方、布局或交互。

## 3. 精确资产与算术

SVG 固定为 issue 指定的以下字节内容（不附加代码注释，遵守仓库规则）：

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 26.6667 26.6667" fill="none" stroke="currentColor" stroke-width="2.2222" stroke-linecap="round" stroke-linejoin="round"><g transform="translate(1.3333 1.3333)"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></g></svg>
```

几何中心线极值 2..22；取描边两侧各半宽计算包围盒：

| 项目 | 现状 | 目标（四位小数近似） |
| --- | --- | --- |
| viewBox 宽 | 24 | 26.6667 |
| UI 缩放 | 16/24 = 0.666667 | 16/26.6667 = 0.59999925 |
| 源描边宽 | 2 | 2.2222 = 2/0.9 的截断近似 |
| UI 描边 | 2×16/24 = 1.333333 | 2.2222×16/26.6667 = 1.333318 |
| 墨迹宽 | (20+2)×16/24 = 14.666667 | (20+2.2222)×16/26.6667 = 13.333303 |
| 理想每侧留白 | (16−14.666667)/2 = 0.666667 | (16−13.333303)/2 = 1.333348 |
| 目标实际左/右留白 | 0.666667 / 0.666667 | (2+1.3333−1.1111)×16/26.6667 = 1.333318 / 1.333378 |

v1 澄清 issue 的两处文字误差，不改变冻结资产/验收：旧字形每侧留白不是 0；源描边补偿为除以 0.9，不是乘以 0.9。四位小数带来的左右差约 0.00006px，不影响 13.33px 目标。32px Retina 栅格的连续墨迹范围约 26.6666px、描边约 2.6666px；抗锯齿像素计数不等于连续几何尺寸。

## 4. 先行测试与交付顺序

生产未修改前，由实现子 Agent 使用现有 `gpui_kit::SvgRenderer` / `SvgSize::ExactSize` 的真实 usvg/resvg 输出，对未来 `COPY_SVG` 编写几何行为测试；不得仅断言 SVG 字符串、查源码或镜像算法。不新增依赖、不扩生产 public API。

先行调查确认 `sized_icon` 返回包装后的 `AnyElement`、Svg payload 私有，现有 seam 不能直接观测旧/新路由选择与语义 tint。因此采用执行总纲 §7.2 允许的最小接口缺失 RED：生产常量尚不存在时，测试引用 `COPY_SVG` 的编译失败只记录为接口缺失，明确行为断言尚未执行，不冒充旧字形渲染断言失败。实现后同一测试必须真实执行 GREEN，检查 16px/32px 栅格的 alpha 墨迹和直线段描边；路由及 tint 由静态调用链审查与用户手测覆盖，不把原始 SVG 渲染说成完整界面/颜色验收。

测试产物就绪并停止写入后，非实现者独立审查。之后建立 draft PR，以现有 `.github/workflows/pr-check.yml` 的 Actions 取得缺失 `COPY_SVG` 的最小接口 RED，排除其它编译或 runner 原因。主 Agent 固定测试目标/预期及原始源码、完整 SHA、run/job 后才授权实现。相同冻结测试 GREEN 后固定证据，以独立提交删除 Rust 测试及独占支持；最终空集 head 完整 required check 通过、独立审查无缺口后转正式 PR 并 squash merge。

本地只进行快速差异、格式与静态检查；不运行 Vega，不编译、构建、打包或测试。RED/GREEN 与最终完整门禁均交 Actions。未运行项不得标为通过，空集明确为 0 发现/0 执行而非测试通过。自动渲染回归不替代用户 Retina 观感验收。

### 4.1 测量接线纠正记录（目标不变）

初始测试源码固定在 `ac2b5c1f7af20b31238a2c951dff50e2220783dc`；接口缺失 RED 为 [run 39565316867](https://github.com/puzige/vega/actions/runs/39565316867)。实现 `8e4527b24e8e89c3f24abe3fd04399b3566a1af3` 的 [run 39566283153](https://github.com/puzige/vega/actions/runs/39566283153) 实际执行 2 项、0 通过、2 失败、0 跳过：16px 墨迹 alpha 推算为 13.5059px、直线描边覆盖为 1.5059px，超出原有 ±0.15px；32px 断言未执行。不能将此称为 GREEN。

独立只读诊断确认原测量前提不成立：tiny-skia 0.11.4 的 `scan/path_aa.rs` 使用 4 倍扫描网格并量化 coverage，直接 16/32 栅格的 alpha 不等于精确解析面积；GPUI `Window::paint_svg` 实际使用 `SMOOTH_SVG_SCALE_FACTOR = 2` 的图集 mask，再按半尺寸显示。原测试的 ExactSize 16/32 绕过此观测边界。接口 RED、原始测试和首次执行失败均在 [PR #528](https://github.com/puzige/vega/pull/528) 留证。

按执行总纲 §7.2 测试接线纠正规则，先记录缺陷并独立审查批准，之后只修正 harness：按生产约定将 16/32 设备像素对应的 mask 渲染为 32/64 栅格，所有维度测量除以 2 后再与原目标比较；alpha 不缩放。测试名称、13.3333/26.6667 墨迹目标、1.3333/2.6667 描边目标、居中目标、±0.15 设备像素容差与冻结 SVG 均不变。不拟合失败输出、不放宽断言、不调整生产。另以等价 `as_chunks::<4>()` 和余数为空断言纠正 Clippy 的测试解码接线问题。修正后必须重新取得 Actions GREEN；若仍失败，停止复核，不能继续换采样方式试绿。

此回归验证生产图集 mask 的几何，不宣称复现最终 GPU 纹理采样或 Retina 主观观感；用户实际界面验收仍为必要条件。

## 5. 手测与回滚

合并后保持 Issue 开放并置 In review：用户在 CI 发布后的实际应用检查浅/深主题、消息与代码块复制、Copy/Fork Retina 配对、Tab/Enter/Space、tooltip 和可访问名称。未收到明确通过前不关闭或 Done。

回滚仅撤销 COPY_SVG 与两处 Copy 路由修改，不调整其它图标、token 或按钮。交付记录固定 PR、最终完整 SHA、RED/GREEN、删测与最终门禁链接；当前这些证据尚未取得。
