# #907 — Native acceptance evidence inventory and blockers

2026-11-10 实机续验已在新版应用复现会话崩溃并开始修复。当前结果见[原版与 NSH 生产窗口对照](nsh-native-parity-loop.md)；下文各版本的取证时间与状态保留。

## Current AX repair and visual-loop boundary

The user subsequently authorized the minimal fixed-version AX repair. Its implementation, 12/12 development regressions, independent closure of both review findings, restored-production-dependency checks and exact candidate binary are recorded in [AX replay repair](nsh-ax-replay-fix.md). This supersedes the earlier authorization denial below; the original crash evidence is retained as history, not a current prohibition.

**Post-fix native crash replay, screenshot/action/recheck and visual parity remain unverified.** Current desktop preflight returned nonretryable `screen_recording_permission_missing` (`not_dispatched`). Enable the currently running Vega in macOS System Settings → Privacy & Security → Screen & System Audio Recording (or Screen Recording), plus Accessibility for operations. Restart if macOS requests it, then send a new message for fresh preflight. No permission was changed automatically or bypassed.

The requested actual-layout overlay has not started because the user explicitly placed the real screenshot/operation/recheck loop first. No native row is newly complete; no merge or release is authorized.

## Historical macOS attempt — 2026-11-10, native AX crash; repair then unauthorized

At the time of this attempt, this section superseded the earlier macOS missing-permission/missing-observation status below. The current status above now supersedes it. It does not supersede the nine acceptance requirements or claim user sign-off. The user's latest feedback is that the development application differs substantially from the expected product; historical review and green CI do not resolve that feedback.

### Version and isolation

- Worktree branch `feat/907-nsh-full`, source HEAD `080a755caec8c5ccc9e48fe40b7aa4728d159ade`; production-source freeze `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`. Fetch and fast-forward update found no remote increment; fetched master is already an ancestor. Only the pre-existing untracked Python cache directories were present before this attempt.
- Host Darwin 24.6.0 arm64. Queried official rustc `1.98.0 (88d9e12ae 2026-08-18)` and Cargo `1.98.0 (797e8a9bc 2026-08-05)`; no new compilation or behavior suite was run.
- Production executable `target/official-nsh-validation/debug/vega`, SHA-256 `bad6faffc8604cfbccfbae1796b28dc642160a5dd14320db771ab50d274ead46`. Two local test application identities contain byte-identical executable copies. They are local automation wrappers, not distribution packages or installed applications; no preview/demo was used.
- Existing daily application is version `0.1.412`, executable SHA-256 `6d63bf0703842f2cc72142394b5eecfe54ac2730a05c9052341117511ba31553`. Its PID 43353 and production window were observed read-only; no input was dispatched to it, no active task was stopped, and no installed bytes were changed. Its source commit is unknown.
- New test root `target/nsh-native-macos-20261110-r1-home` was created through the existing `VEGA_HOME` entry point. The previous `target/nsh-native-manual-home` was not used, copied or deleted. The new test root and wrappers are retained. No credentials were copied.

### Reproduction and actual observations

1. Launched the existing full production executable with the new absolute `VEGA_HOME`; the bare process was not addressable by the available computer-use tool. Stopped only that owned test process, PID 88510.
2. Created an independent local test identity `ai.vega.nsh907.native.r1` under `target/native-907-r1/`, containing the identical executable and isolated-home environment. Native computer-use discovery read a real production AX tree: sidebar/navigation, Settings, empty-task content, composer field, permission/model controls and workspace toggles. The daily application was a different process and identifier.
3. Subsequent screenshot/input attempts returned missing screenshot files or an inactive computer-use target. These errors were initially mistaken for an automation-identity issue. macOS crash reports then established that each observed test process had aborted after AX activation; repeated discovery had relaunched the crashing application. Do not count the failed screenshot or input requests as completed interactions.
4. To capture the actual error, launched the identical executable under a second independent identity, `ai.vega.nsh907.native.r2`, with `RUST_BACKTRACE=1`, the same isolated root and redirected stderr. Native AX discovery again exposed the full application's initial controls; the process then aborted. Changing the test identity/process name did not prevent the failure.
5. Raw stderr records `TreeUpdate includes duplicate child #13343736181152719449` in `accesskit_consumer 0.38.0`, `tree.rs:225`. The main-thread backtrace passes through AccessKit macOS tree update and GPUI Fast `Window::draw_roots`. macOS records SIGABRT. The duplicate ID deterministically maps to NSH surface key 35, node 9 using NSH's existing ID hash.

This is an observed product failure during native accessibility activation, not a screen-recording permission denial. It is not evidence that every launch crashes without an AX client. No NSH screenshot was successfully obtained, so the user's visual/layout discrepancy remains uncharacterized. Empty isolated data explains only the observed lack of history/configuration, not an inferred layout result.

Source-derived diagnosis: NSH `access.rs::synthetic` submits hierarchical nodes through a leaf-only synthetic builder and then rewrites parent children. Fast 0.1.9 retained-frame replay reattaches those leaves to the open parent; nested child references retained in their own nodes then duplicate parentage. This diagnosis has not been validated by a corrective patch. No Kit/Fast source was modified.

### Evidence retained locally

`target/nsh-native-20261110-ax-crash-evidence/` contains six original macOS crash reports, both attempts' stdout/stderr, the local wrapper plists and a hash manifest. Raw reports stay local because they include machine paths/identifiers. The initial bare-process stdout/stderr are empty and do not establish success. The tool-returned AX observations above are summarized, not represented as a saved raw transcript.

| Evidence | SHA-256 |
| --- | --- |
| `manifest.json` | `e30c3ba27e7f30b26f47f3d4e5dae68e9959e64d4cbbae6f905eb2ef50265ad9` |
| `vega-907-native-r2.stderr` | `cd8d9f8bf5f76c98ed26ed6ace21b624d7fa261b50a41df646c9f3eb097d0e08` |
| `VegaNSH907-2026-11-10-093204.ips` | `17beeb2be7fa6f45d74a2736b97fefbbe473f221ac4269203df39482f5762a39` |

### Existing nine-row matrix — this attempt only

All rows bind to source/binary/platform above. No row is complete. AX discovery is an observation, not an AX action/focus/IME pass.

| Existing row | Status | Actual step/result | Remaining prerequisite/action |
| --- | --- | --- | --- |
| Main window/navigation | PARTIAL | Full production window and initial navigation controls observed through native AX; application subsequently crashed. | All task/project actions, draft/persistence, settings open/close and restart behavior remain unrun. |
| Conversation | BLOCKED | No test history/provider configured; no conversation interaction. | Stable app plus authorized service/history; stream, Markdown/tools/thinking/subagents, anchors, selection/copy and scroll. |
| Composer | BLOCKED | Initial input and controls observed only; no confirmed input dispatch. | Stable app; native Chinese candidates, editing, send/stop/recovery/queue, attachments/completion, selectors and Undo/Redo. |
| Permissions/questions | NOT RUN | No approval/question/danger/CUA state prepared or acted on. | Stable app and harmless authorized pending requests; verify every option without bypassing permission. |
| Settings | BLOCKED | Settings entry observed; attempted activation did not dispatch because the test target had exited. | All pages/fields, scroll/save/failure/retry/close/reopen; preserve credential privacy. |
| Workspace | BLOCKED | Initial panel toggle states observed only. | Disposable project/files and stable app; files/images/Diff/commit/branch/artifacts/automation/subagents and safety fences. |
| Terminal | NOT RUN | No PTY opened or input sent. | Stable app; real PTY I/O, native IME, resize and background identity across hide/switch. |
| Cross-cutting | FAIL | Native AX client activation followed by duplicate-child panic and process abort. | Fix requires new authorization; pixels/themes/languages/narrow window, shortcuts/focus/Tab/menu/lifecycle/AX actions remain unrun. |
| Release/platform evidence | PARTIAL | Source and executable hashes verified; this native failure is macOS-only evidence. | No merge/release/install; no new head CI or user acceptance claimed. Windows/Linux remain NOT RUN with the historical environment/action checklists below. |

### User decision and recovery boundary

The user selected **“暂不批准，保留崩溃证据”** when asked to extend the fixed-version NSH patch scope to `vendor/nsh-platform-gpui/src/access.rs`. Existing backend authorization covers reading/selection, native editing, wheel routing and synchronous caret geometry; it does not authorize this AX adapter correction. No implementation file, vendor source, dependency pin, permission setting or daily data was changed. AX was not disabled and no alternative event/screenshot mechanism bypassed the failure.

Implementation and reliable native regression are blocked on that explicit decision. First recovery action, only after authorization changes: specify the minimal AX hierarchy/replay correction, retain all semantic nodes/actions/focus, implement within the permitted fixed-version package, rebuild, and repeat the native crash scenario before visual/navigation/settings/composer testing. Independent review and new-version CI apply to any later implementation. Issue remains OPEN/In progress; PR remains Draft; no merge, release, installation or user sign-off.

## Current clarification — final-source gates and native boundary

The inventory below is a historical, delegated read-only snapshot from `2026-11-10T00:11:38Z`. Its prohibitions on network, Git mutation, application launch and desktop operations applied to that documentation assignment, **not to the parent agent or user globally**. They are not evidence that macOS native acceptance is impossible or that the user prohibited it. Current production source is committed at `c5cd4ffeceda9665dace2602f1d2429fbf9849b1`; both P2 findings and cleanup reviews are closed, development tests are remotely archived and removed, and the source-gate CI succeeded. See [current final-version gates](nsh-final-version-gates.md) for precise evidence and the separate final documentation-head check. The historical pending-gate statements below do not override those later results.

Native status is unchanged: **none of the nine rows has current-version OS-native/user acceptance evidence on any of the three platforms**. macOS ARM64 host availability is established, but a matching accepted build/session and actual OS IME/clipboard/AX/window/pixel outcomes are not. Repository policy assigns real system integration to user manual acceptance; this report does not substitute agent fixtures or screenshots. Windows/Linux have no authorized available GUI route in the present context; machine availability elsewhere is unknown. These are incomplete evidence/environment prerequisites, not demonstrated product failures. No native row is waived or marked complete. No merge/release/install or user acceptance is asserted.

## Scope, snapshot and conclusion

Inventory observed at `2026-11-10T00:11:38Z`; read-only HEAD inspection confirms `0b5ea426abfd8d4358489e916273d89474fb190f` in the supplied `vega-907-nsh-full` worktree. The implementation is dirty (tracked modifications and untracked sources/evidence), so HEAD alone does not identify its bytes. The user supplies the stopped, reviewed-P2-fixes state. This document does not independently refreeze or re-review the implementation, certify its final content identity, or assert that an old binary matches every current dirty file.

**Native acceptance remains NOT RUN / NOT ESTABLISHED for macOS, Windows and Linux in the inspected evidence.** This is a missing-evidence/environment conclusion, not a demonstrated platform product failure. Completing this inventory completes neither #907 nor any native/final delivery gate.

Only this new document is written. Independent evidence reads were batched; no delegation/execution route was invented. No application launch, desktop control, clipboard inspection, installation, network, Cargo/rustc invocation, test execution, Git mutation, CI query, merge or release was performed. Fetch/update was deliberately not attempted because this task explicitly forbids network and Git mutations. Existing source, reports, historical failures and test assertions are unchanged.

## Binding acceptance sources

- [Full migration spec](nsh-full.md), lines 7–18 and **验收先行**, lines 40–52: macOS/Windows/Linux scope and the nine acceptance rows reproduced below. The evidence column requires static contracts, Actions and user manual acceptance; development results are not a substitute for all three.
- [Full migration delivery rules](nsh-full.md#验证与交付), lines 82–86: exact stopped-version independent review, final-head required CI, development-test archival/removal, cloud publication and user acceptance remain separate obligations. Missing Windows/Linux evidence must stay pending.
- [Keyboard acceptance](nsh-keyboard.md#acceptance-before-implementation), lines 7–15: real focus chain, Tab/Shift-Tab, visible focus, pointer routing and native assistive-technology checks are not established by policy/proxy construction alone.
- [Mounted palette evidence boundary](nsh-palette-mounted-ime.md#evidence-boundary), lines 73–80: `integration_edit_key(InputKey)` uses the backend route, not the GPUI Window event closure or OS input-method/candidate UI.
- [Editing backend contract](nsh-editing-backend-contract.md), lines 5–11, 25–27: mounted native hooks, menu actions, recovery and AX replacement are in-process operations, not OS acceptance; no new AX Paste or OS HTML work is authorized.
- [Reading backend contract](nsh-reading-backend-contract.md), lines 19–25: geometry, scrolling, native pointer and three-platform AX acceptance retain a manual-validation boundary.
- Repository `AGENTS.md` and [execution guide §6–7](../vega-exec-guide.md#6-遇阻上报协议所有-agent-必须遵守): report actual blockers, do not replace user system-integration acceptance with agent screenshots or simulated evidence, and do not waive final gates.

The following is an inventory of existing criteria, not new product requirements, a new E2E automation gate, or a request to expand the implementation scope. Historical spec/status paragraphs and earlier failed runs are version-specific; later recorded results do not erase their history.

## Existing evidence and what it does not prove

| Evidence inspected | Recorded result / class | Native-acceptance boundary |
| --- | --- | --- |
| `target/nsh-review-fixes-execution.json` and `target/nsh-review-fixes-vega_ui.stdout` | Retained execution receipt: exit 0, no watchdog timeout; footer **362 passed, 0 failed, 0 ignored**. | Previously executed in-process UI tests, not rerun here; no OS desktop/IME/AT acceptance. |
| Same receipt and `target/nsh-review-fixes-nsh_reading_integration.stdout` | Exit 0; **15 passed, 0 failed, 0 not executed, 0 ignored**, including mounted rename persistence and recovery/history/AX/submit. | Actual mounted production/controller paths can be exercised without a real OS candidate window, desktop clipboard gesture or assistive-technology client. “Native” in a target/helper name does not change that boundary. |
| Same receipt and `target/nsh-review-fixes-nsh_settings_integration.stdout` | Exit 0; **4 passed, 0 failed, 0 ignored**, including queued General-settings command revocation on close. | Retained SettingsView/Surface/queue regression, not a full native settings session or full-app sidebar-persistence acceptance. |
| [Integrated validation record](nsh-reading-editing-integrated-validation.md), lines 1390–1561 | Earlier stopped-implementation receipts include native-host **36**, geometry **24**, reading **15**, scroll **16**, mounted palette **3**, and other application/workspace checks. | Historical development evidence at its own recorded identities; not fresh post-P2 execution of every target or final-head CI. Its final section explicitly leaves OS AX/IME, three-platform manual acceptance and delivery gates uncompleted. |
| [Conversation independent review](review-conversation-production.md) | No actionable P0–P3 in its bounded reviewed delta. | Static implementation review, not exhaustive coverage or native acceptance. |
| [App/settings/workspace independent review](review-app-settings-workspace.md), lines 85–132 | Incremental static review closes the queued-General-settings P2 at its exact listed hashes; runtime responsibility is separate. | Does not establish OS pointer/focus/AT behavior, final CI or user acceptance. |
| [Native/security review](review-native-security.md) and [paste-fix implementation record](nsh-review-single-line-paste-fix.md) | The inspected review file retains the original P2 finding; the implementation record supplies repair hashes and scope. The supplied task context says reviewed P2 fixes are stopped. | This inventory neither contradicts that supplied state nor manufactures a missing independent closure receipt in the original review file. It does not reopen the finding or repeat the source audit. Neither record establishes OS clipboard/IME acceptance. |

The post-P2 counts above were read from retained footers and receipts, not inferred from authored tests or empty stderr. No current toolchain was queried, no test executable was launched and no complete current-source-to-binary provenance was newly established. Older “unrun” author handoffs remain historical; the retained parent results provide later development execution evidence, not native passes.

No current-version OS-native acceptance record or user pass was established by these reads. No screenshot was captured, inspected as current acceptance, or promoted from an older snapshot. Product screenshot/attachment features and mounted pixel/geometry observations are not screenshots of a current accepted desktop session. A historical screenshot, if supplied later, requires its own version and scenario attribution and cannot by itself close an interaction row.

## Platform environments and authorization blockers

| Code / platform | Observed availability and actual blocker | Concrete required environment / next step |
| --- | --- | --- |
| **M — macOS** | `uname -srm` reports **Darwin 24.6.0 arm64**. A macOS host is observed, but this task forbids app launch, desktop control, installation and network. No usable interactive session, OS IME/AT permission or current installed app identity was tested. | User-authorized interactive macOS desktop with the exact accepted source/build identity, native Chinese IME, clipboard/menu access and native accessibility client as needed for the existing rows. The user performs native acceptance through the established delivery/manual-test process; obtain explicit authorization before any otherwise prohibited preparation or agent operation. Host OS alone is not an executed test. |
| **W — Windows** | **No authorized available Windows GUI execution route in the current tools/context.** Workflow absence does not prove that no Windows machine exists. Hardware, VM, remote session, toolchain and app availability are unknown, not probed failures. | An explicitly provided/authorized interactive Windows desktop (physical, VM or remote route), compatible exact-version app/build, native Chinese IME, system clipboard/context menus and Windows accessibility client. Authorized owner supplies the build/session and user records the existing matrix outcomes. Do not assume provisioning, installing or adding a workflow is authorized. |
| **L — Linux** | **No authorized available Linux GUI execution route in the current tools/context.** The Ubuntu aggregation job is not such a route. Machine/display-server/desktop/input-method/build availability is unknown; this is not a failed Linux build or a proven lack of hardware. | An explicitly provided/authorized interactive Linux desktop with compatible exact-version app/build, working graphical display session, Chinese input method, clipboard and accessibility stack. Record the actual display-server/session and platform semantics used; this does not add a requirement to test every display server or distribution. User executes the existing rows there. |

For every row below, use the exact source/build intended for acceptance and record the actual platform/session, scenario, observed result, executor and bounded evidence. Existing repository policy puts real UI/system integration with user manual acceptance, normally after final gates and merge on master. This report does not authorize pre-gate installation or change that order. Where a scenario depends on existing service/provider/PTY state, have the authorized user prepare that state without disclosing secrets; a test fixture does not stand in for the real result.

## Exact unmet platform acceptance rows

**Status legend:** `NOT RUN (M/W/L)` means current OS-native/user acceptance evidence is missing and the corresponding environment/authorization blocker above applies. It does not mean the product is known to fail. The nine source rows are unchanged; every platform cell remains open.

| Existing spec row | macOS | Windows | Linux | Exact outstanding native scenario / observable result and next step |
| --- | --- | --- | --- | --- |
| **主窗口/导航 — Main window/navigation** (`nsh-full.md:44`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | In the authorized desktop, create/switch/delete/rename/pin tasks, switch projects and open/close settings. Verify unchanged routing, drafts, persistence/confirmation and recoverable empty/error states. Use disposable user-approved task/project data. Include native rename paste then Return from the affected P2 path; a mounted persistence pass alone is insufficient. |
| **会话 — Conversation** (`:45`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | With appropriate history and a streaming conversation, load history, expand tools/thinking/subagents, locate anchors, select and copy. Verify correct message/source/history/position and retained actions. Exercise actual pointer/keyboard/context-copy, shaped selection/caret, scrolling and link/focus/AX behavior covered by the existing reading contract; helper snapshots and geometry are not OS event or AT observations. |
| **Composer** (`:46`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | Use the platform Chinese IME and real candidate UI; send/stop/recover, queue, attach existing supported clipboard/file payloads, complete files/skills and switch model/thinking/permission. Verify no accidental send and original trusted-controller behavior with truthful results. Check candidate/focus handling in the migrated palette/editor paths, native clipboard/menu Undo/Redo and recovery. Retained marked-key/AX replacement tests do not establish OS delivery. No HTML capability expansion is added. |
| **权限/提问 — Permissions/questions** (`:47`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | With user-controlled pending tool approval, dangerous confirmation, question and CUA-guidance states, verify all original options are reachable by real input, original permission checks remain and no execution result is fabricated. Real protected operations stay subject to their own authorization; the acceptance session is not blanket approval. |
| **设置 — Settings** (`:48`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | Walk General, providers/models, MCP, Agents, Skills, usage, updates and other existing entries. Verify original field validation/save/failure/retry behavior and credential non-disclosure. Exercise real close/reopen/focus behavior around the repaired General-settings path; the four-case retained-owner fixture is not this user session. Use authorized configuration/service state without logging secrets. |
| **工作区 — Workspace** (`:49`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | With user-approved files/repository/artifacts, exercise file/image/Diff/commit/branch/artifact/automation/subagent panels. Verify existing actions, dirty/conflict state and safety fences. Actual opening/editing/destructive or external actions require their normal authorization; source contract inspection is not a completed native action. |
| **终端 — Terminal** (`:50`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | In an authorized harmless PTY session, verify real output/input/IME, resize, task switching and hidden panel behavior; retain PTY identity and background execution. A painted text projection or in-process controller test cannot prove an interactive OS terminal session. |
| **跨切面 — Cross-cutting** (`:51`) | NOT RUN (M) | NOT RUN (W) | NOT RUN (L) | Use light/dark themes, Chinese/English, keyboard/shortcuts, Tab/Shift-Tab, narrow windows, close/reopen. Verify original tokens/i18n, actual reachable/visible focus, modal/input ownership and surviving background work. Use native AT to check the existing accessibility/focus contract; in-process AX actions/proxies do not prove OS accessibility exposure or client interaction. No performance A/B gate is introduced. |
| **发布 — Release/platform evidence** (`:52`) | NOT ESTABLISHED (M) | NOT ESTABLISHED (W) | NOT ESTABLISHED (L) | Bind the complete final source to its required CI and authorized cloud build/publication evidence, and collect platform-specific user manual outcomes. Current macOS-only release workflow is not evidence of Windows/Linux delivery. Parent must obtain an authorized platform build/distribution route for those sessions, not silently modify CI or publish/install here. No final release, three-platform acceptance or merge is claimed. |

## What current workflows can and cannot supply

Inspected all three files in `.github/workflows`:

- `pr-check.yml:14–80`: Clippy/fmt and Nextest run on `macos-latest`; Python script checks also run there. `:84–99` runs the aggregate **check (fmt, clippy, test)** on `ubuntu-latest` and only checks the two job result values. It does not check out/build/run the app on Linux. There is no Windows job or interactive native-acceptance job in this file.
- `release.yml:22–34`: one **build & publish (macos-latest, arm64)** job, fixed source checkout. Its packaging/signing/notarization/upload steps publish `Vega-macos-arm64` (`:89,139–184`). It does not supply Windows/Linux packages or native acceptance.
- `cicd.yml`: master pushes call that reusable release workflow with `target_sha: github.sha`; this adds no platform matrix.

These are static workflow facts, **not current Actions status**. No network query or final-head run receipt was obtained. A green macOS check or an Ubuntu result aggregator would still not close any Windows/Linux GUI row. No available Windows/Linux GUI route is authorized in the present tools/context, but no claim is made that such machines or routes cannot exist elsewhere. Adding CI jobs, provisioning machines, changing dependencies or creating new acceptance automation is not authorized by this inventory.

## Preserved baseline limitation, not a new acceptance requirement

[Native HTML acquisition boundary](nsh-native-html-acquisition-boundary.md), lines 11–17, classifies missing OS HTML acquisition as **pre-existing**, not a migration regression: the original GPUI path and the pinned current clipboard carrier do not acquire HTML. Opaque metadata is not HTML. This report does not claim native HTML works, turn its absence into a new #907 regression, add rich-document editing requirements, waive preservation of originally supported text/images/paths, or begin an acquisition patch. Any separately requested expansion needs explicit scope/semantics/platform authorization first.

## Remaining gates and actionable handoff

1. **Native/user acceptance — blocked, not completed.** `[BLOCKED] #907 | Current macOS/Windows/Linux native acceptance evidence is absent; macOS execution is prohibited in this inventory and no authorized available Windows/Linux GUI execution route exists in current tools/context | Mounted/native-host tests and macOS CI are not OS acceptance; Ubuntu only aggregates results; workflow absence does not prove machine absence | Provide authorized platform sessions/builds and have the user execute the existing nine rows, retaining exact-version per-platform results.`
2. **Review — separate bounded evidence.** Reuse exact-version independent-review conclusions only within their recorded scope. Supplied reviewed-P2 status is not a new whole-tree/native approval by this document. The inspected native review file retains its original finding; any separate closure evidence belongs with the parent record, not an invented append here. Do not restart optional audits or implement unrelated findings.
3. **Final-head CI — not obtained.** Parent must bind the final content/commit to required CI under the normal workflow. This dirty HEAD and historical local receipts are not a final-head CI pass. Network/Git prohibitions prevent advancing that gate here.
4. **Development-test archival/removal and final zero-test delivery check — not performed.** Existing development tests/support are visible in the retained evidence; this task authorizes no cleanup or Cargo execution. Parent follows the existing lifecycle, preserves recoverable evidence, removes development-only Rust test/support wiring before master, and verifies the final head without weakening production or Python coverage. Do not equate an old nonzero passing suite with the final zero-discovery/zero-execution gate.
5. **Merge, cloud packaging/publication, installation and user sign-off — not performed or claimed.** Follow existing gate order; no Issue/Project state was changed and #907 is not declared Done. Provisioning/build/distribution or approval decisions require the appropriate owner/authorization; they are not hidden implementation work for this report.

## Read-time evidence identities and verification limits

SHA-256 values below bind the cited bytes read during this inventory, not all dirty production source or future runtime evidence:

| Path | SHA-256 |
| --- | --- |
| `docs/907/nsh-full.md` | `dba7ae4653eb7a274b45b8f75863420689bb991c1ac79bdaab0303c338ecfed3` |
| `docs/907/nsh-native-html-acquisition-boundary.md` | `f88517ff55f3170c02ac6ad9fcd405f4eacc1ec800c4fa721e6e1f9e2831d156` |
| `docs/907/review-native-security.md` | `cfd52a85052b1e1663b0cc707ef49e02bee8adfc80b7eecbfaff722b0bcc06a3` |
| `docs/907/review-app-settings-workspace.md` | `fc1af91c7c991f3804200dcf3228df26b5d93f679daabf287ff3d1c34cbe0218` |
| `docs/907/review-conversation-production.md` | `f7df5bee29688a3cc8cae076280c0c3185eebd91f8814471b3a338266146cbf0` |
| `target/nsh-review-fixes-execution.json` | `719cca996a844cc8c5135b6f024921582737606e9e73397aad1396bbf4a63c27` |
| `target/nsh-review-fixes-vega_ui.stdout` | `1a1cb4b3d7ce1cc69267570525fcea0371ef4ad0bc057d983f9beb72d0db0115` |
| `target/nsh-review-fixes-nsh_reading_integration.stdout` | `900151850e4c5049cd2ba0de4c94fb8d0b46fe0f032059566bbb6484ef727e32` |
| `target/nsh-review-fixes-nsh_settings_integration.stdout` | `c17c9e6c3b3b025de28b2b1b411a47a0f8c0446ecfad45504a331d76b644ca38` |
| `.github/workflows/pr-check.yml` | `536ee05031209834437b7c03be7e076392ec9ab7f81b77b25f2ed21ef977150a` |
| `.github/workflows/release.yml` | `4d36813ea110fe472ab1a65b19dbd7b4dce319238e2d06edd4b3588bda23a1cc` |
| `.github/workflows/cicd.yml` | `a11cc5e9c781127c31515fba9bb6572a3d700996ae88c39051178b8d0a29c496` |

Performed focused file/spec/workflow reads, retained-result inspection, read-only `git rev-parse`/status/diff metadata, host identification and SHA-256 calculations. Final document readback and documentation-only checks cover local links, matrix/platform status cells and whitespace; they are not product validation. This document's final hash is reported separately after readback to avoid a self-reference. No tools will be used to run the app, alter Git or obtain network evidence as part of this inventory. Stop after report verification/hash with the native and final-gate limitations intact.
