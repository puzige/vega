# Stream fade range projection for native reading

## Scope and acceptance before implementation

Expose existing StreamFade decisions without producing legacy GPUI TextRuns. Do not change the 700 ms normal, 150 ms finish, 24 ms segment spacing, 800 ms queue cap, 2500 ms maximum age or reduced-motion/inactive behavior. Keep the old TextRun consumer available until native application integration is finished. No dependency or rendering change belongs to this small application-only increment.

A native consumer requests a part using the existing exact `(block ID, logical line, table row, TextPart)` identity plus the full expected displayed part text. Return that part's existing sorted UTF-8 byte ranges and alpha only when the expected string exactly matches the source captured in this frame. Equal display strings in a different block/part do not substitute for identity. A stale frame must reject newer appended content, and a replacement that is not a prefix clears previous part fades. The native adapter remains responsible for verified fragment mapping and UTF-8 to scalar-safe UTF-16 conversion; these displayed-part offsets are not raw Markdown source offsets.

Store one shared immutable part text per update and clone only its Arc into active frames, rather than cloning a growing paragraph on every animation frame. Each range remains tied to the captured text even after later StreamFade updates.

## Required development regressions

1. Body/marker/cell identities and exact expected text fence range access; multibyte append ranges retain UTF-8 boundaries and do not bleed to another part.
2. Source appended after a frame cannot repurpose the old frame; a non-prefix replacement invalidates active fades for that part.
3. Segment alpha and legacy TextRun alpha use the same values; range access does not alter input data or timing.
4. Finish converges from current alpha within 150 ms, age cap clears queued ranges, disabled motion clears active state and cannot leave a later half-opacity frame.
5. Existing line TextRun styling remains unchanged, including foreground, background and explicit underline/strike alpha.

This increment alone does not attach native opacity, prove actual paint or native acceptance. The body writer and backend opacity writer have separate scopes; their later integration must revalidate mounted projection/current snapshot and clear inactive/hidden overrides.
