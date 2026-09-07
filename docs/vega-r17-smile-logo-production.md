# R17 — 蓝色单星笑脸Logo正式接入

Date: 2026-09-08
Status: Completed; root acceptance in vega-r17-acceptance.md
Executor: Luna max; root coordinates, reviews and accepts.

## 用户定稿与范围

用户选择 `assets/logo/explorations/r16/vega-blue-01-single-spark-smile.png` 的蓝色单星笑脸作为Vega正式Logo，授权实现。不是02的镂空星版本。浅色为宝石蓝，深色为冰蓝；图形只有三个部分：连接的终端 `>`、右侧单颗修长曲边四角星、下方轻微上扬的光标。保留参考的柔和高光、轻微浮起层次与macOS底板。

实现正式、可维护的统一几何SVG（浅/深/单色），接入现有生产ICNS导出和产品内确有Logo用途的资源引用。保留历史F1/F3与R16设计探索，不将带背景的1536px方案板裁剪充当生产图标。推荐新增具语义的正式文件名，并将生产引用切向新主标；旧文件可继续存档。全应用UI配色不在本卡范围。

## 生产约束

- 沿用R15的1024透明画布、居中832底板、四边96透明留白；保持既有Dock/Launchpad尺寸，不修改系统Dock大小/数据库布局。
- 图形比例、单星数量和微笑光标必须与选中参考一致；浅深版使用相同轮廓，小尺寸依然清楚。
- 通过现有AppKit SVG渲染和 `cargo xtask package-icon` 的真实入口导出ICNS；不可依赖浏览器效果而忽略AppKit实渲染。对渐变/高光的实现选择以生产渲染效果为准，不增加依赖。
- 修改范围：`assets/logo/`正式资源、`xtask/`必要的图标源引用/文案、现有Logo消费入口的最小替换、`docs/vega-r17-logo-delivery.md`。如需要扩展到其他生产模块先向root说明。
- 不改业务/凭据/数据库/权限，不访问Keychain、不发真实供应商请求。性能专项仍延期。

## 交付与验收

Executor先读AGENTS.md与docs/vega-exec-guide.md，查看参考图；在独立sibling制作，提交代码和交付文档。交付包含改动文件、统一几何来源、真实生产导出、十档ICNS解码与透明边缘检查、浅深及小尺寸PNG实渲染、测试结果、剩余偏差。

Executor执行fmt、受影响xtask测试及strict lint、生产package-icon导出。Root独立review、统一门禁、构建/签名并检查真实Dock/Launchpad效果；在替换已安装资源前保留回退副本。优先复用当前有效应用二进制进行图标资源替换，避免无关业务变化；如需新构建副本先关闭旧Vega实例。用户已授权作为Logo接入，无需再次征询同一批准。

## 并行清理

Root负责清理 `~/Workspace/worktrees/` 和 `~/Workspace/` 下不用的注册worktree：先核对Git状态、持久分支、运行进程和后台服务引用；当前集成树、新Logo实现树、主仓库、未保存改动及仍在用的目录保留。不猜测不明目录用途，不删除分支历史。清理记录与恢复命令另存。
