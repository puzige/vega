# Issue 448 · Subagents workspace delivery

Status: CI3 formatting and all-target Clippy passed; Nextest ran 3151 tests,
with 3150 passed, one failed and five skipped. The failing fixture is corrected
and awaits CI4; manual acceptance remains pending.
Specification: [Subagents in the workspace v1.2](vega-issue-448-subagent-panels.md).
Implementation baseline: `6e9a69c7`; frozen specification commit: `ab331d14`.
Integration baseline: `9555d9e8` (#462 and #464); the Composer primary-action
changes affect a different render section. The main agent records the integrated head, PR, CI evidence
and merge outcome in the Issue and PR. Compilation succeeded in CI3, but the full
automated gate and manual acceptance remain pending.

## Behavior and implementation

`window/subagent_panel.rs` loads every durable direct Native child through the
conversation API, independently of the parent's current history page. Accepted
Admit/Updated/Finished projections merge into this cache. Per-parent load epochs
and per-child live revisions prevent older reads from replacing newer state.
Successful privacy cleanup or exception changes invalidate the projection and
selected detail before reloading it.

The existing Environment slot displays a compact Subagents summary only when
children exist or a file-backed read actually failed. Standalone parents use the
same slot; project Review and Terminal retain their previous gates. In-memory
stores without an asynchronous database path do not create a false error entry.
The workspace menu and summary open one Subagents tab. List selection and inline
Open show detail without replacing the parent route, stream or draft; Back returns
to the list. The existing workspace docking, hiding, closing and maximize controls
remain in use, including narrow-window hide/restore and resize behavior.

Detail holds the actual active child stream when available, otherwise loads its
history through the read-only history API. It retains text, thinking, tools and
history paging. Open conversation uses validated parent/child navigation and the
normal main-column Composer. No provider is started by listing or previewing.
The old child-to-parent route and original continuation behavior remain intact.

ConversationStream's reading presentation suppresses Composer, settings, commit,
source-specific artifact actions and secondary approval/question responders. It
keeps error content, Plan review notes, disclosure and copying. Mounting does not
clear the window selection or restore input focus. Artifact cards added during
reading inherit that presentation. Switching out of the pane resets presentation
and releases abandoned hydration state; a separate history epoch rejects delayed
reads across hide/show, and selection epochs reject reads after Back/close/change.
The normal `owns_stream_request` mutation guard was not widened.

Active controller state takes precedence over the original delegation status when
a child is continued by the user. Stop validates the parent/child relationship and
captured controller generation, pauses only that child's automation, and requests
cancellation of that exact run. Returning to the list does not revoke an explicit
Stop. A later generation, sibling and parent remain unaffected. The original
persisted delegated result and automatic parent cancellation ownership are unchanged.

## Regression coverage added

CI3 passed nine of the ten added regressions. The artifact-card fixture failed
before its reading-mode assertions; its correction awaits CI4. Existing #448,
ACP, continuation, permission/question and workspace assertions remain in place
and passed in CI3.

| Coverage | Test | Acceptance |
|---|---|---|
| Durable children outside the loaded parent page; default-collapsed summary; project-only gates; tab dedup; mounted menu, Tab/Enter list selection and Tab/Space Back; inline detail; full-route navigation; retained draft and no worker start | `panel_summary_list_inline_detail_and_full_route_preserve_parent` | P01–P03, P06–P07, P10–P12 |
| Active stream reuse and Running/Stopping overrides; Stop survives Back; target-only Goal/Loop pause and cancellation; stale generation; close without cancellation; active full-route reuse; unchanged delegation result | `panel_live_run_stop_keeps_sibling_parent_and_later_generation_safe` | P04–P06 |
| Live revision wins over delayed list; old history after hide/show; old detail/history after selection; real paging after re-entry; A→B→A route epoch | `panel_stale_load_detail_and_paging_callbacks_cannot_replace_current_owner` | P01, P07–P08 |
| Read failure and Retry; detached child rejects navigation; narrow pane hide/restore/resize; bottom docking | `panel_errors_retry_detached_child_and_narrow_restore_remain_usable` | P09, P11 |
| Actual privacy Exception and Clean callbacks invalidate names/detail and reject older cache reads | `panel_privacy_cleanup_invalidates_cached_names_and_old_reads` | P07–P08, P12 |
| Empty file-backed and in-memory parent have no Subagents availability/error | `panel_empty_memory_and_file_backed_tasks_keep_overview_unavailable` | P01, P12 |
| Reading mount preserves parent input focus, draft and selected text | `reading_preview_mount_preserves_parent_focus_selection_and_draft` | P10 |
| Leaving reading releases older/newer hydration ownership; preview cannot submit and normal presentation restores submission | `reading_preview_releases_abandoned_history_and_restores_submission` | P08, P10 |
| No preview permission action or keyboard responder; existing full-route responder remains functional | `reading_preview_has_no_permission_responder_and_full_route_retains_it` | P10, P12 |
| Newly added real artifact cards inherit reading mode and restore their normal mode afterward; Git boundary uses an in-process replacement | `stream_artifacts_arriving_during_reading_inherit_and_release_the_mode` | P10, P12 |

The new app tests use temporary SQLite fixtures and the production window/controller
and mounted controls. Delayed callbacks are delivered with captured owners and
controlled in-process scheduling, without sleeps, external providers or real E2E.

## Validation and remaining evidence

Local checks:

- `cargo fmt --all`: exit 0.
- `cargo fmt --all -- --check`: exit 0 on the final implementation files.
- `git diff --check`: exit 0.
- Static review of route/selection/history ownership, exact-generation cancellation,
  file-backed availability, privacy cache invalidation, added dependencies/schema,
  production unwrap/expect and added comments.

No local Vega execution, compilation, build, test, Clippy, nextest or packaging was
performed. The required cloud `check (fmt, clippy, test)` gate remains pending and
must cover the integrated head, including the full workspace regressions. There
are no new dependencies, schema changes or planned specification deviations.

P13 and real UI acceptance remain pending: two parallel children; summary → list →
detail → Back/Open conversation/Stop; long transcripts and paging; focus/copy;
light/dark pixels and narrow/docked/maximized layouts. Pixel appearance and real
service integration cannot be established by the local static checks in this report.

## CI1 correction

[GitHub Actions run 39106565722](https://github.com/puzige/vega/actions/runs/39106565722)
failed on `2fb660bbe0eb6d74739cbd6f0bb8e5424c473dfb` before tests could run:

- The terminal preview image subscription used `Context::subscribe`, whose emitter
  argument is an owned `Entity`; the helper requires a borrowed entity. The callback
  now passes `&stream`. The live subscription uses `subscribe_in`, which already
  supplies `&Entity`, and its existing call remains correct.
- The new window test module imported `super::*`, transitively importing GPUI's
  `test` procedural macro. The macro's generated `#[test]` therefore resolved back
  to itself. The module now names its parent types and application types explicitly
  and uses the same macro-free GPUI prelude as the existing workspace tests.

No recursion limit, test exclusion or assertion was changed. Static inspection
also checked the other new test modules: their parent imports expose an explicit
GPUI type set and the prelude, without importing the `test` macro. No local
compilation or test execution was performed. Formatting and diff checks passed;
the next integrated head and its cloud result remain for the main agent to record.

## CI2 correction

The Clippy job in [GitHub Actions run 39107202279](https://github.com/puzige/vega/actions/runs/39107202279)
failed on `38c9f387` with three `E0061` errors in the new panel tests. The tests
called `AgentWorkerStartProbe::load` as though the wrapper were an atomic counter.
The existing wrapper takes no arguments and chooses `SeqCst` internally. All three
calls now use `load()` and the unused `Ordering` import is removed. Each assertion
still requires the real worker-start count to equal zero; the production probe is
unchanged.

Static follow-up checked the new tests' controller begin/finish, workspace restore,
stream hydration/composer and permission/selection fixture calls against their
existing definitions. No further signature changes were needed. Formatting and
diff checks passed. No local compilation or tests were run; the next CI result is
pending and must establish whether these corrections resolve the failure.

## CI3 correction

[GitHub Actions run 39107706516](https://github.com/puzige/vega/actions/runs/39107706516)
on `4d89a668` passed formatting and all-target Clippy. Nextest reported 3151 tests
run: 3150 passed, one failed and five skipped. The only failure was
`stream_artifacts_arriving_during_reading_inherit_and_release_the_mode`, whose
artifact capture returned `ArtifactConflict` before the reading-mode assertions.

The fixture used project `p`, thread `t` and call ID `read-pane-write`, but its
checkpoint reference encoded call ID `c`. The fixture's proposed call and artifact
lookup now both use `c`, matching its existing checkpoint reference. Production
checkpoint validation and both real reading-mode assertions are unchanged. The
other nine added regressions and all existing executed regressions passed CI3.

Formatting and diff checks passed after the fixture correction. No local
compilation or tests were run. The corrected fixture and complete integrated
head still require CI4; this report does not claim a passing full test gate.
