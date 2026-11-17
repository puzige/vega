# #907 Conversation loading NSH leaf

## Scope and frozen acceptance (before implementation)

Baseline supplied by parent: `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4` plus unrelated in-flight changes. Single writer owns only `crates/vega/src/window/conversation_loading.rs`, its new `conversation_loading/` modules, and this document. No parent render/window wiring, startup/shared logo edits, dependency changes, Git/index writes, network, builds, app launch, or delegation. Parent owns independent review, integration, testing and CI. The explicit no-network/no-Git-mutation direction supersedes the general fetch/update workflow for this leaf.

1. Replace every visible loading-leaf pixel with NSH: delayed loading symbol, failure text and Retry. GPUI remains only a paint-free surface/entity, geometry, native focus, keyboard and accessibility host. Preserve the existing `render_conversation_loading(visit_started, cx)` signature using local deferred rendering.
2. Do not change worker preparation, start, channel/result/generation handling, route invalidation, visit timestamps, or 25 ms polling. Do not replay an unknown Retry. Preserve `Motion::CONVERSATION_LOADING_DELAY_MS`, canonical symbol geometry and 64-slice shimmer, shimmer duration, theme and logical scale; stop shimmer when hidden, inactive or Reduce Motion applies.
3. Reuse one canonical NSH logo renderer. Parent integration extracted the unchanged startup geometry into `crate::loading_logo`, parameterized only by current colors and optional animation phase; both surfaces call it without duplicate module registration or legacy GPUI painting.
4. Retry captures loading thread/generation/start/worker/result phase, sidebar visit and current display identity. The UI-thread dispatcher rechecks Settings, OpenedThread, sidebar opening, draft/cached/controller streams, failure state, current root/controller, live mount, surface health, displayed revision and control geometry before invoking the original `start_conversation_load` once. Stale pointer, keyboard and AX callbacks fail closed.
5. Retry has a native focus handle and one real Tab stop; Tab/Shift-Tab use native traversal, Enter/Space and AX Click converge on the same checked dispatcher. No new keymaps, IME claims or dependency workarounds. Loading has no interactive stop.
6. NSH layout acknowledgement is geometry evidence only, not proof of compositor-visible pixels. Native acceptance remains unrun. No builds/tests while the parent coordinates stable166tests; only scoped source inspection, formatting and static checks are performed here.

## Verification and handoff

### Implemented API and integration

- Existing `render_conversation_loading(visit_started: Option<Instant>, cx: &mut Context<VegaWindow>) -> AnyElement` is unchanged. It returns a local `RenderOnce` wrapper around the native host entity; no parent signature or call-site change is needed.
- `ConversationLoading` adds only its cached presentation entity. Existing lifecycle methods and the full readiness/start/worker block are byte-identical to the supplied baseline. Local helpers project current state and recheck the same cached-stream/draft precedence as the parent render branch, without starting work during validation.
- `model.rs` carries the exact observed thread, generation, start, worker/result phase and sidebar visit, plus display revision/theme/language/focus. `view.rs` draws all leaf pixels with NSH. `host.rs` owns native focus/traversal, display-layout acknowledgement, geometry/viewport/scale invalidation, weak live-mount lease and the single checked dispatcher. Consumption precedes the original start call; unknown results are not replayed.
- Parent resolved the temporary path-module adapter: `loading_logo.rs` now owns the single NSH implementation, startup and conversation loading use module aliases and colors/phase closures. The earlier duplicate path registration and now-unused legacy GPUI function are removed, without changing symbol paths, strokes, colors, scale or64-slice geometry. Independent static review confirmed this affected contract.
- Parent must independently review and compile the final integrated version, especially native surface pointer/focus ordering, element-state lease lifetime and NSH logical-coordinate geometry. The geometry acknowledgement is not compositor/native visibility evidence. This implementation is not counted as part of stable166tests results.

### Actual checks

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; scoped `rustfmt --edition 2024 --config skip_children=true` and subsequent `--check` on the four owned Rust files: exit 0. `skip_children` prevents formatting the reused startup source.
- Read-only `git --no-optional-locks show bd89db7d182e6d9a5c60b1388e79a4623e8cacc4:crates/vega/src/window/conversation_loading.rs` plus Python comparison: lifecycle block and readiness/start/worker block byte-identical. Block SHA256 respectively `ad41869bed528a134e03746d4b126747339765bed7737effe2f66d301a8b3104` and `bbf996db5a5fa1d198b75c30c5addf98d944b55b1416b841fc66bedd9470ff14`.
- Scoped static assertions: no added code comments, unwrap/expect, Rust tests or trailing whitespace; old GPUI button and loading-logo calls absent. Canonical logo SHA256 remains `355a6460684e24a153e39c252e924036b028f1959ee2d8c2f2abb17792d89539`, and the local module path resolves to that file. These are source checks, not behavior-test passes.
- No Cargo/rustc compilation, Clippy, tests, app launch, network, Git/index mutation or CI actions were run by this writer. Cargo/rustc versions were not re-queried for this no-build leaf.

### Stopped Rust content (SHA256)

| File | SHA256 |
| --- | --- |
| `crates/vega/src/window/conversation_loading.rs` | `cc528bfebb4bc8075dfd845983ac52ba40bd0f72965e599dc9c187177a2278dd` |
| `crates/vega/src/window/conversation_loading/host.rs` | `4ea6207cc5deab23a11292d56acbb2c8788fe8921adb718b1f065560a6fbcb91` |
| `crates/vega/src/window/conversation_loading/model.rs` | `6e42931f0ffe65af861455230259ea59873f687d772c5903186c8780b0d83043` |
| `crates/vega/src/window/conversation_loading/view.rs` | `0d2a63425807d66512cb1aaeffb07a3ded014b4119c7b02db2b938cae6cbb1a4` |

### Parent integration validation and development regressions

Parent fmt, workspace all-target Clippy `-D warnings` and Nextest run `c48aecbb-ebee-4ea0-ac3f-abe77b2994d4` passed (166executed/166passed/0skipped) after integrating all three leaves and sharing the logo. Non-implementer static review found no blocking issue in retry authority, display acknowledgement, route/worker preservation or logo extraction. That suite is regression evidence, not complete coverage of the new leaf.

Focused development cases now being added: mount the actual production failure renderer and invoke its Press callback to verify it carries the original thread/generation/visit/revision rather than a later unpublished projection; pending and sidebar-visit loading must expose no Retry; display-frame identity must reject changed authority/environment while animation-only phase changes do not churn it. These tests do not simulate native focus or claim compositor display proof. They remain unrun until recorded below and will be archived/removed before merge.

### Unrun acceptance

Required parent validation includes compilation/Clippy and behavior regressions for stale actions, mount replacement, geometry, route precedence, cached streams, focus traversal and duplicate activation. Native macOS/Windows/Linux appearance, AX, Tab/Shift-Tab, Enter/Space, theme, scale, Reduce Motion and delayed loading remain unrun. Parent owns independent review/testing/CI; no integration, merge or full migration completion is claimed.


## Parent integrated validation (current development batch)

Baseline `ee3947ac0f17b503e6efb635e0c383cea99576af` plus this stopped delta. Format, workspace all-target Clippy `-D warnings`, and Nextest run `4eb222ed-ad9e-4ff9-8cd6-1ccea4c84ec8` passed: **175 discovered /175 executed /175 passed /0 skipped** across16binaries; runtime0.294s. Existing Python release/preparation/general groups passed5/15/40. Actual tools: rustc1.98.0, Cargo1.101.0 (huacnlee0.102.0-9), Nextest0.9.151; CI versions/dependency pins unchanged. This is headless development regression evidence, not native focus/AX/IME/pixels or Windows/Linux acceptance. Final exact-head CI is pending. Rust tests and exclusive wiring must be preserved as recoverable development evidence, then removed before master.

All three focused loading tests executed in the175-test run. The fixture now publishes a competing thread/generation/revision through an engine-owned update task; Press while that update is pending must carry the exact old observation, then the reconciled callback must carry the exact new observation. Production Retry sending uses a stable component owner so dynamic replacement cannot cancel an already-captured request before trusted rejection. This does not revive disposed listeners or weaken host current-frame/layout authority. Current view/test SHA256: `4b343707a3316e4e5b300edb44be179dbdbff81efa71a5d2f824bbb1b45b9be3` / `ea511025cf404e2e48f432721c25acaf128e58c5f1410afb5225d21d9a3248aa`. Native handtesting and compositor proof remain unrun.
