# ImagePreview NSH 生产迁移

## 范围与基线

- 工作区 `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`，基线 `bf0be9adb642077b2cebdb6f081b3ecf01209dba`；主控已 fetch 并确认 `origin/master` 的 `4b604cb8` 为祖先。本叶子不操作 Git/index/提交。
- 仅写 `crates/vega_ui/src/image_preview.rs`、新 `image_preview/*` 与本文；共享 `crate::nsh`、调用方、服务、依赖及锁文件只读。无子 Agent、构建、测试、应用执行、打包或安装；验证由主控协调。
- 遵循 `AGENTS.md`、执行/设计指南、`nsh-full.md` 与图片原始规格 `../296/vega-issue-296-image-preview.md`（含图片栏清理修订）。不恢复旧规格的强制 TDD。

## 实现前验收计划

| 边界 | 验收要求 | 验证方式 |
| --- | --- | --- |
| 生产绘制 | 全图预览与本模块公开缩略图由 NSH image API 绘制；GPUI 仅为 Surface/焦点/可见性追踪宿主，不嵌入旧 GPUI 图片或菜单 | 静态 API 核对、主控编译、原生手测 |
| 原图 | PNG/JPEG/WebP、横竖/极长/小图均完整 contain、不拉伸；窄窗保留操作，局部内容可滚动；加载和失败真实可见，可继续保存 | 静态布局检查、三桌面手测 |
| 层级 | 文件名为标题，打开方式/保存保持可达；不恢复缩略图尺寸栏或外部打开成功提示，不添加实现说明 | 静态检查、浅深色/中英文手测 |
| 公开接口 | 保留 ImagePreviewAction 六类事件、EventEmitter、new/pending、image/pixels/name、thumbnail/thumbnail_sized/tracker/visibility/reset/focus、status/busy/begin/finish 等实际接口与调用方 | 静态签名清点、主控编译 |
| 图片生命周期 | 原后台 GPUI Image/OnceLock 保留用于现有内部消费者；pending 在原像素未就绪时不伪造完成；不修改附件校验/来源/路由/导出/缓存/业务所有权 | 静态数据流检查 |
| 图片输入 | 仅已验证 ImageAttachment 的原始编码字节；不读取路径或 URL、不引入 fileURI、不进行图像解码/重编码。内存 data URI 只生成一次，后台进行，快照只共享引用 | 固定依赖源码与数据流检查 |
| 应用选择 | 真实原列表，默认首项焦点、上下/Home/End/Enter/Space、Tab/Shift-Tab、Escape、外部点击、同触发器再次关闭和焦点返回；选择绑定所展示列表的不可复用身份，不将旧索引映射到新列表 | NSH 0.4 overlay/焦点源码核对、主控回归及手测 |
| 操作 | 仍发原 Open/Save/Applications/DismissApplications/ChooseApplication(index)/Close；忙碌状态遵守原白名单，取消能结束枚举；旧展示命令拒绝，未知结果不重放 | 静态分支审查、主控回归 |
| 焦点 | 公开 Focusable 获取或转发到 NSH 原生焦点，保留旧外层调用安全；缩略图 Enter/Space 仍可打开 | 静态链路检查、原生手测 |
| 环境/失败 | locale/theme 沿用共享 Surface；展示端失败不冒充成功，提供重新加载展示入口，不重试业务操作 | 静态检查、主控故障回归 |

## 会话截图卡片追加验收（主控实现前记录）

- 会话截图卡片复用同一 ImagePreview 的 NSH 缩略图 Surface，删除截图模块独立 GPUI 图片、边框和可见操作；保留现有附件、预览实体、observe_image 和打开工作区事件，不新增文件读取或图片转码。
- 保留按原图等比缩放至 SCREENSHOT_MAX_WIDTH/HEIGHT、最小宽高和边框的几何；宿主仅负责大小和原可见性追踪，图片 contain、边框、圆角、hover/键盘焦点、可访问名称及按需快捷键帮助由 NSH 绘制。
- 截图与普通附件缩略图样式分开；样式切换重建缩略图展示身份，旧动作仍被拒绝。Enter/Space 和点击仍走原 ImagePreviewAction::Open；不更改源图缓存、回收和导出所有权。
- 验证为主控编译、非实现者增量审查与原生手测；此前图片审查结论不冒充覆盖本次新增路径。

## 已核对的固定 API

实际 registry 源码为 NSH `0.4.0`，无 Cargo 修改：

- `nsh_core::element::image(impl IntoReactive<ImageSrc>)`，`Image::on_load/on_error`，`ResizeMode::Contain`。
- `ImageSrc` 的 `data` 为 crate 私有；公开 `ImageSource::with_bytes` 不能直接转换成 Rust builder 的 `ImageSrc`。不通过 unsafe、JS host 伪装或改依赖绕过。
- 使用 `ImageSrc::from(SharedString)` 的标准 `data:<mime>,...` 入口。对原始压缩字节进行一次百分号传输转义，不重新编码 PNG/JPEG/WebP、不解码，不在 UI 关键路径执行。最坏传输字符串为原字节的三倍（附件原有上限 8 MiB）；它由 OnceLock 共享，不在每个快照复制。NSH 自身的 data URI 解包/显示解码属于其图片 API。
- 实际核对 `Menu/MenuItem` 后，不依赖其未提供的完整焦点/方向键契约；使用 NSH `overlay`、`ElementRef`、局部 `on_key`、`scroll` 显式实现首项焦点、方向/Home/End、Tab/Shift-Tab 循环与可见性滚动。overlay 的外部点击排除 anchor，避免同触发器 down/up 重开；Escape 由原生 overlay cancel 处理。Surface 仍使用固定 64 容量通道与主线程原控制器。
- 基线不存在 `set_pixels` 方法；像素通过公开 `pixels() -> Arc<OnceLock<Arc<Image>>>` 写入。不得为了迁移改变该写入协议。

## 验证记录

本文验收计划先于代码写入，现已实现并停写交接；READY 仅指叶子产物可供主控集成/编译/独立审查，不等于生产验收完成。

- `ImagePreview::render` 与本模块公开 thumbnail/thumbnail_sized 不再绘制 GPUI 产品图片/菜单。GPUI 只嵌入 NSH Surface、转发稳定外层 FocusHandle 并保留 viewport thumbnail tracker；原 Image/OnceLock 后台机制和全部 20 个实际公开方法保留。基线无 set_pixels，未凭空新增或要求调用方改接口。
- 应用列表每次 set_applications 分配不可复用身份，选择命令捕获该列表身份；可信接收方核对 Surface 身份、列表身份、有效索引与当前 busy，拒绝旧列表索引。其他动作核对当前 revision。标签刷新不更换应用身份，不反复重建菜单。选择、取消、关闭继续发原六类事件；枚举期间允许同触发器或 Cancel/Escape 发原 DismissApplications，由原服务取消操作。
- 外层 Focusable 保留稳定身份，挂载时以及订阅聚焦后向 NSH 发送递增 focus revision，原生按钮 ElementRef 接收；恢复页独立 Surface，Reload 只销毁展示，不重放保存/打开等未知业务操作。
- 图片 source 仅后台生成一次；快照共享 SharedString。图像节点只在 source 就绪状态翻转时创建，普通状态/主题刷新不重载大图。完整 contain、小图最大自然尺寸、标题换行和原图区域局部滚动已实现；真实几何、长列表键盘滚动和错误恢复效果仍待手测。
- 本叶子不改 ScreenshotCard、会话其它图片视图或任何其它模块；这些范围的全量 NSH 切换仍由主控/对应单写者负责，不宣称本叶子完成整个产品迁移。

### 已执行静态验证

- 工具：`rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`；只格式化两份自有 Rust 文件。`rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/image_preview.rs crates/vega_ui/src/image_preview/nsh_preview.rs` 返回 0。
- `git diff --check -- crates/vega_ui/src/image_preview.rs` 返回 0；新文件另检查尾随空格。只读 Git diff/show/rev-parse，没有 Git/index 写入、fetch/rebase/commit/push。
- 文件级静态清点返回 0：20 个公开方法及顺序与基线一致，所有使用的 ui 翻译键在 en/zh-CN 均存在，无注释、unsafe、unwrap/expect、文件读取、file URI、try_send、Rust 测试或旧 img/deferred 菜单绘制。该清点不是行为测试，也不能证明 Rust 类型检查通过。
- 核对实际固定 NSH 源码并修正带数据回调签名、`ScrollEvent.offset` 和 `Align` 导出；不增加依赖、不修改 Cargo。首次禁止项 grep 表达式有语法错误，已修正为逐项检查并取得上述结果，不将错误命令记为通过。

### 停写 Rust 产物 SHA-256

| 文件 | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/image_preview.rs` | `419d313d4968ac4b686a0726ebf57d4601aa82a28734cba078f0f450f75b9675` |
| `crates/vega_ui/src/image_preview/nsh_preview.rs` | `d4d1e97b5d0acee36c031909d5a2a76364b93004939d97341f58123a3bf58e22` |

### 主控集成增量与独立审查

- 独立审查发现恢复 Surface 缺少显式保存入口，以及保存服务失败状态未呈现；主控已增加原 Save 动作（原身份/revision/busy 围栏）、真实 Snapshot.status、拒绝回执失败反馈。Reload 仍仅重建展示，不重放保存。两项问题在 `a7c955ae32528268de76a3537ac3bb5a31ffe47a4a123491ebb5c51ff8f11058` 静态复核关闭。
- 会话截图卡片新增复用路径，保留同一附件/预览实体、observe_image、Open 事件及可见性追踪。NSH 负责截图边框/圆角/标签/快捷键帮助与图片；宿主只提供几何。窄列按宽高比缩放。非实现者增量审查发现普通缩略图零宽边框使焦点不可见，已恢复 1px 并复核关闭；无其他具体发现。
- 当前停写版本：`image_preview.rs` SHA-256 `0913507d2b5400355ae0abe58c5e4ac19fed1d89a6ebe88427597b6a3002e91b`；`image_preview/nsh_preview.rs` `2f3790d67292a17f2cf71717eccc780995c365164f60512f166924626a1a5661`；`conversation_stream/screenshots.rs` `864d4555fea279ad1f4006f14a43f5039a1e3da7b8c058f778e182dadd9ac186`。
- 主控 scoped rustfmt 与 diff 检查通过；最新集成编译在活跃 CUA 模块的语法错误停止，尚未证明该截图版本通过类型检查。无原生交互/三桌面运行证据；静态审查不是最终完整 head 门禁。

### 未运行与限制

构建、Clippy、Rust 回归、CI、真实保存/系统应用枚举/打开、原生焦点/键盘/弹层以及 macOS/Windows/Linux 手测本叶子均未运行。主控需检查最终集成 head、独立审查并覆盖上述验收矩阵，尤其关闭/重开、同列表标签刷新、相同长度新列表、重复触发、枚举取消、busy/error、独立恢复和窄窗极端图片比例。无 Rust 开发测试新增，未声称已有测试通过。传输源最多三倍原压缩字节，加 NSH 自身缓存成本；不宣称零拷贝端到端或性能收益。没有 essential API blocker，但无本轮编译证据，仍可能有需主控定位的类型或接线问题。
