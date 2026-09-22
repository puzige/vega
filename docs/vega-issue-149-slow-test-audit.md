# Issue #149 后续 Git 依赖隔离分诊（只读）

基线：master da4cc484a64e811ffe829d9a34a1d9276ff3123e；证据 final-cloud/full.log。共70项 >2s trusted_git测试，累加543.863s；是逐测试耗时总和，不是流水线wall时间或承诺可节省时间。初始分诊阶段未改代码或运行新测试。其余57项分诊及最高两项拆分决定已补在本文后半部分；实施状态与测量结果见Issue149规格和PR。首批9场景完成不代表全体127项或整体优化完成。

## 分类含义和边界

- A：现有GitCommandBackend+staged raw fixture可直接或很小扩展复用；优先交付。
- B：可迁移业务规则，但需要有限mutation/read failure/同步事件脚本；不是现有只读stub已经支持。
- M：一个测试混合业务规则与真实Git/文件系统语义，先分清断言职责；不能整项mock或删除。
- R：目前应保留真实适配/语义验证。>2s是审查阈值，不是忽略或迁移命令。

| 分类 | 项数 | 累加秒数 |
|---|---:|---:|
| A | 3 | 18.340 |
| B | 39 | 270.765 |
| M | 5 | 64.327 |
| R | 23 | 190.431 |

## 下一批建议

### 快速第一组：只读业务保护（不等待一次提交fake扩展）

1. filter_gitlink::capture_head_service_rejects_bad_born_oids_before_any_mutation（8.621s，5种坏OID）。已有代码只是用shell篡改status，不是真实Git生成异常OID；用真实workspace.refresh创建初态后切换status原始bytes即可。保留MalformedOutput、零mutation、终态snapshot不变。不要改workspace缓存或让fake直接返回最终错误。
2. summary_draft::failed_draft_keeps_prepared_authority_usable（7.309s）。复用staged raw fixture，真实prepare后执行失败provider→成功provider；保留DraftFailed、恢复文案、同一个prepared.id仍存在、mutation_active=false、零Git mutation/stdin。无需新增外部结果种类。
3. selection_topology::disconnected_recovery_consumes_zombie_owner_before_future_checklist（2.410s）。真实begin_owned_refresh与recover_disconnected_mutation，固定raw状态切换即可；保留generation推进、owner释放、mutation_active清零和后续checklist成功。不得手工伪造恢复结果。

这3项累计18.340s，是最小风险可立即实施的业务组，尚无迁移后计时。保留现有真实empty-selection/status-drift/argv-stdin集成。

summary_draft::commit_draft_request_matches_frozen_literals_for_both_truncation_flags（9.087s）也属业务规则，但当前测试直接修改stored.summary和summary_truncated。迁移时应明确这些只是请求构造输入：优先从raw summary/overflow驱动真实prepare，若冻结文案输入不能自然生成则用窄内部请求builder测试并保留一个service端到端代表。不得制造prepared成功结果，更不能仅把旧手工状态更改搬入fake后宣称整条准备路径被验证。保留model/tools/max_tokens/两条消息/完整system和user字面量/一次provider请求。

### 最大有界组：11种 commit-proof 故障

累计80.818s。共享helper commit_proof.rs:65，都是一次真实commit后由shell脚本篡改父对象/tree/ref读取再检查应用是否fail closed。重复建仓库和shell注入不是每项业务断言的必要条件；这是下一批收益潜力最大的同质组。精确名单如下（不是新增测试列表）：

| 测试 | 秒 | 返回契约 |
|---|---:|---|
| `commit_proof_rejects_mixed_parent_after_one_commit` | 11.415 | `MalformedOutput` |
| `commit_proof_rejects_two_parent_after_one_commit` | 8.594 | `ChangedDuringRead` |
| `commit_proof_rejects_ref_moved_after_one_commit` | 8.046 | `ChangedDuringRead` |
| `commit_proof_rejects_tree_diff_after_one_commit` | 7.552 | `ChangedDuringRead` |
| `commit_proof_rejects_malformed_parent_after_one_commit` | 6.991 | `MalformedOutput` |
| `commit_proof_rejects_object_missing_after_one_commit` | 6.844 | `GitFailed` |
| `commit_proof_rejects_ref_deleted_after_one_commit` | 6.822 | `ChangedDuringRead` |
| `commit_proof_rejects_ref_renamed_after_one_commit` | 6.789 | `ChangedDuringRead` |
| `commit_proof_rejects_wrong_parent_after_one_commit` | 6.337 | `ChangedDuringRead` |
| `commit_proof_rejects_zero_parent_after_one_commit` | 5.968 | `ChangedDuringRead` |
| `commit_proof_rejects_short_parent_after_one_commit` | 5.460 | `MalformedOutput` |

最小backend扩展：

- 保持GitCommandBackend接口，不增加新生产公共API。给测试stub一个明确的一次mutation expectation，精确匹配commit argv与完整stdin；返回现有Output或外部错误后切换到一份已捕获的post-commit原始读取集。opaque prepared依旧由真实refresh→checklist→prepare产生。
- 扩展仅prove_commit实际需要的原始读取：明确new OID、new_oid^@父列表、new_oid的tree、最后refs/status；为每种故障覆盖某个raw响应或返回GitFailed。不要在fake计算合法parent/tree关系或直接返回CommitOutcome，它们必须由真实prove_commit判断。
- 精确命令协议可以断言，但不要给所有无关read固定次数/顺序。ref最终检查若需要事件切换，使用明确proof阶段/一次性匹配规则，不做通用Git状态机，不通过任意第N个status来模拟复杂仓库。
- 每项保留原typed failure、workspace Some、恰好一次commit argv/attempt、duplicate StaleAuthority与仍然一次commit。未知请求必须记录并使测试失败，防止GitFailed掩盖缺失fixture响应。
- object_missing的业务映射可以用backend返回GitFailed；但真实对象暂移/恢复导致实际Git失败的适配契约至少留一个集成代表，不能把两者混称。本组80.818s不是可全部删除的时间；如果保留原object_missing集成，其6.844s会保留。

真实adapter保留：commit_proof_uses_explicit_new_oid_for_born_and_unborn_commits（15.801s）验证真实新提交OID/父关系/tree/ref；trusted_git_mutations_use_exact_argv_and_in_memory_stdin（6.768s）验证真实进程bytes；commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit（8.117s）是本轮特意增加的真实漂移代表，不能因为又>2s就再删。保留object-missing实际错误与root inode/目录替换的文件系统覆盖。旧11种逐项均保留业务断言，只隔离故障生成机制。

验证：两类以上negative control（接受错误parent/tree、重复消费prepared/允许额外commit）必须使迁移测试失败；同机before/after；sandbox-exec禁止所有Git/Shell仍通过；正常真实adapter回归；最终cloud全量门禁。不要靠增加测试名称/分片来冒充减少执行量。

### 后续组（不要塞进第一批）

- filter_gitlink的显式filter4值、gitattributes当前/rename旧路径、attrs三屏障：分别验证UnsafeFilter和ChangedDuringRead、zero/zero/one add、owner释放。旧脚本只override trusted读取，而新backend由workspace和trusted共享；直接让所有check-attr返回坏值会破坏初始/terminal refresh，导致错误测试。需要明确有界的read域/phase响应，保留workspace正常真实解析与一次add后的B raw数据。不要以放宽terminal断言绕过。
- commit_message_byte_bounds_and_exact_stdin_are_enforced（19.314s）：空/NUL/ASCII+1/UTF8字节边界属于业务；成功exact/newline路径还测真实stdin与commit。待一次mutation backend成熟后迁移业务组合，同时保留实际stdin代表。
- owned_prepare两个ABA/ordinary-poll测试以及owner_refresh两个恢复测试：属于owner/generation业务，适合channel握手与固定A/B raw快照；真实后端错误产生/进程回收另有集成。不要继续真实sleep或造Git reset模拟器。
- runner_mutation的20项service_*是错误映射与权威恢复；可逐组用backend错误/一次mutation后状态取代脚本。固定3s wait场景应返回外部TimedOut事件，实际超时/排空/信号边界保留trusted_mutation_runner_*集成。此扩展涉及cancel与mutation后快照，不应无准备地批量替换。

## 最高耗时两项为何不整体迁移

filter_gitlink::empty_blob_add_worktree_delete_and_staged_empty_delete_remain_distinct（28.817s，:686）验证空blob≠不存在、worktree删除与index空blob的区分、选择/不选择删除后的真实ls-files终态；fake若自己实现这些Git行为只是在重复答案。保留实际Git代表；必要时仅提取UI/业务分类组合，不能删真实语义。

selection_noop::clean_and_normalized_noop_are_no_staged_changes_without_commit（24.254s，:74）混合clean空选择、core.filemode=false、CRLF经.gitattributes规范化、选择集外并发漂移。clean业务可用静态raw，CRLF/属性/mode语义必须真实Git验证。已有ready/release屏障，无24秒固定sleep可直接删。

real_gitlink（15.025s）真实submodule clone/clean/dirty/union语义、SHA256（18.542s）对象格式与Git版本、rename/type/mode选择拓扑和raw路径argv契约也保留真实集成。可以优化fixture准备，但不能把外部算法复制到fake。

## 全部70项逐项分诊

源码位置相对于 crates/vega_conversation/src/git_workspace/trusted_git/tests/。A/B仅表示迁移候选，M/R均有必须保留的真实契约；所有条目目前仍运行，未改频率。

| 测试 / 源码 | 秒 | 类 | 判断与保留范围 |
|---|---:|---|---|
| `filter_gitlink.rs:686` `empty_blob_add_worktree_delete_and_staged_empty_delete_remain_distinct` | 28.817 | R | 空blob/删除差别或真实submodule/gitlink语义，留Git adapter；不整项替换 |
| `selection_noop.rs:47` `clean_and_normalized_noop_are_no_staged_changes_without_commit` | 24.254 | M | clean空选可迁移；CRLF规范化/core.filemode与集外漂移依赖真实Git，拆断言职责保留语义集成 |
| `commit_proof.rs:301` `commit_message_byte_bounds_and_exact_stdin_are_enforced` | 19.314 | M | 空/NUL/byte边界业务可迁移；精确stdin与实际commit至少保留代表 |
| `codec_topology.rs:458` `sha256_repository_completes_checklist_prepare_and_commit` | 18.542 | R | 真实SHA256 Git版本/对象格式及完整提交契约 |
| `commit_proof.rs:4` `commit_proof_uses_explicit_new_oid_for_born_and_unborn_commits` | 15.801 | R | born/unborn实际新OID proof、真实status漂移代表或intent-to-add隐藏删除Git语义 |
| `filter_gitlink.rs:561` `real_gitlink_is_allowed_only_as_exact_clean_unchanged_union_entry` | 15.025 | R | 空blob/删除差别或真实submodule/gitlink语义，留Git adapter；不整项替换 |
| `filter_gitlink.rs:495` `attrs_drift_at_immediate_final_and_post_add_barriers_has_zero_zero_one_add` | 11.812 | B | 三处业务屏障与zero/zero/one add可用有限phase事件；post-add需backend mutation响应 |
| `commit_proof.rs:156` `commit_proof_rejects_mixed_parent_after_one_commit` | 11.415 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `runner_mutation.rs:596` `service_add_process_failure_wait` | 11.069 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:686` `service_cancel_after_real_add_or_commit_returns_authoritative_state_once` | 10.598 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `filter_gitlink.rs:361` `prepare_maps_every_explicit_filter_value_to_unsafe_filter_before_add` | 10.144 | B | 四种check-attr bytes拒绝规则；需区分workspace正常读取和trusted故障读取，不能全局污染terminal refresh |
| `runner_mutation.rs:601` `service_commit_process_failure_wait` | 9.929 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `selection_topology.rs:524` `staged_rename_destination_delete_claims_only_canonical_old_deletion` | 9.354 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `summary_draft.rs:610` `commit_draft_request_matches_frozen_literals_for_both_truncation_flags` | 9.087 | M | 请求构造纯业务；当前直接改prepared summary/truncated，应从原始summary输入或窄builder输入验证，保留service代表 |
| `selection_topology.rs:4` `trusted_git_empty_selection_commits_existing_staged_delta` | 8.717 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `runner_mutation.rs:676` `service_commit_stderr_overflow` | 8.665 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `filter_gitlink.rs:29` `capture_head_service_rejects_bad_born_oids_before_any_mutation` | 8.621 | A | workspace初始refresh后仅改坏status raw；真实head parser拒绝/终态不变/零mutation |
| `commit_proof.rs:129` `commit_proof_rejects_two_parent_after_one_commit` | 8.594 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `selection_topology.rs:34` `e2e_owned_repo_checklist_prepare_mock_draft_commit` | 8.570 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `selection_topology.rs:216` `trusted_git_selected_am_component_preserves_forced_add_topology` | 8.266 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `runner_mutation.rs:621` `service_commit_stdout_exact` | 8.149 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `commit_proof.rs:270` `commit_status_drift_real_git_consumes_prepared_and_spawns_zero_commit` | 8.117 | R | born/unborn实际新OID proof、真实status漂移代表或intent-to-add隐藏删除Git语义 |
| `commit_proof.rs:167` `commit_proof_rejects_ref_moved_after_one_commit` | 8.046 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `runner_mutation.rs:606` `service_add_process_failure_inherited_pipe` | 7.631 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `commit_proof.rs:135` `commit_proof_rejects_tree_diff_after_one_commit` | 7.552 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `selection_topology.rs:136` `owner_refresh_commit_first_capture_failure_recovers_new_head_once` | 7.336 | B | 有限mutation→首次read错误→恢复事件，验证相同owner/generation；保留真实恢复代表 |
| `summary_draft.rs:664` `failed_draft_keeps_prepared_authority_usable` | 7.309 | A | 复用staged原始读取fixture与MockProvider，保留真实prepare/draft/opaque ID |
| `runner_mutation.rs:636` `service_add_pre_mutation_nonzero_before` | 7.182 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `commit_proof.rs:141` `commit_proof_rejects_malformed_parent_after_one_commit` | 6.991 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `commit_proof.rs:162` `commit_proof_rejects_object_missing_after_one_commit` | 6.844 | M | 业务GitFailed映射可进入proof stub组，但保留一例真实对象暂移/恢复及读取失败集成 |
| `commit_proof.rs:173` `commit_proof_rejects_ref_deleted_after_one_commit` | 6.822 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `runner_mutation.rs:611` `service_commit_process_failure_inherited_pipe` | 6.813 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `selection_topology.rs:103` `owner_refresh_prepare_first_capture_failure_retries_exact_owner` | 6.803 | B | 有限mutation→首次read错误→恢复事件，验证相同owner/generation；保留真实恢复代表 |
| `commit_proof.rs:179` `commit_proof_rejects_ref_renamed_after_one_commit` | 6.789 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `runner_mutation.rs:4` `trusted_git_mutations_use_exact_argv_and_in_memory_stdin` | 6.768 | R | 真实argv/stdin、输出限制、信号/排空/进程回收适配契约 |
| `commit_proof.rs:370` `owned_prepare_accepts_exact_b_published_by_ordinary_poll` | 6.695 | B | owner/generation ABA业务并发；channel+固定A/B snapshots，不模拟Git reset |
| `commit_proof.rs:419` `owned_prepare_rejects_a_to_b_to_a_without_capability` | 6.622 | B | owner/generation ABA业务并发；channel+固定A/B snapshots，不模拟Git reset |
| `selection_topology.rs:619` `trusted_git_selected_executable_add_binds_exact_worktree_mode` | 6.343 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `commit_proof.rs:123` `commit_proof_rejects_wrong_parent_after_one_commit` | 6.337 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `selection_topology.rs:414` `trusted_git_selected_staged_rename_with_unstaged_edit_proves_structural_split` | 6.309 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `summary_draft.rs:722` `summary_authority_change_after_capture_fails_before_provider` | 5.988 | B | summary返回后原始authority切换，以有界channel屏障替代外部脚本 |
| `commit_proof.rs:117` `commit_proof_rejects_zero_parent_after_one_commit` | 5.968 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `selection_topology.rs:279` `selected_delete_and_untracked_destination_may_canonicalize_to_staged_rename` | 5.940 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `selection_topology.rs:247` `untracked_entry_is_optional_only_and_prepares_as_added` | 5.821 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `runner_mutation.rs:591` `service_commit_process_failure_stdout_overflow` | 5.813 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `selection_topology.rs:592` `trusted_git_selected_regular_to_symlink_binds_type_change` | 5.769 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `filter_gitlink.rs:418` `selected_current_or_rename_old_gitattributes_is_zero_add_unsafe_filter` | 5.626 | B | 路径/旧rename路径安全策略可读固定raw状态；保留一例真实.gitattributes解释 |
| `selection_noop.rs:172` `selected_awkward_raw_paths_use_one_sorted_nul_stdin_and_no_path_argv` | 5.561 | R | 原始路径argv/stdin适配或特意保留的真实空选代表 |
| `runner_mutation.rs:641` `service_commit_pre_mutation_missing` | 5.538 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:651` `service_commit_pre_mutation_nonzero_before` | 5.472 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `commit_proof.rs:150` `commit_proof_rejects_short_parent_after_one_commit` | 5.460 | B | 已有shell注入proof raw故障；一次mutation后切换有限读取结果，真实prove_commit判定 |
| `runner_mutation.rs:616` `service_add_stdout_exact` | 5.431 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:661` `service_add_stderr_overflow` | 5.427 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:581` `service_commit_process_failure_nonzero` | 5.418 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:671` `service_commit_stderr_exact` | 5.408 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `selection_topology.rs:444` `staged_rename_destination_mode_flip_is_rejected_after_one_add` | 5.319 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `runner_mutation.rs:656` `service_add_stderr_exact` | 5.212 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `selection_topology.rs:483` `staged_rename_source_recreation_is_not_owned_by_destination_edit` | 5.191 | R | 真实Git add/rename/type/mode/commit行为或全链代表；不把Git算法写入fake |
| `runner_mutation.rs:586` `service_add_process_failure_stdout_overflow` | 5.033 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:626` `service_add_pre_mutation_missing` | 4.988 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:576` `service_add_process_failure_nonzero` | 4.923 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `runner_mutation.rs:646` `service_commit_pre_mutation_pre_cancel` | 4.836 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |
| `commit_proof.rs:185` `commit_proof_rejects_root_identity_swap_after_exactly_one_commit` | 4.828 | M | 真实root inode/目录替换必须保留filesystem；Git提交可有限stub但非本轮最小组 |
| `runner_mutation.rs:142` `trusted_mutation_runner_times_out_cancels_and_reaps_process_groups` | 4.149 | R | 真实argv/stdin、输出限制、信号/排空/进程回收适配契约 |
| `selection_noop.rs:4` `trusted_git_empty_selection_spawns_zero_add` | 4.056 | R | 原始路径argv/stdin适配或特意保留的真实空选代表 |
| `commit_proof.rs:464` `trusted_git_rejects_intent_to_add_and_hidden_delete_form` | 2.980 | R | born/unborn实际新OID proof、真实status漂移代表或intent-to-add隐藏删除Git语义 |
| `runner_mutation.rs:213` `trusted_mutation_runner_drains_floods_while_writing_large_stdin` | 2.645 | R | 真实argv/stdin、输出限制、信号/排空/进程回收适配契约 |
| `selection_topology.rs:179` `disconnected_recovery_consumes_zombie_owner_before_future_checklist` | 2.410 | A | 已有raw状态切换可覆盖owner释放与新checklist；不需真实Git |
| `runner_mutation.rs:52` `trusted_mutation_runner_enforces_spawn_cancel_exit_and_output_caps_for_add_and_commit` | 2.371 | R | 真实argv/stdin、输出限制、信号/排空/进程回收适配契约 |
| `runner_mutation.rs:631` `service_add_pre_mutation_pre_cancel` | 2.230 | B | 服务层error→权威终态/zero-one attempt/消费规则；脚本故障可变为有限backend结果，OS错误产生留runner集成 |

执行跟踪：Issue #149。其他57项仍待分诊，不能把本报告当作整体完成或全量迁移承诺。

## 最高两项的拆分决定（实施前更新）

进一步审查后，两项最高耗时测试可以拆分，前文的整项保留建议不再作为实施结论：

- **空 blob / 删除（28.817s）**：四个业务场景改为固定原始 Git 输出驱动真实 checklist/prepare/commit。一个小型真实 Git 契约验证空 blob、工作区缺失、索引缺失的区别，以及 add/commit 后的真实索引和树。业务断言验证请求与权威状态，不能让替身返回预期 ls-files 答案来代替真实契约。
- **无变化 / 归一化（24.254s）**：clean、忽略 mode、CRLF 归一化后无变化、选择集外漂移四分支改为固定输入和 add 后状态切换。真实 Git 契约保留 core.filemode 与 text/eol 规范化输出；真实业务代码仍判定 NoStagedChanges/ChangedDuringRead，保留零/一次 add、零 commit、无 prepared、终态权威断言。

## 其他 57 项慢测试

累计 308.136s，是候选成本池，不是可全部省去的时间。结合模块边界，后续先关注 Artifact 的重复 Git 准备、workspace/branch 的 generation 与 lease 业务，再处理需要跨 crate 边界的 UI 测试。真实 HTTP 的 15.2s 延迟可进一步拆成虚拟时钟业务与真实网络契约；不能直接冻结时钟后宣称网络集成等价。

| 分组 | 项数 | 累计秒数 | 下一步 |
|---|---:|---:|---|
| UI controllers/layout | 14 | 82.576 | 保留真实 GPUI action/focus；检查有序 worker completion 边界，避免新建通用 UI 工厂 |
| Artifact library | 9 | 73.564 | 保留真实文件与路径安全断言，替换重复 Git capture 和 launcher 业务结果 |
| Workspace snapshot/lifecycle | 12 | 47.237 | 优先 generation/race 策略；10.522s 实际进程超时契约保留 |
| Other | 6 | 45.584 | 分离 provider deadline 的时间与网络契约；S6 完整链、markdown 算法另行量化 |
| UI agent concurrency | 7 | 30.154 | 使用已有 MockProvider 显式握手；检查无关 Git 初始准备 |
| Branch library | 9 | 29.021 | lease/generation/guard 策略用固定输入；保留实际 switch/ref/argv 契约 |
## Per-test disposition (all57)

| # | 秒 | 完整测试名 | 处置/具体下一步 |
|---:|---:|---|---|
|1|23.832|`vega::bin/vega::tests::commit_panel::commit_app_production_handlers_reconcile_before_release_across_close_and_routes_s6_controller`|23.832s主业务为close/route/reconcile/release；用已有worker结果边界注入有序completion(保持owner/fence真逻辑)，只保留一次真实commit到UI刷新契约。跨crate缺backend通路先列最小访问改动。|
|2|18.928|`vega_conversation::s6_acceptance::agent_diff_artifact_dirty_reject_and_two_stage_commit`|保留一条S6真实agent→diff→commit链；把dirty拒绝/重复状态分支迁出到service业务fixture；先给内部步骤计时再分配18.928s。|
|3|18.025|`vega_conversation::artifact::tests::preview_open::artifact_preview_public_api_exact_and_plus_one_boundaries`|优先迁移Git捕获准备；7个真实文件边界/UTF8/typederror全部保留，直接真实文件读取不mock。每轮capture反复Git才是可去开销。|
|4|15.701|`vega_conversation::provider_settings::tests::production_cancel_and_total_deadline`|真实HTTP服务延迟15.2秒是主要原因；拆真实loopback取消adapter和虚拟时钟deadline业务，明确自动advance与网络IO握手，不能直接start_paused误超时。|
|5|14.670|`vega_conversation::artifact::tests::preview_open::open_in_preflight_is_zero_attempt_and_failures_are_one_attempt`|拆预检/错误映射业务和真正timeout+reap；前者用launcher记录调用，后者保留真实子孙进程。不要把14.670s全部承诺省掉。|
|6|10.522|`vega_conversation::git_workspace::tests::lifecycle::git_workspace_read_timeout_is_typed_and_bounded`|保留真实进程定时/子孙reap；现有Repo::new可换plainowned目录(被替代script不读Git)，先量化仅准备收益；10秒timeout不可mock掉。|
|7|9.415|`vega::bin/vega::tests::commit_panel::commit_panel_real_key_handlers_are_scoped_and_first_wins`|保留真实GPUI键盘scope/首个action胜出，panel所需checklist用受控worker结果；真实Git mutation另契约。|
|8|8.857|`vega_conversation::artifact::tests::preview_open::open_in_uses_six_exact_raw_argv_forms`|6种argv组合用记录backend校验真实组装，保留一条真实exec argv/环境对照；Git卡片准备另用rawfixture。|
|9|8.560|`vega_conversation::artifact::tests::preview_open::open_in_symlink_segment_hardlink_special_and_root_swap_are_zero_attempt`|保留真实symlink/hardlink/FIFO/root-swap文件事实；替换4次Gitrepo初始化/capture，仍断言launcher零调用。|
|10|7.908|`vega_conversation::artifact::tests::preview_open::artifact_preview_is_bounded_utf8_no_nul_and_path_classified`|真实字节/文件类型读取保留，Gitprojection录制，复用单fixture状态避免多次capture。|
|11|7.265|`vega::bin/vega::tests::artifact_preview::artifact_controller_preview_open_latest_stale_and_max_fences`|latest/stale/max fencing业务注入有序结果，保留真实UIcontroller与filelimit入口一条；不伪造被判定的owner/id。|
|12|5.963|`vega::bin/vega::tests::palette::production_root_palette_escape_preserves_composer_and_settings_action`|纯palette/focus/route行为；去除不必要Gitrepo初始化候选，保持实际mountedroot/actions，先核对初始环境读取。|
|13|5.652|`vega::bin/vega::tests::artifact_terminal::artifact_controller_terminal_refresh_captures_and_bash_reconciles_downgrade`|名字bash但无shell执行；Git/Artifact capture与downgrade。复用库级rawfixtures/workercompletion，保留真实降级策略。|
|14|5.296|`vega::bin/vega::window::navigation::tests::r14_current_head_loads_without_selector_click_and_refreshes_hidden_sidebar`|head载入/hidden sidebar刷新业务以worker受控结果重放，实际HEAD adapter一条保留；crosscrate注入尚需小访问边界。|
|15|5.237|`vega::bin/vega::tests::agent::issue67_concurrent_b_preprovider_failure_can_retry`|B启动前失败+恢复可重试；保留真实owner/路由/凭据前置，MockProvider门闩代替延时，去无关Gitsetup。|
|16|5.171|`vega::bin/vega::tests::agent::issue67_concurrent_stop_b_only`|两provider显式进入后只cancel B，保留A继续/独立事件；减少真实Gitartifact刷新非本断言部分。|
|17|5.100|`vega::bin/vega::tests::agent::issue67_concurrent_production_new_thread_enters_before_origin_finishes`|MockProvider受控进入/结束顺序替代预设sleep；真实两个任务controller不替换。|
|18|4.999|`vega_conversation::git_workspace::branch::tests::lease_cleanup::rejected_execute_cannot_compete_with_owner_cleanup_refresh`|backend显式gate lease/cleanup顺序；拒绝执行不得抢lease，保留authority发布。|
|19|4.965|`vega::bin/vega::tests::artifact_terminal::artifact_controller_real_batch_pairing_conflict_overflow_and_route_cancel`|pairing/conflict/overflow/cancel是业务矩阵，复用真实controller+定序workercompletion；file/Git适配最少一条真实链。|
|20|4.901|`vega::bin/vega::tests::agent::issue67_concurrent_finishes_under_settings_without_route_change`|providercompletion+settings route fence业务，不需真实shell；先隔离无关Git后台刷新。|
|21|4.889|`vega_conversation::artifact::tests::capture_reconcile::artifact_rename_tracks_raw_path_and_delete_disables_actions`|录制rename/delete投影跑真实卡片归属和禁用逻辑；小真实Git契约校验rename原始路径事实。|
|22|4.867|`vega_conversation::git_workspace::tests::snapshot::git_workspace_delete_rename_space_and_literal_magic_names`|业务投影用录制raw；原始NUL/空格/pathspec解释留一个实际Gitargv契约。|
|23|4.681|`vega_conversation::git_workspace::branch::tests::switch_e2e::newer_permit_invalidates_older_and_target_move_fails_before_switch`|fixture refs变化+permit轮换，assert零switch；保留真实ref更新小契约。|
|24|4.438|`vega_conversation::git_workspace::tests::snapshot::git_workspace_unborn_detached_nonrepo_and_stale_ids_are_typed`|拆4种状态：raw返回/unborn/detached/notrepo错误供业务typed映射，真实探测各状态小契约不重复全service。|
|25|4.321|`vega_conversation::artifact::tests::capture_reconcile::artifact_provenance_downgrades_once_and_aba_does_not_upgrade`|优先fake A→B→A捕获序列，保留降级一次/不升回断言；真实文件指纹继续读取。|
|26|3.755|`vega_conversation::git_workspace::tests::lifecycle::git_workspace_latest_refresh_wins_without_stale_overwrite`|**已迁移(batch5)** 以backend channel确定完成顺序替代shellmkdir/sleep gate，保留latest胜出；同范围3.290s→0.032s(3项)，保留真实adapter 0.378s。|
|27|3.752|`vega_conversation::git_workspace::tests::snapshot::git_workspace_private_content_head_and_raw_rename_rotate_ids`|用定序raw/content变化保留每种generation旋转，真实内容指纹不mock。|
|28|3.729|`vega_markdown::stream::tests::ten_thousand_line_document_keeps_cache_bounded_and_linear`|这是48k增量的真实算法/内存界契约，不能换假parser；保留大样本，检查doc生成/重复parse开销并profile，不降低规模掩盖复杂度。|
|29|3.657|`vega_conversation::git_workspace::tests::lifecycle::git_workspace_owner_finalize_fences_pre_registered_poll_completion`|**已迁移(batch5)** backend gate控制poll/finalize顺序，保留owner fence及最终generation；无需Git进程。|
|30|3.616|`vega::bin/vega::tests::agent::issue67_production_routes_keep_one_background_run_and_origin_stream`|保留真实route/stream归属，定序provider events，去重复repo准备；不mock active run判定。|
|31|3.526|`vega::bin/vega::tests::commit_controller::commit_controller_same_id_entity_aba_is_stale_and_worker_recovers_authority`|真实entity ABA/ownership判定+受控完成事件，避免为每个ABA场景真commit；保留worker恢复adapter对照。|
|32|3.497|`vega::bin/vega::tests::agent::issue67_concurrent_stop_a_only`|对应stopB反向断言，真实cancel隔离+受控provider gate，清理同组Gitfixture。|
|33|3.486|`vega_conversation::git_workspace::branch::tests::snapshot_ids::stale_permit_after_generation_rotation_does_not_leak_mutation_lease`|rawgeneration变化触发stalepermit，后续owner还能获取lease；无进程业务。|
|34|3.479|`vega_conversation::git_workspace::tests::snapshot::git_workspace_clean_staged_unstaged_untracked_and_structured_projection`|4个raw状态业务映射迁移；实际Git状态/codec一次契约验证。|
|35|3.477|`vega_conversation::git_workspace::branch::tests::lease_cleanup::refresh_registered_before_owner_cannot_commit_after_lease_acquisition`|channel控制注册/lease时序，assert旧refresh不能发布；真实switch adapter另保留。|
|36|3.355|`vega::bin/vega::window::navigation::tests::navigation_coalesces_attributes_bounds_history_and_rejects_stale_result`|保持coalescing/bounds/history/stale逻辑，backend/workergate替代Git等待；真实attrs读取小对照。|
|37|3.265|`vega::bin/vega::tests::diff::diff_controller_worker_preserves_unchanged_generation_and_rejects_stale_file`|相同投影/新投影顺序喂真实worker/controller；保持generation及stale文件拒绝，实际diff解析单独真实契约。|
|38|3.244|`vega_conversation::artifact::tests::capture_reconcile::artifact_strict_success_duplicate_and_non_candidates`|成功/重复/noncandidate全保留真实判定，以相同rawprojection消除重复Git；unexpected mutation拒绝。|
|39|3.164|`vega_conversation::git_workspace::branch::tests::switch_e2e::dirty_and_operation_races_are_zero_switch_with_owner_cleanup`|真实operation marker文件+rawdirty变化；断言0switch及cleanup，不需真实Git。|
|40|3.090|`vega_conversation::artifact::tests::capture_reconcile::artifact_rename_old_path_collision_never_binds_replacement`|rawrename+旧路径重建fixture跑所有权拒绝，保留真实pathidentity检查；真实rename事实单独契约。|
|41|3.026|`vega_conversation::git_workspace::tests::snapshot::git_workspace_identical_refresh_retains_generation_and_opaque_ids`|相同raw响应连续两次，真实service必须保持generation/id；立即适配已有backend。|
|42|2.865|`vega::bin/vega::tests::r69::r69_a7_project_draft_lists_and_switches_without_materializing`|项目draft/list/switch业务检查不落盘；去除不必要Git准备并记录每个创建入口调用，保留真实store，不mock materialization结果。|
|43|2.721|`vega_ui::sidebar::projects_block::tests::production_sidebar_refreshes_real_checkout_and_rejects_removed_results`|拆sidebar latest/remove业务与真实checkout事实；现有crosscrate backend不可直接注入，先重用workergate/最小repo，需单独接口范围审查。|
|44|2.632|`vega::bin/vega::window::workspace::terminal_tests::r44_terminal_entry_points_and_creation_menu_preserve_explicit_focus`|本轮保留真实PID/写文件契约；先拆纯focus/UI业务与进程持续性，再考虑跨crate session seam；不是直接改fakePID。|
|45|2.632|`vega::bin/vega::tests::agent::issue67_concurrent_settings_fails_closed_for_both_permissions`|settings下两个permission均failclosed，真实permission/controller判定；零mutation必须由请求记录证明。|
|46|2.606|`vega_conversation::git_workspace::branch::tests::switch_e2e::safe_temp_repo_switch_is_exact_and_authoritatively_refreshed`|保留代表真实switch argv/ref终态；可删重复helper初始化，不能用fake结果自证actualswitch。|
|47|2.497|`vega_conversation::s6_acceptance::clean_fixture_branch_switches_authoritatively`|与库safe switch对照检查重复的argv/ref责任；保留最低一条跨层真实switch，业务刷新fixture覆盖其他情况。|
|48|2.463|`vega_conversation::git_workspace::tests::snapshot::git_workspace_binary_symlink_and_special_are_metadata_only`|保留真实binary/symlink/special文件和围栏，stub无关Gitmetadata读取；不mock文件分类结果。|
|49|2.460|`vega_conversation::git_workspace::tests::snapshot::git_workspace_aba_allocates_fresh_generation_without_id_revival`|raw A→B→A；断言旧ID绝不复活，优先无进程业务迁移。|
|50|2.430|`vega_conversation::git_workspace::tests::snapshot::git_workspace_clean_info_attributes_change_rotates_generation`|真实info/attributes文件变化保留，rawcapture相同；断言attrs变化仍旋转generation。|
|51|2.388|`vega_conversation::git_workspace::tests::lifecycle::git_workspace_obsolete_failure_does_not_invalidate_newer_snapshot`|**已迁移(batch5)** backend延迟旧请求返回错误，新请求先完成；保留新snapshot有效，无需shellgate。|
|52|2.292|`vega::bin/vega::tests::branch::branch_controller_close_cancels_owner_but_releases_only_after_cleanup`|channel暂停真实cleanupcompletion，assert先cancel后release，不靠shell忙等；真实branch cleanup另测试。|
|53|2.280|`vega_conversation::git_workspace::branch::tests::snapshot_ids::opaque_ids_are_service_generation_slot_and_seal_bound`|录制相同投影创建独立service/epoch，保留跨service/slot/seal拒绝，不需重复Gitrepo。|
|54|2.268|`vega_conversation::git_workspace::branch::tests::state_guards::staged_and_untracked_states_are_dirty_and_every_marker_is_rejected`|rawdirtyfixture+真实marker文件遍历，保留各marker拒绝；缩减真实Git准备。|
|55|2.253|`vega::bin/vega::window::workspace::tests::r21_shell_mounts_resizable_sidebar_and_exact_environment_boundaries`|UI shell是应用外壳非OSshell；保留真实GPUI几何边界，查fixture是否不必要创建Git/terminal，勿错误归因Shell。|
|56|2.060|`vega_conversation::git_workspace::branch::tests::switch_e2e::ignored_collision_is_not_overwritten_and_failure_refresh_is_authoritative`|真实ignoredcollision不覆盖需保留小adapter契约；失败后authority刷新业务移stub。|
|57|2.008|`vega_runtime::images::tests::issue63_pixel_budget_rejects_valid_overbudget_header_before_decode`|**已迁移(batch6)** 改用预生成完整有效PNG小fixture(15 629字节)，解码一次验证4001x4000真实尺寸，再跑同一生产入口；同范围1.152s→0.011s；负控制放宽budget→测试失败。|

## Access constraints and next action

GitCommandBackend is an existing crate-private interface, but command_backend field is private to git_workspace and current PolicyFixture is private to trusted_git tests. Snapshot/branch tests can use the existing module boundary directly; Artifact siblings and vega UI cannot automatically access it. Rehoming a test-only fixture or a narrowly scoped test constructor would need explicit ownership/approval; no interface was added in this audit. Avoid inventing a cross-crate test framework just to claim coverage.

Immediate recommended sequence: migrate the two top matrices (53.071sec candidate pool) with small real adapter contracts; then Artifact byte boundaries18.025sec and workspace generation/lease policies using strict finite fixtures; treat UI82.576sec as a separate bounded boundary-design task. Imagefixture2.008sec is a low-risk independent win. The15.701sec deadline requires a realIO/virtual-time separation design, not a blindtimewarp. Keep actualprocessreap and algorithmstress tests honest.
