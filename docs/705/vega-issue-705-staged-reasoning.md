# #705 运行中切换模型后的思考配置

## 目标与边界

运行中从 A 切到 B 后，Composer 可重复打开 B 的真实思考档位并保存偏好。当前运行的 A 请求、模型快照及思考冻结值不变，不停止、不重启、不热切换当前任务。沿用既有 provider/model 解析与 reasoning authority，不新增配置来源、依赖、同步 IO 或权限路径。

基线：c42148f69c9cb987901aeae56932e7ecf57a24ce。Issue：https://github.com/puzige/vega/issues/705 。

## 根因

`ConversationStream::selector_model` 返回 staged 模型，但 `on_activate_model` 和 `open_model_picker_slider` 仅允许与 `displayed_model` 相同的模型进入面板。滑块和 Composer 思考保存仍使用当前模型投影。单纯删除相等判断会造成档位或保存目标串模型。

## 实现契约

1. 菜单思考投影独立于当前模型的 `composer_defaults.reasoning`。当前模型的提交/上下文接口保持原语义；菜单仅使用与 `selector_model()` 匹配的 profile，缺失、读取失败或身份不匹配均不复用旧档位。
2. staged 模型确认成功后重新投影菜单配置；失败保留先前模型与配置。恢复 pending model、catalog 更新、reasoning 保存回执同样刷新当前所选模型。沿用 thread/request/owner 隔离。
3. 菜单分流取决于匹配的真实档位，不取决于运行模型与所选模型相等。保留关闭、重开、进入列表和键盘操作。
4. 思考变更事件携带所选模型及其 profile；保存端校验事件模型仍为当前所选模型，并依据既有唯一 provider 解析验证归属。合并中的待保存意图保留模型身份，不能在 B→C 后将 B 的档位写给 C；旧回执只更新 authority，再重投影当前选择。
5. 原模型配置仍供当前模型的上下文/提交接口使用。下一轮在现有 pending model promotion 边界重新解析 B 的已保存配置；不修改 runtime 内的已冻结请求。
6. 无档位继续进入列表；配置错误沿用现有错误恢复路径，不新增常驻解释文案或伪造默认值。

## 涉及路径

- `crates/vega_ui/src/conversation_stream/core.rs`：独立菜单投影、档位事件归属及菜单分流。
- `crates/vega/src/window/reasoning.rs`：双投影与保存目标、合并意图模型隔离。
- `crates/vega/src/window/session.rs`：staged ack 和 catalog 刷新。
- `crates/vega/src/window/render.rs`：恢复 pending model 后的菜单投影。
- `crates/vega/src/window/mod.rs`：待保存意图必要身份。

## 回归覆盖与验收

开发回归覆盖真实决策/状态边界，不用源码字符串断言替代行为。必要 Rust 开发测试仅交 Actions 执行，留存完整 SHA 与源码/结果后从最终交付 head 移除；不得进入 master。最终 head 完整 required CI 与非实现者独立审查不可省略。

| 场景 | 必须结果 |
|---|---|
| A 运行→B（不同档位）→重开 | B 的真实档位和选中值，A 的提交快照不变 |
| 修改 B→关闭→重开 | 保存 B；下一轮 B 使用保存值；A 当前任务不受影响 |
| A→B→C 与切回 A | 正确投影，旧事件/合并意图不可串写 |
| 切会话再返回 | 恢复 pending model 及对应 profile |
| 无档位/配置不可用 | 无旧档位残留，不伪装成功 |
| 模型或思考保存失败→恢复 | 保留真实状态，错误可见，恢复后可再次操作 |
| 运行完成/取消 | 既有 promotion 语义不变，下一轮模型与思考一致 |
| 空闲、仅打开列表、鼠标重复开关、键盘 | 原交互不回归 |

本地只做差异/格式/静态检查，不运行 Vega、不编译或执行测试。真实 UI 与服务执行隔离由用户手测；CI 不等于用户验收。尚未执行的项目不得标通过。

## 回滚

整体回退本卡提交，恢复原菜单分流；无 schema 或用户配置迁移，不删除用户数据。
