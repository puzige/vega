# #542 测试观测接线核查

阶段：生产实现前静态核查；未编译、未运行测试，无 RED/GREEN。

基线：`871f91f246e90753743a256cee1eec315e74effc`。

## 已核实的可行路线

锁定版本为 `gpui-kit 0.6.1` / `gpui-base 0.6.1`，底层 `gpui-pre 0.3.4`。
以下均为本机对应 registry 源码静态检查，不是运行结果。

- `gpui-pre/src/elements/text.rs:254–375`：`&'static str`、`SharedString` 的
  `Element::RequestLayoutState = TextLayout`；`String::into_element()` 返回
  `SharedString`。可用测试专用透明 Element 包装实际文本叶子，原样委托布局、
  prepaint、paint，不重写文本或业务逻辑。
- 同文件 `918–977`：`TextLayout::text()`、`wrapped_text()`、`bounds()`、
  `line_layouts()` 均为公开 API。父 Agent 已复核 `254–318`、`918–977`。
- 同文件 `626–645,660–746`：普通字符串的默认 text run 使用
  `window.text_style().to_run(...)`，布局阶段确定颜色并塑形。
- `gpui-pre/src/window.rs:2269–2275`、`style.rs:558–573`：
  `Window::text_style()` 提供实际继承样式，`to_run()` 复制其颜色。
- 只在真实 leaf 的 `paint` 执行时记录该布局的文本/几何及布局时取得的颜色，
  同时记录 content mask/viewport，按窗口和当前帧隔离。比较颜色使用当前主题
  `Hsla::from(theme(cx).colors.danger)`，不把期望色作为观察输入。
- 对显式 TextRun/highlight 的 StyledText 不能仅用继承色推断实际 run 色。

## 证明边界

上述证据是**真实文本布局及绘制调用路径**，不是最终 framebuffer 像素。要求文本
在 clip/viewport 内，必要时观察实际父容器的 visibility/opacity；不能用布局存在
推断未被遮挡或完全可见。异常路径要先确认观察接线的阳性对照，而不是直接算产品 RED。

GPUI `ElementSnapshot` 的 label/value 是 AX 元数据，不是可见文本；没有 color getter。
`painted_quads()` 可检查背景几何，但文字是 glyph sprite，不能用 quad 色冒充字色。
默认 TestAppContext 使用 NoopTextSystem、没有 headless renderer；仅开启 test-support
不足以得到截图像素证据。不得为此引入本地执行或新 renderer/dependency。

## 观察流结构

通过真实 `ConversationStream::new`、`apply_event`、`apply_artifact_card`、
`apply_task_summary` 驱动，挂载实际 stream，不读取私有 entries 字段。

- 在实际 assistant、ArtifactGroup、文件/错误行、Summary 容器上使用临时观察接线，
  `.id(...).test_support()`；`within(...)` 的 ancestry 与当前 bounds 验证归属和顺序。
- 正常 selectable assistant 的容器在 `render_rows.rs:532–553`，不要只观察备用
  assistant 路径。现有 message-list 在 `render.rs:1487–1495`，列表本身是虚拟化的。
- 使用足够大的受控窗口或公开滚动，不能把虚拟化离屏误判为不存在，不能把
  snapshot 枚举顺序当作视觉顺序。
- 无来源标签/编辑器行的否定断言须收集整个被测产物表面的实际文本叶子，并在
  RED 基线确实观察到旧来源/按钮；不能仅验证缺少某个观察 ID。
- workspace 回归在 UI test host 内复用 `ArtifactCard::row_count()` 和
  `ArtifactCard::render_row()`，报告为共享 renderer 覆盖，不冒充完整 app controller 验收。

测试写入者可在 `artifact_card.rs` 的 cfg(test) 模块内实现最小 pass-through helper，
必要的 render site 只作 cfg(test) 透明接线。所有独占接线和 helper 随测试在 GREEN 后删除。

## 最终仍待 Actions

`gpui_kit` 现有依赖临时开启 `test-support` 可提供上述公开测试接口，无新 crate。
API 类型组合、运行观察有效性以及本卡行为必须由现有 Actions 验证，本记录不宣称通过。
