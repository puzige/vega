# #575 中断恢复检查点

## 第一次恢复版本

- 用户确认继续实现，口径仍为最近一次主调用的 `input + output`；不累计任务消耗，不另加缓存。
- 原规格提交 `f7a34d3b14b42aee751e646dfefdf9b45d771f87`；基线更新后规格提交为 `4e580e33d6f8637b487c8ff2ce50ff042363769f`，规格内容未改。
- 最新集成基线 `6e490aefc18fd0e8a263cbcd6b67570bcbf9348e`。新增基线只涉及 #307 onboarding，与本卡 7 个生产文件不重叠。
- 恢复时无活跃子 Agent。原实现留下 7 个未提交生产文件，尚无开发测试、PR 或本轮 Actions 证据。
- 原生产补丁及基线更新后恢复补丁 SHA-256 均为 `c8d83b0b0458a494b4bc83b7fec7005761f2f420a6b58e725f30b81408effa58`，确认恢复未丢失或重写改动。

## 已有产物

- app ingress 在既有 generation/thread/stream/model 栅栏内接入主调用 usage。
- UI 保存调用 response identity、最近调用总量及估算/真实来源，使用 checked_add；初稿处理投影、预检、压缩及 run 边界。
- 来源短标签及现有 tooltip 数据传递已修改，布局与交互未改。
- 以上仅是中断初稿，不表示实现已就绪、独立审查通过或行为已验证。

## 未决与下一步

1. 由单一实现写入者接续，不重做已有实现；复核同 message 多调用、同 revision 预检/压缩顺序、结束保留与新 run 清理。
2. 按规格添加必要开发 GPUI 回归，沿真实事件/状态边界观测；不恢复历史测试集合，不扩大生产 API。
3. 本地仅静态检查；停写后固定完整版本，独立审查及现有 draft PR Actions 验证。
4. 实际验证完成后留存原始测试源码/patch 与完整 SHA，清理开发测试和独占支持，最终 head 完整 required CI 确认 no Rust tests（0 发现/0 执行）。
5. 合并后 Issue 保持开放/In review 等用户手测。不使用 CUA，不在本地运行、编译或测试 Vega。

## 第二次恢复与依赖接线

- 接续实现子 Agent 达到单次 20 分钟执行时限，终态为 interrupted、未 READY；已停止写入。留下 6 组新的开发 GPUI 回归及其模块/feature 接线，尚无云端验证。
- 受保护的 tracked diff SHA-256 为 `361f84f0f34ea63d9d5321de20a35d81d22eca882bcb46bffe8ee75f790c7156`；原始测试文件 SHA-256 为 `8023a32e8c6e7afd05efe1f891bdcd844c38f196cf3284bcdebdf646284fa37b`。
- 再次 fetch 并安全保护产物后，更新至 `3277ebb5fd710d7d33080f91b6c5235e1558e0d3`；该增量只在 UI 的 core/message_anchors 和 #344 文档，与本卡写入路径不重叠。新的规格提交为 `b82a60feec041e30acdac19d9f98349cd8811eea`，内容未改。恢复前后 diff 与测试哈希一致。
- Parent 运行 `cargo metadata --offline --format-version 1`（20 秒上限，实际快速退出 0），只解析依赖、不编译/执行 Vega。Cargo.lock 新增 10 个 GPUI test-support 独占包；既有 package identity 无移除、版本和 checksum 未变。测试支持及其独占锁文件差异在开发验证后清理。
- 剩余实现交给单一写入者，只完成既有回归的 API/格式检查及必要修正；不继续手工推导依赖。Parent 独占锁文件和 Git/index，未开启 PR/CI。
- 本检查点不是 READY 或验证通过。下一步仍为固定停写版本、独立审查与现有 Actions；不以元数据解析冒充编译或行为测试。

## 停写静态交付

- 第三次执行只完成既有产物的静态 API 核对、单文件格式化与无预算下一调用回归，没有重做生产行为。实现者已 READY 并停止写入。
- 6 组开发测试覆盖首次总量向上/向下校准、缓存不重复、同调用去重、同 message 下一调用下降、迟到估算、压缩与摘要/子任务隔离、无效/错误归属、结束保留、新 run/模型重置、冷投影和未知预算。
- 新增无预算时的第二 response id：150000 可替换为 100000，分母仍未知；结束和投影不覆盖该最近总量。
- 实现者对 9 个改动 Rust 文件执行 `rustfmt --edition 2024 --config skip_children=true --check` 均退出 0，`git diff --check` 退出 0；未新增代码注释或非测试 unwrap/expect。初次格式化 shell 包装使用 zsh 只读变量 `status` 失败，已修正包装后完成检查，不属于产品故障。
- 停写时 tracked diff（含 Parent 锁文件）SHA-256 为 `637fc617e3de46ba5e3222ea01d5b9852a288e51d20f8fd4d0150296b8a51826`；测试文件 SHA-256 为 `c981b1901884de2aa0a50ac7f2a596abefcfe1e730cef4c20912b16330637379`。文档更新另记，不把该哈希当作最终提交。
- GPUI 回归不覆盖 app generation/thread/stream/model 栅栏；该部分单独静态审查。usage 不带 response id 的协议边界保持不变，不能声称鉴别任意同 message 乱序旧事件。
- 编译、Clippy、Rust/Python 测试与 required check 均待现有 Actions；未执行 UI/CUA 或本地 Vega 代码。
