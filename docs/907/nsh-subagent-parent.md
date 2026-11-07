# Subagent parent navigation — production NSH migration

## Acceptance before implementation

Base HEAD `bd89db7d182e6d9a5c60b1388e79a4623e8cacc4`. Main Agent owns this leaf and shared conversation registration/lifecycle hooks; other writers own sidebar, commit panel, tool activity group and workspace. Preserve `SubagentOpenRequested { source_thread_id, thread_id }` and all subagent business state. Migrate only the production Back to parent control in `conversation_stream/subagents.rs`; do not modify subagent result/permission/usage ownership.

- NSH paints label, border, hover/press/focus and explicit presentation recovery. Fast supplies only layout, native host focus/key/AX and mount lifetime.
- Real native Tab stop; Enter/Space ignore repeats, modifiers and character-input preference. No text-input keymap bindings or dependency changes.
- Pointer, keyboard and retained AX invocation require the current source/parent route, displayed projection, mounted lease, non-preview visible state and closed Settings. A prior source or hidden stream cannot navigate.
- Parent stream suspension and reading-preview entry explicitly revoke presentation; unmount/recycle weak leases independently reject retained requests. Presentation hide does not cancel business work.
- Recovery is explicit UI-only reload, no automatic navigation replay. All commands use existing OrderedCommands; errors close authorization rather than silently dropping operations.
- Keep existing label/token sizes and public event semantics. Do not add explanatory success text.

## Verification plan

Meaningful development regressions cover stale route/session/mount and hidden/preview/Settings authorization plus recovery-only action routing. Parent runs coordinated compilation, strict Clippy/fmt and tests after all active writers stop. Existing 124-test green applies only to the preceding palette/branch delta, not this new code. Non-implementer independent review is required before final delivery. Native focus/AX/pointer/three-desktop behavior remains unverified. Development Rust tests must be archived and removed before merge.
