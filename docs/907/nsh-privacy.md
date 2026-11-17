# NSH privacy review and compatible controls

## Scope and acceptance

This lane migrates `conversation_stream/privacy.rs` and its dedicated submodules. All visible review text and controls must be constructed by NSH; Fast may retain structural layout, scrolling, focus/event plumbing and invisible native accessibility nodes. Existing `PrivacyButton::new(...).label(...).on_click(Fn(&(), &mut Window, &mut App))` remains source-compatible.

- Preserve pending/error/empty/review states, finding source, redacted preview, details, message location, eligible false-positive exceptions, permanent-clean preview/confirm/cancel and close.
- Only `PrivacyFinding::preview_text()` enters the visible preview. Never project `preview`, raw credential bytes or add a new credential-reading path. Existing authorized backend review, exception and cleanup services remain unchanged; errors/unknown are never presented as clean.
- Business actions carry original review snapshots. Reject stale owner/thread/model/backend/navigation epoch, review, confirmation/details/pending state, input revision, permission or route observations. Permanent cleanup still requires the actual current confirmation state and cleanable findings.
- Each leaf owns a real NSH surface, a current measured displayed row, an independent focus handle/native AX label/button and a mount lease. No composite accessibility node may stand in for a multirow panel. Pointer, keyboard and native AX activation all use the same current-display fence. Unmount stops the NSH view; hidden/partially clipped/inactive/stale controls cannot activate.
- Preserve labels, enabled state and necessary pending reasons, focus affordance, Enter/Space without modifier/IME/repeat interception, and Tab traversal. No GPUI visible label/control painting is retained.
- A failed NSH renderer must not fall back to GPUI product painting or replay unknown business commands. Recovery is a separately created NSH retry control; it replaces only the failed view, not the business operation.

## Validation plan

Only scoped rustfmt/static inspection is authorized in this lane. Parent owns Cargo/typechecking and any developer behavior execution. Real native focus/AT, scrolling, theme/locale/narrow-window and all desktop-platform checks remain independent and must not be claimed by static evidence.

## Implemented delivery and parent wiring

The panel now delegates all pending/error/empty/source/preview/details/warning text and all seven action families to `privacy/control.rs`. GPUI retains only structural flex/scroll layout and invisible measured native accessibility proxies. The old GPUI interaction surface and painted fade were removed; no visible legacy control is embedded in NSH. Each leaf dynamically measures its real NSH content height and converts `Measurement.page` plus `frame.size` into a native row rectangle. Publications are deduplicated by presentation identity so a Displayed ACK does not destroy its own row lease. Focus entry requests actual host autoscroll; every activation consumes the displayed identity and must obtain a new measured observation before another activation. Failure callbacks are fenced by the exact failing Surface entity. Window activation/visibility subscriptions retire old displayed identities across hide/deactivate/reactivate round trips.

Public button listener type is unchanged. Parent has connected the explicit source binding to the external production caller, without changing its existing callback:

```diff
--- a/crates/vega_ui/src/conversation_stream/render.rs
+++ b/crates/vega_ui/src/conversation_stream/render.rs
@@
                                     super::privacy::PrivacyButton::new(
                                         "composer-skill-settings",
                                         colors,
                                     )
+                                    .owner(self, cx)
                                     .label(vega_i18n::t("ui.open_skill_settings"))
```

No shared file was written. Existing panel render signatures need no changes. Future callers without `.owner(...)` remain source-compatible and render, but activation intentionally fails closed rather than guessing an owner for a delayed callback.

## Actual checks and remaining acceptance

- `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`; scoped `rustfmt --edition 2024 --config skip_children=true` and final `--check` on both Rust artifacts: exit 0.
- Read-only Python static audit: both Rust files have zero comments, trailing whitespace, production `unwrap/expect`, or `try_send`; exit 0. This is not a behavior test or typecheck.
- No developer Rust tests were added or executed. Cargo/rustc/Clippy/CI/native app/AT were NOT executed; parent owns integrated compilation. No Git/network/native/install/config/dependency/vendor/shared-file operation occurred.
- All three desktop platforms, NSH wrapping/height convergence, scroll reveal and clipping, Tab/Shift-Tab, pointer versus native focus handoff, Enter/Space and real IME, per-leaf native AT bounds/Focus/Click, renderer failure/reload, delayed actions after hide/remount/route/permission/review changes, and themes/locales/narrow windows remain UNVERIFIED. The implementation is not evidence of successful native AT delivery.
- Fully offscreen or partially clipped leaves have no actionable native proxy until current geometry and measurement show them. No composite AX fallback is used. Retained host focus handles request scrolling on entry; actual keyboard reveal still needs platform validation.
- Removing the old GPUI scroll-edge painted fade is deliberate: no GPUI visible painting remains in this panel. Scrolling and all content/actions remain present; edge appearance is pending visual acceptance.
- Privacy implementation remains frozen for parent typecheck/integration; this is not a claim of all-product migration completion. Context inventory is resolved and nonblocking as below; utility bar was not edited.

Rust SHA-256 (paths relative to `crates/vega_ui/src/conversation_stream/`):

| Artifact | SHA-256 |
| --- | --- |
| `privacy.rs` | `77a43eb81cf2d28cbf92e840b0c4ccb828e9de2cf53225a4ccb76840328088ce` |
| `privacy/control.rs` | `e3235abea3dcdbd95747474bf8bf6aa872130fa292d9811613de278a06d7d455` |

## Context inventory resolved — no migration target, nonblocking

Parent and this lane checked `context_control.rs` and `docs/76/vega-issue-76-model-context-correction.md:15–33`: there is no existing production context settings panel to migrate. The earlier task package's panel assumption was incorrect, not a new user product requirement, and that subgoal is withdrawn. Legacy field entities, save ACK and compaction controllers do not authorize inventing a numeric parser or restoring a per-conversation capacity/reserve/automatic editor. Providers remains the model authority; the production usage indicator in `foreground_chrome/context_usage.rs` is already NSH and slash compaction remains independent. This inventory finding is not a blocker and requires no user product decision. No context production code, parser, authority or full-product scope was changed.
