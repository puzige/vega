# 运行中追加消息的凭据检查事务边界

版本：v1，2026-10-24。基线：`80eaba474df5a23c3a7849e11b39a0864b7e0ad6`。关联 #581、#576。

## 场景与根因

原生运行执行工具期间追加 steer 消息，工具成功结束后，`QueueSteeringSource::take` 开启 immediate 写事务并调用 owner credential reader。reader 经 `privacy::refresh_authority` 在第二个连接调用 `refresh_epoch`，在判断 epoch 是否变化前取得 Credentials 写许可，命中同线程跨连接重入保护并返回 SQLite LOCKED。上层抹去底层原因后显示凭据不可读。

本机两次事件记录均为追加消息入队、工具成功、run `credential_check`；新一轮模型请求成功且凭据文件检查正常。SQLite 6 来自对应版本的静态调用链推导，未声称日志直接捕获。#577 首次在 v0.1.216 引入这条常态失败路径。

## 验收矩阵

| 前置与操作 | 可观察结果 | 验证层级 / 状态 |
|---|---|---|
| 有 pending steer，凭据回调通过第二连接真实刷新 authority | take 成功且消息被 claim 一次；重复 take 不重复返回 | 云端进程内回归已通过 |
| 凭据 epoch 已变，需要刷新写入 | 同库凭据写入成功，正文仍脱敏，不能只修 unchanged 快速路径 | 云端进程内回归已通过 |
| 空队列或已取消 | 不领取、不调用无用凭据回调 | 云端进程内回归已通过 |
| 凭据 reader 返回真实错误 | CredentialCheckUnavailable；消息保持 pending，无误交付 | 云端进程内回归已通过 |
| 准备期间候选被移除、payload 改变、generation 改变或被其他消费者领取 | 返回无交付，不 claim 过期候选或替代消息；新内容保留 | 云端进程内回归已通过 |
| 准备期间更早 queue 消息被提升为 steer | 重读发现 next 候选改变，本次不越序领取 | 云端进程内回归已通过 |
| 准备期间取消，或等写许可时取消 | 获取写许可后、claim 前观察到取消则不 claim；不承诺 token 与提交的原子化 | 云端 SQLite 等待回归及静态边界审查已通过 |
| 普通正文 / 引用 / 附件随追加消息交付 | 原始内容、引用展开、脱敏、图片与 seq 语义保留；provider guard 仍在发送前检查 | 云端关键路径回归与静态审查已通过 |
| 已安装修复版本，工具等待期间发送补充消息 | 当前 run 继续，无凭据误报，已完成工具不重放 | 用户手测，待验证 |

## 最小实现

只调整 `crates/vega_conversation/src/pending_messages.rs` 的 `QueueSteeringSource::take`，必要时给存储行类型增加等值比较。不开新 schema，不改 UI、模型协议、协调器或凭据存储。

1. 取消时提前返回；在无写事务状态读取同一 thread/generation 的下一条 pending steer，并保留完整候选快照。
2. 无写许可时完成 decode、附件校验、引用读取、凭据刷新及脱敏。失败仍 fail closed；异常不 claim，不重放模型或工具。
3. 开启短 immediate 写事务，重新检查取消并重新读取 next steer。严格匹配候选 id、thread、generation、behavior、ordinal、payload 及 created_at；任何变化返回无交付，让后续调用重新准备，不使用旧内容或越序领取。
4. 同一事务内 CAS claim、获取 next_seq 并提交；原有 RuntimeSteeringInput 及下游 SteeringAccepted 持久化语义不变。

所有凭据及 exception 回调、文件引用操作必须在写事务外。保留同线程跨连接重入保护，不捕获错误并接受空凭据，不增加全事件/模型/工具重试；provider 发送前已有实时凭据投影校验保持原样。锁外准备失败和事务内重验失败不得遗留 claimed 行。

## 验证与交付

本地仅格式、diff 和静态检查，不执行或编译 Vega；不得访问真实凭据、live DB、网络模型或外部应用。新增少量开发 Rust 回归使用自有临时目录、SQLite 与假凭据；真实 authority/写协调 API 不替换，竞争使用受控回调或同步信号，不依赖 sleep 猜时序。沿用现有依赖和 PR Actions，不改 CI。

开发 head 通过云端行为回归后，固定完整 SHA、源码/patch 和实际结果。按仓库政策移除开发 Rust 测试及独占接线，独立审查生产差异保持等价，最终 head 通过完整 required CI（Rust 0 发现/0 执行，no Rust tests）后合并。空集结果不是功能回归通过的替代。Issue 保持开放/In review 等用户手测。

回滚通过后续 PR revert 本卡生产修改，不更改用户数据。安装、运行应用及真实交互验收不属于本轮自动验证。

开发回归的精确提交、实际云端结果及测试源码留痕见[验证记录](vega-issue-581-validation.md)；最终 required CI 和用户手测状态分别记录。
