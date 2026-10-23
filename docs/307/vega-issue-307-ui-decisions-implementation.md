# #307 UI-A implementation evidence

## Scope and frozen inputs

UI-A only: accepted pure controller/lifecycle APIs, persistent conversation entry, and pre-finish error capture. Base HEAD remains `961387f9323084f8aaffd1583a0fb870e5a16dd5`; parent owns fetch/merge, Git, review and CI. No commit/push, dependency, runtime/conversation type, theme, test or acceptance changes were made by this stage.

Parent-supplied accepted missing-API RED: run `39811963094` at `bdce33a5837234fe7b18f07c621a6c7965eb4da2`, reconfirmed by run `39813558507` at the base HEAD. Both Clippy and Nextest reported only missing `controller`/`lifecycle` imports and `CheckTicket`; behavioral assertions had not executed. These are inherited prerequisites, not locally rerun results. Runtime's 13-test GREEN at `87a67566621317b2d131db850216e5195952de65`, run `39808680834`, remains an inherited result; the combined patch still needs CI.

Frozen files remain byte-identical to HEAD:

- UI: 15 declarations; `guidance_tests.rs` SHA256 `26971f63dbaa01dc31e68e76d6237b2ffcc1942234bdc4ade96055069e6e48f2`.
- Runtime: 13 declarations (9 Tokio + 4 synchronous); `preparation_session_tests.rs` SHA256 `48076fccdbe6546543d4246f45128adf5177b19172015f6b0d3c5ddd94328d2a`.

## Decisions and source integration

- `cua_onboarding/controller.rs`: implements the frozen actions/effects/view, false/false inspection, unknown/busy/denied separation, manual cancel-before-close/return, stale completion rejection, actual `bundle::bundle_path` drag/Finder selection and development-executable fallback. The guidance projection always reports capture `NotChecked` and `desktop_ready: false`; static grants never close it. Successful drag construction permanently freezes following for that card lifetime.
- `cua_onboarding/mod.rs`: private `Arc` pointer-identity tokens keep card lifetime/check and task-generation identity alive with their callbacks, avoiding wrapping-counter collisions. Exports controller/lifecycle and the UI event `PreparationRequested { thread_id, context }`; no event subscriber/window is implemented here.
- `cua_onboarding/lifecycle.rs`: delegates classification to existing `retained_entry`; detach preserves a retained entry, explicit dismissal survives repeated delivery, and a fresh task token rejects old observe/detach/dismiss calls. Static review of `ExecutorFailure::needs_permissions` in runtime `diagnostics.rs` confirms only the three permission classes match, not transport. This is not executable transport-injection coverage.

Mandatory anchors 1–5:

1. `conversation_stream/core.rs`: one stored lifecycle and cloned active context, initialized from `thread.id` in `new_with_request_queues`. `apply_thread` and authoritative/model updates reject other thread/project identities. Window `render.rs` creates a new stream entity for an uncached thread; contexts are not copied between entities. Same-thread history paging/replacement preserves the lifecycle. Draft-project rebinding explicitly starts a fresh context.
2. `computer_use.rs::set_computer_use_control`: reads outgoing `status().error` before replacement/removal; observes incoming errors even for the same instance; repeated `None` does not erase retained state. Preserves takeover invalidation and invalidates diagnostics on removal/replacement. Agent polling routes each drained batch through guarded ingress and this setter, so rendering stays read-only.
3. `content.rs::MessageStarted`: calls the task transition only after duplicate-active-message rejection. The transition first detaches old control, then begins the fresh lifecycle generation.
4. `computer_use.rs::render_computer_use_notice`: renders the persistent “电脑操控准备” action and explicit “关闭准备入口” independently of the active-control diagnostic. Both keyboard and mouse callbacks capture cloned original contexts. Existing Settings, diagnostic, browser approval and resume callbacks also check their original context; approval/resume additionally verify control identity. Existing `conversation_stream/render.rs` placement remains unchanged. Only preparation and dismissal labels are added; no progress/success/readiness claims are added.
5. `window/artifact.rs::detach_finished_computer_use`: checks existing generation/thread/stream ownership, reads the actual outgoing run control error before `finish` can stop it, then sends that snapshot and a cloned stream context to `detach_computer_use_control`. Both normal batch finish (`artifact.rs`) and worker-spawn failure (`agent.rs`) invoke it immediately before `agent_controller.finish`. The stream clears control through its setter without losing retained state; stale finish cannot clear a newer run.

## Local checks and evidence limits

No local build, tests, application, cargo check or Clippy were run. No independent review is claimed; parent reviews this immutable UI-A patch.

- `rustfmt --edition 2024 --config skip_children=true --check` on the eight authored Rust files: final exit **0**. An earlier check reported one signature wrap, which was corrected. Static caller review also kept the notice renderer at `&self`, matching its existing foreground caller, without changing the out-of-scope renderer.
- `git diff --check`: exit **0**.
- Inline Python source audit (Git diff additions plus owned new files): exit **0** for no added comments, non-test unwrap/expect/panic or test definitions, exact frozen test hashes and 15/13 declaration counts. This is patch hygiene, not a behavioral test. The initial synchronous-only runtime count of 4 was an incomplete observation; counting Tokio declarations yields 13.
- Whole-worktree scope audit: **failed** when an unexpected untracked `crates/vega_ui/src/cua_onboarding/native.rs` appeared after the earlier clean scope snapshot. This writer did not create, inspect for implementation, modify, export or delete that file. It is excluded from the owned UI-A patch, not treated as a passing whole-worktree audit. Parent must reconcile its ownership before integrating. Owned changes are the eight Rust files described above plus this note.

## Open stage-B and delivery requirements

Anchors 6–9 remain open: renderer/window ownership of this controller and `CardView`; subscribe to the preparation event and reject stale context; shared embedded-session false/false probe adapter retaining exact tickets; truthful URL/Finder failure feedback; navigation-only return; native exact `.app` payload and synchronous drag freeze; follow/placement/nonactivation/Spaces/Settings discovery; close/destruction cancellation and ownership cleanup without stopping an unrelated run. Settings integration is not implemented.

Run the unchanged 15 UI plus 13 runtime tests through the existing cloud gate, then complete parent review. Pure tests establish reducer/lifecycle behavior only: no actual retained-entry UI, keyboard focus, native dragging/following, administrator-dialog visibility or real TCC behavior has been validated. Required native/user checks and eventual test-removal/final empty-set gates remain parent-owned. No writes follow this stage's READY handoff.
