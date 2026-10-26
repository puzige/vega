# #307 phase2 driver integration blocker

## Checkpoint

- Local HEAD: `92f281e5a5a56230fa4e4a8a8cf1ab6f20e9670b`, synchronized with master; PR #537 remains draft at `7d855493f019610541e3f7bca4cb3ace4cd3ef89`.
- No phase2 Rust changes. Test-author execution stopped and confirmed interrupted before any Rust writes. Only phase2 planning/spec documentation is uncommitted.
- No local compilation, application execution, tests or native permission operations.
- Stage1 classification GREEN does not establish real driver integration. No phase2 RED/GREEN exists.

## Verified upstream evidence

Pinned driver tag `cua-driver-rs-v0.34.0` resolves to commit `b0968e1b12834e485dda68789541a3cc57664a9f`.

[check_permissions.rs](https://github.com/trycua/cua/blob/b0968e1b12834e485dda68789541a3cc57664a9f/libs/cua-driver/rust/crates/platform-macos/src/tools/check_permissions.rs):

- Lines 162–168: live capture requires `should_prompt && screen_recording && probe_direct_capture`.
- Lines 170–172: `should_prompt` requires requested AND NOT embedded mode AND NOT host-owned permission UX.
- Lines 372–413: embedded mode disables prompt regardless of argument; read-only requests return `screen_recording_capturable: null` and `direct_capture_status: not_checked`.
- Tool description lines 313–337 confirms the live probe can itself raise separate direct-capture consent, so it never runs with prompt false.

[permission_prompt_authorization_test.rs](https://github.com/trycua/cua/blob/b0968e1b12834e485dda68789541a3cc57664a9f/libs/cua-driver/rust/crates/cua-driver/tests/permission_prompt_authorization_test.rs) asserts that standard-mode socket `check_permissions(prompt:true)` is refused with `os_permission_prompt_requires_trusted_host`.

These are upstream static sources, not locally executed test results.

## Consequence

Current `probe.rs` invokes the public MCP route with embedded/standard identity and maps UserInitiated to prompt true. That route cannot produce the fresh live-capture Ready required by R3/R18. Sending false/true cannot repair this; removing embedded identity or using standalone grant would measure/change a different responsibility identity and is not an acceptable workaround. Existing evidence-store fields are not equivalent to fresh live readiness and must not silently replace it.

This overturns the original F2/F3 assumption that choosing prompt true on a host-initiated call makes the existing MCP session a trusted permission setup route. The phase2 test plan's proposed non-prompting live recheck is likewise unsupported by the pinned driver and must not be frozen as achievable integration behavior.

## Next architectural decision

Preferred direction: preserve embedded MCP for passive permission status and add an explicit Vega-host native permission/live-capture setup path, only from user action. Freeze its responsibility identity, consent lifecycle, bounded cancellation, result mapping and test seams before implementation. Real host identity/consent still requires manual integration validation; no model tool gains prompting capability.

Alternative scope change: ship following/draggable guidance and passive permission status only, visibly preserving capture as unverified and deferring verified-ready/automatic completion. This does not satisfy current full acceptance and requires explicit user approval.

Do not proceed to final delivery or claim full readiness until this architecture/acceptance decision is resolved. R19 administrator-dialog stacking also remains manual verification, not a consequence proven by level 3 alone.
