# #907 — shared window navigation NSH migration

Baseline `bc7d8ae0d91649a8ed0fb48679eb43d6efd6ce4d`. Scope: navigation.rs/new child module and the sidebar/main-header callsites only; preserve original route actions, draft-budget guard, task-mutation state and key bindings.

## Acceptance before implementation

| Case | Required observable result | Verification |
| --- | --- | --- |
| Sidebar expanded/collapsed header | Same stable Sidebar, Search, Back, Forward order; NSH icons, hover/pressed/disabled/focus state, original control size/gap | Source review; native pixels pending |
| Back/Forward disabled or changed | Fresh NavigationState, task/project identity, mutation epoch and displayed mount reject stale clicks/keys; dispatch original action only | Development policy tests |
| Search and Sidebar | Original OpenPalette and ToggleSidebar action dispatch; no direct route or persistence IO | Source review |
| Keyboard | Real Tab-enabled host, descendant AppView entry transfer, disabled skip, no held Enter/Space, external exit, current action AX proxy | Shared policy tests/source review; native pending |
| Sidebar relocation/unmount | Window-local keyed owner, fresh Surface identity on relocation; old commands cannot target new owner | Identity policy tests; native pending |
| Failure | Explicit presentation rebuild cannot replay navigation/search/sidebar actions; no legacy GPUI product rendering | Source review |

All visible controls are NSH. Fast hosts only focus/event/AX and native window composition. Existing keyboard navigation guards and services remain unchanged. Use fixed NSH0.4 APIs, no upgrade/fork. No app execution or handtest inferred from compilation.

## Pause recovery and integration

Resume retains the interrupted navigation implementation rather than restarting. A paint-free Fast element retains a weak presentation lease only across actually mounted frames; loss of that lease rotates identity on remount and rejects queued callbacks. Current SettingsOpen and sidebar preference are rechecked at action time, alongside task/project identity and mutation state. The original draft budget guard and window action receivers remain authoritative. NSH uses pinned `role`, `label`, `disabled`, and `ImageSrc::svg_markup`; all buttons remain NSH pixels. Pointer activation restores host selection, while AX activation additionally requires current host focus and matching selected target. Four development regressions cover these policy boundaries; native focus/geometry and compilation remain pending at this checkpoint.

## Resumed static review and verification checkpoint

Stopped production content: `navigation/nsh_controls.rs` SHA-256 `b2f422673ef13f669c5a8b3b03921adc58896b73b5246d0c55d77676ff3a2ef7`. Independent read-only review closed three actual pinned-API blockers: use `on_press_down/up`, convert GPUI colors through `crate::nsh::color` (including reactive background), and construct `ns::svg(ImageSrc::svg_markup(&source))`. Parent additionally replaced silent `try_send` with the existing `OrderedCommands`; incremental nonwriter review confirmed the owned request/lifetime and predecessor ordering. The channel remains capacity 64; this does not establish a global bound on pending spawned submissions. No additional concrete finding was established in reviewed freshness/lifecycle/action/keyboard paths. Static conclusions do not establish native focus/AX/pixels.

The coordinated `cargo check -p vega_ui` attempt was refused by the tool permission system with `permission decision failed closed`; no command result or Rust compile evidence was obtained. This is an execution-environment/permission blocker, not a product compile failure. No alternative build route was used. Compilation, workspace strict all-target Clippy, fmt and all new development tests remain UNRUN on this content; earlier 94-test/CI success belongs to baseline `bc7d8ae0` only. Restore normal command execution, then run the stopped integrated version, correct any actual failures, and obtain current-head CI after a development commit. Four navigation tests and nine Artifact tests are source counts, not executed counts; final merge still requires Rust test archival/removal.

## Normal execution restored — current local validation

On user-authorized retry, normal execution succeeded; the earlier permission failure is no longer blocking. An optional background-session request was rejected before dispatch (`workspace_sandbox_required`); removing that unsupported session parameter allowed the unchanged standard Cargo route. No permission or machine configuration was changed.

Actual compilation exposed missed pinned-API errors: Tooltip requires an NSH element, hover callbacks accept `&MouseEvent`, SVG source is `nsh_core::element::ImageSrc`, and Sidebar's window argument must be named. These were corrected. All-target test compilation exposed wildcard import of the GPUI `test` proc macro; explicit test imports fixed wiring without altering the four assertions/cases or recursion limit. Artifact callback types, opacity inference and a collapsible match were likewise corrected at their responsible layers.

Stopped navigation SHA-256: `nsh_controls.rs` `2b2494bd5b8283166958f7c17dce55a4a3132ca6216adf3bd3ac9ff1616077af`. On the coordinated dirty version based on `bc7d8ae0d91649a8ed0fb48679eb43d6efd6ce4d`, actual `cargo check -p vega -p vega_ui --locked --offline -j 2`, workspace `cargo fmt --all -- --check`, `git diff --check`, and `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` succeeded. Nextest list discovered 111; `cargo nextest run --workspace --locked --offline --build-jobs 2` executed/passed **111/111**, skipped 0, run `74a739c9-cb15-4040-82a0-646607972288` (execution 0.167s), including all four navigation and thirteen Artifact tests. Python checks passed 5/15/40. Actual tools: rustc1.98.0, Cargo1.101.0 / huacnlee fork0.102.0-9, Nextest0.9.151; CI remains pinned0.9.146. Existing fork manifest unused-dependency warnings remain outside scope; no Rust compiler/Clippy failure remained. Original logs: `/tmp/vega-907-resume-{check,clippy,tests,release,computer,python}.log`, discovery JSON `/tmp/vega-907-resume-test-list.json`.

Nonwriter incremental review of these API/test-wiring corrections completed with no concrete finding; reversing the declared production corrections in memory reproduced the prior reviewed hashes, and final scoped hashes remained stable. Current-head cloud CI, native focus/AX/pixels and supported-desktop acceptance remain unestablished; development Rust tests must still be archived/removed before final merge. New independent palette/branch work is not covered by these results.

## Composer capability checkpoint

The fixed Input exposes a single ResolvedTextStyle, not range decorations or range geometry. Editor supports decorations but its public events omit composition and full clipboard payload: editor/view.rs sink forwards only `PasteEvent.text`; editor/controller.rs `paste_clipboard` reads text only. Its internal composition is private. Overlaying independently laid-out text over Input cannot establish glyph/wrap/caret parity, particularly weighted skill mentions, CJK and combining characters. No production Composer cutover is authorized by this investigation; retain original production input until a behavior-preserving implementation or explicit dependency decision is established.
