# 置顶后回底箭头显示条件

## 范围与契约

初始范围只修复新 turn 自动置顶后立即显示向下箭头的问题。后续用户明确授权在本卡 / PR #832 内一并修复“上滚后下不回来”的边界。保留现有置顶、上滚回收留白、尺寸变化及 resume_tail 行为，只补齐下滚到已回收留白的边界时恢复 Tail 的判定。不新增文案、控件或依赖版本。

箭头仍以非 Tail 为基础条件，但正在自动置顶（target 存在且未被既有输入逻辑 interrupt）时不显示。pending 首帧及完成布局后的自动置顶都适用。既有输入中断后不再抑制箭头，正常浏览历史保持原入口；新 turn begin 重新进入自动置顶。reset 后恢复原显示规则。不是按 active 一直隐藏，避免手动浏览历史失去入口；不改 following_tail 的含义或共享调用者。

## 已确认根因与最小方案

Actions `41301184851`、提交 `60afa0dca8ea6f206f83a0f8457c652f40c42f0a` 的探针确认：上滚 50px 后 offset=510px、spacer=150px；后续下滚的 List 回调已到达（scrolled=true），但 offset=previous=510px、bottom_distance=0。上滚回收的留白使当前位置恰好成为新最大偏移，原 `offset > previous` 无法满足。这段边界逻辑在基线已存在。

在既有滚轮 capture 路径记录本帧向下意图；消费后清空，避免旧方向污染键盘/鼠标及后续布局。恢复 Tail 仍需要原有 scrolled、底部距离、累计位移等门槛；仅将“偏移增加”扩展为“偏移增加，或明确下滚且偏移被钳在原处”。留白回收公式不变，恢复后沿用原 reset 与 Tail 路径。移除已完成诊断的测试日志，保留失败回归的所有终态断言并补上误触发和贴底几何检查。

## 验证

- 真实 render_resume_tail 生成的 scroll-to-bottom 元素：普通 Tail 隐藏；自动置顶首帧及后续布局隐藏，并保留 Normal、逻辑锚点和必要留白。
- 对真实 TurnList 派发上滚，按钮恢复显示，保留上滚回收留白；上滚后下滚到边界必须释放置顶、恢复 Tail，末条消息底边与视口底边之间仅保留 foreground，箭头消失。
- 再次上滚或零 delta 不得误触发 Tail；仅在既有 List 滚动回调已到达、padded-bottom 距离约为 0 且存在累计用户位移时，允许明确向下的滚轮意图在偏移被钳住时恢复 Tail。保留原先偏移严格增加的恢复路径。
- 未置顶的历史浏览仍显示按钮，原鼠标/键盘入口及回底语义不改。
- 新 turn 重新隐藏按钮；无新增历史恢复/重复投影副作用。#818 的入口实现保持逐字不变。

本地只做格式、差异与静态检查，自动回归在现有 Actions 执行。GPUI 无头渲染/列表回归不冒充原生桌面首帧或用户手测。Rust 回归与专用 test-support 仅供开发辅助。用户已授权留痕清理后，在最终 head required CI 通过时合并 PR #832，并跟踪自动发布；不安装应用，不关闭卡片，合并后置 In review 等待用户手测。

## 开发回归留痕与最终交付

- 已通过回归的完整提交：`8ccfba8083be031a8ef8845312db7c7f72b352ca`。
- 固定[原始回归源码](https://github.com/puzige/vega/blob/8ccfba8083be031a8ef8845312db7c7f72b352ca/crates/vega_ui/src/conversation_stream/render/arrow_tests.rs)，Git blob `9107d385acbfe84fdc5ce31e09251c5200bc9f9d`；SHA-256 `b71d15ac4ff5a908475192abfe6ec37852388a7a254d894e3fb891176b4e9c28`。清理前已由 GitHub Contents API 核验固定提交中的源码可达；不依赖临时 artifact。
- [云端 Actions 41306425098](https://github.com/puzige/vega/actions/runs/41306425098)，Nextest job `124847643050`，Clippy job `124847643129`，required check job `124848621029`，对应上述精确提交，均成功。
- 云端执行 `cargo nextest list --workspace --message-format json`、`cargo nextest run --workspace`：4 发现、4 执行、4 通过、0 跳过；三组 Python 回归分别 5 / 15 / 40 通过，格式与全目标 Clippy 通过。覆盖自动置顶箭头隐藏、真实列表锚点/留白、小步与大步下滚贴底、零滚动与继续上滚不误恢复、历史浏览焦点和按钮回底、新 turn 再隐藏。
- 已独立静态审查上述版本，无新增阻断项；原生桌面表现仍待用户手测。
- 合并清理仅移除 222 行开发回归、`mod arrow_tests`、按钮 `debug_selector` 和 dev-only GPUI `test-support` 依赖。箭头判断、方向意图及底部边界恢复生产逻辑保持不变；不改锁文件、Python、benchmark、打包或通用 CI。
- 最终无测试 head 的静态清点、增量独立审查与 required CI 结果在 PR #832 留存，尚未取得时不称通过。最终空集 `no Rust tests`（0 发现 / 0 执行）与上述 4/4 行为验证分别记录，不冒充重跑行为回归。本地仅静态检查，不运行、编译或测试 Vega。
