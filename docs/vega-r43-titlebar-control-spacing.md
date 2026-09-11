# R43 Titlebar control spacing

The shared `Sidebar, Search, Back, Forward` titlebar group must use one visual
grid. Every control owns an exact 28px square interactive surface, contains a
centered shared 16px outline icon, and is separated from the next control by
exactly 4px. These shared values are typed `Layout` tokens.

The current mixed implementation gives Search a narrower intrinsic surface
while Sidebar and history controls include horizontal padding. As a result,
the first two icon-center distances differ from Back-to-Forward even though
the container uses one gap. Remove that intrinsic-size difference; tests must
measure both equal surfaces and equal icon-center spacing.

Disabled Back / Forward controls retain their 28px square slot so navigation
availability never shifts neighboring icons. Hover and focus backgrounds fill
the same 28px square and keep the existing rounded neutral treatment.

Preserve the R41 order, Search/Sidebar/history actions, accessibility labels,
tooltips, keyboard behavior, icon artwork, titlebar height, Sidebar geometry,
and Search palette. Do not add separators or labels.

## Acceptance

- All four production-mounted controls measure exactly 28×28px.
- Each adjacent pair has a 4px surface gap and therefore a 32px center-to-center
  distance, with icons centered on the same vertical axis.
- The measurements hold with the Sidebar visible/hidden and Back/Forward
  enabled/disabled.
- Search, Sidebar, Back and Forward interactions continue to work.
- Formatting, strict Clippy, workspace tests, packaging and a native macOS
  screenshot walkthrough pass.
