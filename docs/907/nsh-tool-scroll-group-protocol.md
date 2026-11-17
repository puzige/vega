# Tool group intermediate scroll protocol

## Acceptance before implementation

This author implements the real Fast tool-group ancestor, not independent review. NSH 0.4.0 / Fast 0.1.9 / Kit 0.7.3 and existing dependencies remain fixed. The original group Div, ScrollHandle, maximum height, row order, scrollbar and original conversation List remain authoritative. No second vertical offset, OS event replay or Fast change is allowed.

The synchronous order is native child → mounted NSH ancestors → real Fast group ScrollHandle → enclosing scope → original List. Each consumer receives only residual axes. Group consumption is measured from actual same-event offset before/after an explicitly clamped ScrollHandle::set_offset; post-default-scroll inference is forbidden. Original units, mixed axes, terminal nonzero delta and zero-motion lifecycle events survive forwarding. Vertical-only group scrolling retains Fast's horizontal fallback only when Y is zero. Mixed X/Y consumes only Y and retains X.

A child retains first refusal. A listener inside the original group Div, before its content children, handles ordinary unclaimed events after child bubbling but before the Div's default listener. Explicit remainder delivery stops the original event before the default Div/List listeners can consume it again. A full untouched event may bubble only when no explicit delivery or group ownership occurred. Gesture continuation retains the original group path across pointer movement; cancel, terminal-tail expiry, enclosing scope replacement, unmount, inactive/hidden window and expiry revoke that authority. OS momentum is consumed directly; no additional fling is started.

## Required parent-coordinated API

The inspected vendor ScrollScope exposes only new/cancel/with; its current scope, stable identity and consumer are private. This lane will not edit vendor or crate nsh/scroll.rs. Parent must expose these exact methods on ScrollScope:

- `pub fn current() -> Option<Self>` returning the currently bound scope clone.
- `pub fn same(&self, other: &Self) -> bool` comparing the existing identity Rc by pointer.
- `pub fn consume(&self, remaining: ScrollRemainder, window: &mut Window, cx: &mut App)` invoking the existing synchronous consumer, without an event replay or a new listener.

Also expose `pub fn from_event(event: &ScrollWheelEvent) -> Self` on ScrollRemainder. It must use existing events::scroll_delta, the same nonprecise Shift/X-zero axis normalization as input::wheel, and existing scroll_bridge::remainder with a default unconsumed ScrollOutcome. This avoids duplicating the platform-specific native line scale in the UI crate. Keep phase, modifiers and pointer unchanged.

The production source now uses these four methods; compilation remains blocked until parent adds them. No crate nsh/scroll.rs change is requested. Parent retains responsibility for original List notification, direction/follow interruption, source/body eligibility and shared native/NSH lifecycle.

## Source evidence

Fast 0.1.9 `elements/div.rs` installs custom mouse listeners then the default scroll listener before painting children; Window dispatches capture in registration order and bubble in reverse. The default listener adds raw delta to the shared offset without immediate clamp; prepaint later clamps. ScrollHandle::set_offset changes the actual shared offset/version but does not clamp or notify. Therefore a first-child listener is required to intercept before default handling and explicit consumption must clamp and notify.

Current tool_activity_group.rs uses overflow_y_scroll/track_scroll and contain_disclosure_scroll. That containment runs after the default listener and infers before from after; it cannot preserve partial remainder. Existing ScrollScope::for_native consumes real NSH ancestors and then calls its captured parent consumer, allowing a real intermediate group scope without replacing native ancestry.

## Implemented bounded delta

`disclosure::group_scroll_chrome` preserves the existing scroll_chrome and wraps its prepaint/paint in a retained intermediate ScrollScope. `disclosure::group_scroll_listener` is an absolute zero-size first child of the existing Fast scroll Div; it adds no flow row/gap/offset. ToolActivityGroup alone adopts both entry points and removes its old contain_disclosure_scroll listener. ToolCard/run-activity containment and all other active lanes are untouched.

The intermediate consumer clamps against the real ScrollHandle maximum, writes its original offset, rereads actual movement, subtracts only the consumed original-unit component and synchronously invokes the captured parent scope. The original parent is cloned outside all State borrows before invocation. A group-first ordinary gesture retains only ownership metadata, never a second offset; an NSH-first gesture remains owned by its existing native route, so group delivery does not create a competing capture owner. Zero-motion lifecycle messages are forwarded. A terminal event applies its delta once and retains a short continuation tail using the existing NSH TOUCHPAD_IDLE constant; a subsequent OS momentum event updates that tail without starting a fling. Cancel clears ownership immediately. Mouse latching uses NSH's existing axis/time predicate. Scope identity changes, mount lease drop, clipping out, scale/viewport changes and inactive/hidden window retire the group route. Each callback captures its original generation, so retained replaced/detached scope consumers cannot revive on a no-op ABA.

The group uses Fast window line_height for its own line scrolling and retains line units for the parent, whose original List conversion remains 20 points per line. Outcome accounting allocates the same actual consumed fraction to the NSH native axis; nonprecise Shift/Y-only normalization is undone only at the protocol delta seam. Finite validation rejects NaN/infinite deltas and invalid current geometry without writing offsets.

## Development regression calls

Feature `nsh-reading-integration` declares `disclosure::scroll_group::host_regressions`. Parent may re-export these five public functions from lib.rs for its existing main-thread TestAppContext runner; this lane does not modify lib.rs or runner:

1. `ordinary_group_partial_and_mixed_axes_reach_original_list_once(cx)`
2. `group_horizontal_fallback_and_line_units_use_actual_offsets(cx)`
3. `group_owner_survives_pointer_change_and_terminal_delta_has_no_glide(cx)`
4. `replaced_scope_and_detached_group_revoke_retained_remainder_consumer(cx)`
5. `real_body_then_group_then_original_list_preserve_partial_remainder(cx, factory: BodyFactory)`

The fifth uses the existing production body factory, actual geometry and actual core offset positioner. It first verifies the real inner 8-point remainder and 24-point group consumption while body geometry remains visible, then continues the same owner through group exhaustion to the original List. The other fixtures mount the actual Fast scroll Div, real ScrollHandle, production scroll chrome and original List, not a mirrored algorithm or fake stable geometry. Existing budgets are preserved: mount 10 seconds, drain 2 seconds / 512 ticks, no-glide observation 1100 milliseconds. No existing tests or assertions are changed.

## Validation status and unresolved integration

Authored five regressions; executed zero. Scoped rustfmt plus --check use rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18), edition 2024 / skip_children=true. Static hygiene checks are not behavioral proof. No Cargo/rustc, Git, network, app launch, delegation, Fast/Kit or dependency write occurred. Parent centrally adds the four APIs, compiles, exports/runs the five calls, and checks final exact integrated bytes.

Required parent validation still includes nativechild → NSHancestor → group → List with a real native child (the production-body regression is Surface/body rather than independent native-component authority), source focus/revocation, narrow/clipped/retained groups, ordinary native editor first refusal, inactive-window lifecycle, mixed Shift/RTL paths, visual scrollbar/follow direction notifications, and native acceptance. Existing shared scope/native generation/owner focus rules remain authoritative; this group wrapper does not invent a replacement source owner check. The private ScrollHandle has no public identity comparison; the integration relies on ToolActivityGroup's existing stable handle paired with its entity-keyed id, not arbitrary handle replacement under the same id. Existing source14 scheduler timeout and nested11 List RefCell issue belong to other authors and are not modified here.

Until parent implements the four shared APIs, these files are deliberately an uncompiled integration handoff, not a passing or complete production claim. After parent compilation/regressions, consolidated review remains separate. This author stops writing after final hashes.
