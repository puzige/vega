# NSH reasoning-default failure feedback

## Scope and acceptance

Migrate only the existing production reasoning-default save failure message/model, actual saving state and explicit retry control. Keep Providers authority, the complete `ReasoningDefaultFailure`/`ComposerDefaultsRequested` identity, and `ConversationStream::request_reasoning_default_retry`; do not add a new settings editor or modify shared business state.

- All visible feedback and retry/recovery controls are NSH elements, not a GPUI-painted child wrapped in a Surface. GPUI retains structural layout, focus/event plumbing and invisible, independently measured native AX proxies.
- An action must match the active stream/root/navigation epoch, complete thread and observed failure list, exact failure request (revision, thread and every defaults field), retry-pending state, environment, mounted Surface, live row lease, current display identity and finite fully clipped-in measured bounds. A same-model replacement request must reject an old action. List indices are never authority; derive the current neighbor index only after full identity validation.
- Preserve the existing `reasoning_default_retry_focuses` handles, model-specific accessible retry names, `focus_reasoning_retry_neighbor(index, backwards)`, `NextComposerControl`/`PreviousComposerControl`, Enter/Space and return to Composer after keyboard retry. Pointer and native AX activation use the same fences. Do not activate during composition, modified keys or held repeats.
- Preserve actual pending state: show localized saving instead of an enabled retry, and never label accepted retry as saved. Do not silently retry business operations on mount, redraw or renderer recovery.
- Each visible message/status/button has its own NSH measurement/native AX bounds. Focus requests host autoscroll. Hidden, clipped, unmounted or stale leaves cannot activate. Displayed ACK updates must not invalidate their own presentation or cause a publish/render loop.
- Surface errors expose a separately created NSH reload control that only recreates presentation. No GPUI-painted fallback, no unknown command replay, and no native AT success claim without platform evidence.

## Integration and validation plan

Parent owns module/render wiring and production Cargo checks. This lane may only run scoped rustfmt and read-only static inspection. The exact shared-file wiring and frozen file hashes are recorded below. No dependency/vendor/Git/network/native/config/Cargo changes are authorized. No intermediate review is requested.

Native focus/AT, visual geometry, scroll reveal, IME, keyboard/pointer and all three platforms remain independently unverified until executed. Static checks are not typechecking or behavior tests.

## Implemented

`reasoning_feedback.rs` constructs separate NSH message, saving and retry leaves. Messages retain the localized model name; retry keeps the model-specific accessible name and the original focus handle. Pending replaces retry with the actual saving label. The leaf renderer constructs the text, hover/press/focus styling and recovery button with NSH. GPUI paints no visible feedback/control; its only children are the Surface and an invisible native AX proxy for that measured leaf.

The binding captures the entire failure list, complete selected failure/request, complete thread, owner, navigation epoch and retry focus identity. Before the original model-based business entry is called it must still resolve to that exact failure, including revision/thread/all defaults and retry-pending. A row index is computed only after this validation for the existing neighbor method. Pointer/native AX and keyboard all use current projection, live row lease, Surface health, active root/environment and mounted finite clipped-in measured geometry. Keyboard rejects composition, modifiers and repeat, and returns to the original Composer input after retry. Window activation/visibility and focus invalidate old displayed identities. Host keys isolate owner/focus identities so subscriptions cannot follow a replaced focus handle accidentally.

Every leaf reports `Measurement.page` and `frame.size`; host height converges on actual content. Identical Displayed ACKs do not notify, and accepted ACKs do not invalidate or republish their own projection. NSH publications are deduplicated by projection identity. Geometry/height/focus/source changes require a fresh measurement. Recovery stops/replaces only the failed Surface and does not replay business retry. Unmount retires the mount lease and stops presentation, not a backend operation.

## Parent-only exact wiring

No shared file has been written by this lane. Apply these three changes together before production typechecking:

1. In `conversation_stream/mod.rs`, next to the existing privacy module declaration:

```diff
 mod privacy;
+mod reasoning_feedback;
```

2. In `conversation_stream/render.rs`, change only the visibility of the existing helper (its body remains intact):

```diff
-    fn focus_reasoning_retry_neighbor(
+    pub(super) fn focus_reasoning_retry_neighbor(
```

3. In `render_composer_column`, replace the entire `.children(...)` expression whose iterator is `self.reasoning_default_failures.iter().enumerate()` with this single child:

```rust
            .child(self.render_reasoning_feedback(cx))
```

The current exact replacement interval is `render.rs:286–397`, immediately after the `controller_error` children and immediately before `.when(self.thread.backend == TaskBackend::Native, |column| { column.child(self.render_pending_status(cx)) })`. Delete that complete old failure `.children` expression, including its closure/listener block; do not keep a GPUI fallback or duplicate failure rendering. Keep `previous_composer_control_with_reasoning_retry`, the existing Composer actions, core failure state and `request_reasoning_default_retry` unchanged. No new core field, initializer, lifecycle hook, dependency or export is required. Parent has already connected Privacy `.owner(self, cx)`; that change remains untouched.

## Frozen evidence and remaining verification

This lane is READY for the parent-owned wiring and integrated checks, not a claim that the production path has already switched or compiled. No remaining product/spec blocker is known in this scope. The withdrawn context-panel assumption is resolved and nonblocking; Providers authority and existing NSH usage remain unchanged.

Actual local commands on the final Rust bytes:

- `rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- `rustfmt --edition 2024 --config skip_children=true` on the two Rust artifacts, then the same invocation with `--check`: exit 0.
- Read-only Python static audit of both Rust artifacts: zero comments, trailing whitespace, production `unwrap/expect`, `try_send` and Rust test markers; exit 0. This audit only checks prohibited constructs and file integrity, not behavior or type correctness.
- Privacy production SHA-256 values were recomputed and still exactly match the previous freeze. Only its inventory/integration documentation was updated in this batch.

Not executed: Cargo, rustc/typecheck, Clippy, behavior/developer tests, CI, native app, native AT, network/Git/install/config operations. No Rust tests were added; no test success is claimed. Parent owns integrated compilation. The supplied source HEAD is `0b5ea426abfd8d4358489e916273d89474fb190f` plus preserved dirty state; this lane did not run Git to re-assert it. Final dirty artifacts are identified below rather than attributing them to a new commit.

Still unverified: delayed same-model replacement/revision/defaults/pending actions, owner/route/permission/window transitions, first-frame and post-scroll measurement convergence, narrow-window/model-name wrapping, Tab/Shift-Tab and actual Next/Previous action routing, Enter/Space and real IME, focus return/reveal, pointer/native focus handoff, per-leaf AX bounds/Focus/Click, renderer failure/reload, locales/themes and macOS/Windows/Linux. Fully/partially clipped leaves have no actionable AX proxy until actual measurement and geometry validate them. This implementation is not evidence of native AT success. Final integrated CI and centralized independent review remain parent-owned; no intermediate review is requested.

Rust SHA-256, relative to `crates/vega_ui/src/conversation_stream/`:

| Artifact | SHA-256 |
| --- | --- |
| `reasoning_feedback.rs` | `c9e55ec92ac328b9bdb749e45a2393eb252d82626e723c28084d0988c881317b` |
| `reasoning_feedback/leaf.rs` | `b3a072db70eb9e2682bf969a0fb03555c4bf499f5d06bbf5182e21bcfcb1ee81` |

Only these Rust files and `docs/907/nsh-reasoning-feedback.md` / `docs/907/nsh-privacy.md` were written in this batch. Utility bar, privacy production files, context production files, other lanes, shared files, dependencies and vendor remain untouched by this lane. Stop writing after delivery; parent continues integration and compilation.
