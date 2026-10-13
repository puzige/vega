# #296 消息导航与侧栏边界交付记录

## Freeze

- 规格：[面板边缘契约](vega-issue-296-navigation-pane-edge.md)，N01–N04；无偏离。
- 时间：2026-10-13 06:43 UTC / 14:43 Asia/Shanghai。
- 分支：`feat/296-navigation-pane-edge`，基线为 PR #327 合并后的 master。
- 环境：macOS arm64；Rust / Cargo 1.98.0；独立 worktree 默认 target。
- 本次代码 diff SHA-256：`363bccc8e5725299a0567da88d435b1df8ae95bb3520dcc596172da38729ef36`。

## 实现

消息刻度使用现有 pane 左内缩4，删除仅服务旧正文相对定位的54偏移token；预览仍随刻度定位。侧栏5px拖拽槽延续侧栏主题表面，1px分隔线移至其右缘。宽度、持久化、消息导航状态与权限不变。

## Results

进程内 GPUI production-host 几何/绘制及交互回归，不是原生应用截图验收。

| 需求 | 证据与结果 |
|---|---|
| T01 | 先改旧位置断言，原实现红测 `left: 278px / right: 16px`；修复后宽窄窗口通过 |
| T02 | 真实 VegaWindow host：Sidebar显示/隐藏、240/304/365宽、右Workspace开关；刻度中心相对pane左缘16，hover预览不越界 |
| T03 | 原issue72导航13项通过，包括身份、短内容隐藏、滚轮/键盘/点击、预览与历史水合 |
| T04 | 原resizer实现红测 `resize lane must continue the sidebar surface`；修复后浅深主题5px表面及右缘单1px线通过；R21拖拽/持久化回归通过 |

最终命令：

```sh
cargo nextest run -p vega_ui -p vega -E 'test(issue296_visual_v08) | test(issue296_navigation_pane_edge) | test(issue296_sidebar_boundary) | test(issue296_reading_column) | test(issue72_message_anchor_navigation) | test(r21_sidebar_drag)'
```

原始末尾：

```text
Summary [   0.465s] 18 tests run: 18 passed, 984 skipped
```

退出码0。筛选外984项未在本地运行；全量由云端门禁负责。

红测命令分别使用上述包与filter的 `test(issue296_visual_v08) | test(issue296_navigation_pane_edge) | test(issue296_sidebar_boundary)`、`-p vega -E 'test(issue296_sidebar_boundary)'`。第一轮只有rail断言为产品红测；两个shell测试初稿误用了按测试名取Git录制的fixture，未到产品断言。更换为临时目录/in-memory store的生产host后，修复了helper括号及缺少类型限定的编译错误，重新获得上述sidebar产品红测；均保留失败日志，不作为通过证据。

本地原始日志摘要哈希：

| 日志 | SHA-256 |
|---|---|
| navigation-red | 85158a2e5e34bd267393bfd981a34ef688f1d1fcb1f4063d865ec49528c16362 |
| navigation-shell-red-product | b257ce723693a5423ba4e223a247d9512af7c5311c1f0c9b4eb40fc7d36c90e0 |
| navigation-green-final | 16d66c6f53ce38a1ec23186c96b9382aa8cdda66a7f7ace75ad4627f2acf7536 |

## Residuals

云端PR门禁尚待主控执行。真实窗口像素和用户工作流等待合并后手测；未安装、启动或操作用户应用。无新增依赖、数据迁移或安全契约变化。
