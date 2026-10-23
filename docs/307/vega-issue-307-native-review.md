# #307 native integration review checkpoint

Read-only review baseline `92f281e5a5a56230fa4e4a8a8cf1ab6f20e9670b`; installed registry sources gpui-pre/gpui-pre-macos 0.3.4. No native execution, compilation or tests. New scope in the main spec overrides prior automatic completion assumptions.

## Coordinate boundary

CG window bounds use global top-left coordinates. GPUI window bounds are display-local top-left coordinates. NSScreen uses AppKit global bottom-left coordinates. Pair CG display and NSScreen by display ID, never enumeration index.

Let C be CG display frame, S corresponding NSScreen frame, P desired CG-global card top-left, h actual native frame height:

- GPUI initial origin: `(P.x-C.x, P.y-C.y)` with matching display_id.
- AppKit move origin: `(S.x+P.x-C.x, S.y+S.height-(P.y-C.y)-h)`.
- AppKit visible V converted to CG global: `(C.x+V.x-S.x, C.y+S.height-(V.y-S.y)-V.height, V.width, V.height)`.

Native window.rs 981–999 initializes without subtracting height, but 1226 calls setFrameTopLeftPoint. Do not add compensating height to initial GPUI coordinates. Readback 835–856 subtracts native frame height. Native display.rs bounds() discards CG origin; do not mistake it for global coordinates. Coordinates are points, not Retina pixels.

## Window discovery and ownership

Resolve System Settings by com.apple.systempreferences running-app PID, then CGWindowListCopyWindowInfo on-screen layer0 candidates; use valid positive finite bounds, largest area. Do not depend on foreign window titles (may be redacted) or require AX permission. Largest area is a targeting heuristic, not proof of authorization-dialog layering.

Own card: unique per-lifetime title, exactly one matching NSPanel in NSApp windows, main-thread only. Zero or duplicate matches mutate nothing. GPUI set_window_title remains available; application must preserve marker. Do not retain unowned native pointers across ticks. If lookup fails, nonactivation/Spaces correction may also fail; do not claim only following is affected.

## Panel/ABI

Floating allocates NSPanel and sets level3; lacks nonactivating bit. Create focus:false; otherwise GPUI can makeKeyAndOrderFront before correction. Read-modify masks, apply nonactivating bit and canJoinAllSpaces/fullScreenAuxiliary once. Runtime style mutation does not prove creation-time nonactivation semantics. Keyboard focus accessibility and system-dialog occlusion remain manual checks.

ObjC geometry requires repr(C), 64-bit CGFloat, NSPoint by value for setFrameOrigin. NSRect return ABI differs on x86_64 (stret) versus arm64. Guard architecture or use correct ABI; do not transmute an object-return function to geometry return indiscriminately. Nil/invalid geometry is a failure, not valid zero coordinates. CF Copy objects must be released and autorelease pools balanced.

## Drag

Use on_drag followed by external_drag_payload for the same type; Files(FileDragPaths::new([(bundle,true)])). Promotion occurs only when pointer leaves whole viewport while left button held. Freeze synchronously in constructor for the rest of this card lifetime, including cancel/failure. has_active_drag becomes false during native handoff and is not a completion signal. Native Ended clears internal bookkeeping but does not expose accepted drop result. Drop never grants permission readiness.

## Layout and remaining checks

460x140 was stage1 test input, not frozen actual view size. With 24 padding on all sides its visible body is412x92, insufficient evidence for actual layout. Size the actual card to preserve labelled permission actions, recheck, close, failures/restart information and keyboard access; no clipping or fabricated success. Use existing theme tokens. Initial missing-settings retry must be bounded; later loss retains last valid position. Cancel loops and invalidate queued work before close. Actual native follow smoothness, multi-screen placement, system drop, focus, Spaces and authorization-dialog visibility require user manual verification.
