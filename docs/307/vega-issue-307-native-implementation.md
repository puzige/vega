# #307 independent native geometry/panel adapter

## Scope and baseline

READY for independent review and later card integration, not a claim that the card, native behavior, or issue is complete.

- Implementation baseline: `961387f9323084f8aaffd1583a0fb870e5a16dd5`.
- Parent supplied the fetch/merge of master `871f91f246e90753743a256cee1eec315e74effc`; local HEAD and `origin/master` matched those values before implementation.
- Final static inspection observed concurrent HEAD `212e50d5a0503fe462c2c2b5443c10c5656c4274` (`feat(307): retain permission guidance with task-scoped card decisions`). This writer did not create that commit. These three native-delivery files remained untracked and the native module remained unwired; the source hashes below were unchanged across that update.
- Contract: the latest scope at the top of [the main spec](vega-issue-307-cua-onboarding.md), the complete [native review](vega-issue-307-native-review.md), and the explicit compact API delegation. Older permission-prompt and automatic-completion requirements do not apply.
- This writer added only `crates/vega_ui/src/cua_onboarding/native.rs`, `crates/vega_ui/src/cua_onboarding/native/macos.rs`, and this report. No `mod.rs` wiring, controller/lifecycle/stream changes, UI, worker, placement policy, tests, dependencies, or CI changes. Concurrent controller-child edits are outside this delivery.
- No Git mutation, commit, push, fetch, builds, tests, Vega execution, or native execution was performed by this writer. Parent retains Git and integration ownership.

## Actual API

All exposed types, fields, and functions have `pub(super)` visibility within `cua_onboarding`.

| Item | Contract |
|---|---|
| `NativeError` | `Clone + Copy + Debug + PartialEq + Eq`; typed failures listed below. No logging inside the adapter. |
| `Screen` | `Clone + Copy + Debug`; `id: u64`, `frame: placement::Rect`, `visible: placement::Rect`, `appkit_frame: placement::Rect`. The first two rectangles use CG global top-left coordinates; `appkit_frame` uses AppKit global bottom-left coordinates. |
| `DesktopGeometry` | `settings: Option<placement::Rect>` in CG global top-left coordinates and `screens: Vec<Screen>`. Only copied Rust data escapes. |
| `geometry() -> Result<DesktopGeometry, NativeError>` | Read-only display discovery and System Settings window selection; no permission/AX/capture calls. |
| `configure_panel(marker: &str) -> Result<(), NativeError>` | Exact, unique title lookup within `NSApp.windows`, require `NSPanel`, then configure and read back style, level, collection behavior, and title visibility. |
| `move_panel(marker: &str, screen: &Screen, origin: (f64, f64)) -> Result<(), NativeError>` | Exact same panel identity checks; validate copied screen/origin, read the current native frame height, convert CG global top-left origin to AppKit, and dispatch `setFrameOrigin:` without animation. |

No `panel_rect` API was needed. The wrapper gates the implementation to macOS arm64/x86_64. Other platforms or unsupported architectures return `Unsupported` without linking AppKit. Every macOS entry verifies `NSThread.isMainThread` before AppKit access.

Errors: `Unsupported`, `NotMainThread`, `NativeUnavailable`, `EnumerationLimit`, `InvalidGeometry`, `InvalidMarker`, `PanelNotFound`, `PanelAmbiguous`, `NotPanel`, `PanelConfigurationRejected`.

### Discovery and coordinates

- Resolve all running applications with bundle ID `com.apple.systempreferences` and copy positive `pid_t` values. No process-name, foreign-title, or accessibility lookup.
- Call `CGWindowListCopyWindowInfo(1, 0)` for on-screen windows. Decode only correctly typed dictionaries/numbers, filter matching owner PID and layer 0, and choose the largest positive finite bounds by area. Missing/nil window lists, no matching windows, or unusable/redacted bounds yield `settings: None`, never a synthetic zero rectangle. Structural discovery failures and enumeration overflow are typed errors, not permission results.
- Enumerate `NSScreen.screens`, resolve `deviceDescription["NSScreenNumber"]`, and use that validated display ID with `CGDisplayBounds`. No index pairing and no GPUI `Display::bounds` substitute. Reject empty/invalid screen geometry and duplicate screen IDs rather than inventing a display.
- Screen discovery preserves native enumeration order and does not select a screen for the caller. Counts are bounded to 128 screens, 256 matching applications, and 16,384 windows; bounds are checked before indexing. Panel markers must be nonempty, at most 1,024 UTF-8 bytes, and contain no NUL.
- Rectangles require finite origins, positive finite dimensions, finite endpoints, and positive finite area. Invalid Settings candidates are skipped; invalid display/panel geometry stops the operation before movement.

With CG display frame `C`, corresponding AppKit frame `S`, AppKit visible frame `V`, desired CG-global top-left `P`, and actual current panel height `h`:

```text
visible = (C.x + V.x - S.x,
           C.y + S.height - (V.y - S.y) - V.height,
           V.width, V.height)

native origin = (S.x + P.x - C.x,
                 S.y + S.height - (P.y - C.y) - h)
```

These are point coordinates, with no Retina multiplication. GPUI initial origin `(P.x-C.x, P.y-C.y)` and matching `DisplayId::new(screen.id)` belong to the caller. Existing `perch`, `follow_target`, fallback, screen choice, threshold, drag freeze, and retry policy are not duplicated or modified here. Callers should pass the current discovery snapshot; the adapter does not cache or repair a stale display topology.

### Exact panel mutation boundary

Lookup scans our application's window array and compares titles with `isEqualToString:` against the caller's full marker. It does not read arbitrary UTF-8 buffers, use substring/prefix matching, or inspect foreign Settings titles. Any second exact-title match, including a non-panel window, rejects the operation. Only after the whole bounded scan establishes exactly one match is `isKindOfClass:NSPanel` checked. Missing, duplicate, wrong-class, and unavailable lookup paths perform no panel mutation.

Configuration preserves the marker and unrelated style/collection bits. It adds nonactivating style `1 << 7`, sets level 3, adds `canJoinAllSpaces | fullScreenAuxiliary` (`1 << 0 | 1 << 8`), and hides the title without replacing it. Already-correct properties are not set again. It reads all four properties back; a mismatch returns `PanelConfigurationRejected`. Such a failure can follow partial configuration and is not a rollback guarantee.

The caller must create `WindowKind::Floating` with `focus: false`, preserve the unique marker for the entire lifetime, configure immediately after creation, and log failures once. Lookup/configuration failure affects nonactivation and Spaces styling as well as following; it must not be described as only a following failure. Never substitute PopUp/level 101 or mutate a foreign Settings window.

Successful configuration means native property readback matched, not that runtime style mutation proves creation-time nonactivation, keyboard accessibility, or authorization-dialog layering. Successful movement means the validated native move was dispatched, not independent verification of compositor placement. There are no activation, front-ordering, permission, capture, or completion side effects.

## ABI and ownership evidence

Static source/header inspection only:

- Existing zero-crate precedent: `crates/vega/src/window/image_preview/native/macos.rs`, raw `objc_msgSend`, framework linking, and autorelease handling.
- Installed `objc 0.2.7`, `src/runtime.rs:16–33`: macOS arm64 BOOL is Rust `bool`; x86_64 BOOL is signed char. The adapter uses architecture-gated signatures accordingly, not one integer signature for every selector.
- Installed `core-foundation-sys 0.8.7`, `src/base.rs:14` and `src/number.rs:25`: CF `Boolean` is `u8`, `kCFNumberSInt64Type` is 4. The adapter keeps CF Boolean distinct from Objective-C BOOL and CoreGraphics C `bool`.
- macOS SDK `AppKit/NSWindow.h`: nonactivating bit at line 66; collection bits at lines 130/141; `NSWindowLevel` is `NSInteger` at line 192; hidden title at line 231; `setFrameOrigin:(NSPoint)` at line 350. Getters/setters use `isize` for NSInteger and `usize` for NSUInteger; process ID uses `i32`.
- macOS SDK `CoreGraphics/CGGeometry.h:276–278`: `CGRectMakeWithDictionaryRepresentation` returns C `bool`. `CGDisplayBounds` is declared as a direct C aggregate return, allowing the compiler to use the target ABI.
- Geometry is `repr(C)` with 64-bit CGFloat fields: `NSPoint`/`NSSize` are two doubles and `NSRect` is four doubles. `setFrameOrigin:` receives `NSPoint` by value. `frame`/`visibleFrame` dispatch uses ordinary `objc_msgSend` on arm64 and explicit result-pointer `objc_msgSend_stret` on x86_64; the unsupported-architecture gate prevents an accidental ABI fallback.
- Installed `gpui-pre-macos 0.3.4`, `src/display.rs:36–43`: established `NSScreenNumber` identity lookup. Its `window.rs:1127–1130` confirms Floating level 3; the only explicit initial collection-behavior assignment is in the PopUp branch at lines 1164–1166, not the Floating branch.
- Private RAII `AutoreleasePool` is created after the thread check and dropped on every Rust return path. Private RAII `OwnedCf` releases every non-null Create/Copy result (`CFStringCreateWithBytes`, `CGWindowListCopyWindowInfo`). Borrowed array entries, dictionaries, titles, screens, and windows stay within the synchronous pool; no native pointer is returned publicly or stored between calls.

## Validation and exact content

Initial rustfmt precheck found only formatting differences in the new sources. Formatting was applied only to the two owned Rust files. No assertion/test changes were made to obtain a passing check.

| Check | Result |
|---|---|
| `rustfmt --version` | `rustfmt 1.9.0` |
| `rustfmt --edition 2024 --config skip_children=true --check crates/vega_ui/src/cua_onboarding/native.rs crates/vega_ui/src/cua_onboarding/native/macos.rs` | Exit 0 after targeted formatting. This is parsing/format validation, not compilation or native execution. |
| `git diff --no-index --check /dev/null <each of the three delivery files>` | Exit 1 for each because new content differs from `/dev/null`; no whitespace diagnostics. The wrapper was corrected to distinguish this content-difference exit from a whitespace failure. |
| `git diff --exit-code 961387f9323084f8aaffd1583a0fb870e5a16dd5 -- crates/vega_ui/src/cua_onboarding/placement.rs crates/vega_ui/src/cua_onboarding/bundle.rs` | Exit 0, no changes. |
| Focused source inspection/search | Only adjacent one-line SAFETY comments; no `unwrap`, `expect`, `panic!`, GPUI dependency, placement-policy calls, AX/TCC/capture bridge, title replacement, or activation/front-ordering calls. Static evidence only. |
| Builds, type checking, lint, tests, CI, native execution | NOT RUN by this writer. Module intentionally remains unwired, so unrelated checks cannot establish this adapter's compilation. |

SHA-256 of the final Rust sources:

| File | SHA-256 |
|---|---|
| `crates/vega_ui/src/cua_onboarding/native.rs` | `122a7e9126862ce1beac7bb5c0fecd1a3430b643a5d77097cf1efdabc7d31fb4` |
| `crates/vega_ui/src/cua_onboarding/native/macos.rs` | `e5c65124751445bb5edcec0db7bc7fd27133aef8cb19fcc7f994598b0faf03f0` |
| Unchanged `crates/vega_ui/src/cua_onboarding/placement.rs` | `dac21fe985a387a6ea52b5e7cd84dac478ee669217c0991bb9df2bebe359d31e` |

Parent-provided historical evidence, not rerun or claimed as native coverage: 15 frozen UI-decision tests received effective interface-missing RED at `bdce33a5837234fe7b18f07c621a6c7965eb4da2`, run `39811963094`, reconfirmed at `961387f9323084f8aaffd1583a0fb870e5a16dd5`, run `39813558507`, for missing controller/lifecycle APIs. Stage-one placement/bundle tests previously had 35/35 GREEN. Native/TCC integration remains expressly manual; this writer added no automated native coverage and did not alter those frozen tests.

## Remaining integration and manual checks

Parent/card implementer owns module wiring, caller/error handling, initial GPUI display-local placement, the follow loop, close cancellation, stale callback rejection, drag freeze, controller/lifecycle, and diagnostics. Cloud compilation/lint of the wired adapter remains pending; neither target ABI has been executed in this delivery. Independent review must bind to the source hashes above.

User manual checks remain necessary after integration:

1. Actual unique `NSApp.windows` panel matching, retained hidden marker, no wrong-window mutation, and usable fallback/diagnostics when matching or styling fails.
2. Settings discovery with missing/redacted windows and multiple layer-0 windows; largest-area selection is a heuristic, not proof of authorization-dialog identity or ordering.
3. Correct placement/movement on displays above, below, and left of the primary, differing Retina scales, menu bar/Dock visible bounds, changed card height, and display topology changes.
4. Initial `focus: false`, runtime nonactivating behavior, keyboard-accessible card actions, Spaces/fullscreen visibility, and authorization dialogs remaining unobstructed. Property readback alone proves none of these user-visible outcomes.
5. Actual follow smoothness, loss/reappearance behavior, permanent drag freeze for the lifetime, native file drop, and no movement or work after close. These involve caller integration not implemented here.
6. Real TCC permission changes and restart/reauthorization behavior remain separate manual integration acceptance; this adapter neither queries nor changes permission state and cannot establish desktop readiness.
