# R37: preserve visible Pinned padding through scroll clipping

User screenshot disproves R36 native acceptance: the negative row margin is
outside the Pinned scroll viewport, so the left padding and rounded corners
are clipped despite correct layout bounds. R36 delivery's native PASS is
superseded by this correction.

Keep R36 title alignment, 8px leading surface padding, trailing edge, and R35
section spacing. Make the scroll viewport encompass the complete row surface:
move the leading outset to the scroll container or use equivalent clip-safe
container geometry, retaining real vertical scrolling for more than five pins.
Do not disable clipping globally or move the title column.

Acceptance requires installed-app screenshots showing the full left rounded
corners and visible padding for selected and hovered rows. Layout bounds alone
cannot establish success. Inspect ancestor clip bounds as appropriate. Preserve
scrolling, row actions, and other sidebar rows. Update an existing regression
if useful; avoid duplicating coordinate-only tests. Required repository fmt,
clippy and workspace tests must pass. No dependencies or new tokens.
