# Terminal selection, clipboard routing and inverse backgrounds

## Goal and evidence

The user reports both failed mouse selection / Cmd+C and invisible pasted text.
`TerminalView` has no selection model. R11 deliberately deferred selection and
copies the whole viewport. Clipboard actions use global key bindings.
The renderer supplies TextRun backgrounds but calls only ShapedLine.paint;
gpui-pre 0.3.4 requires a separate paint_background call. Inverse default colors
therefore produce light glyphs without the intended dark background in light mode.
This is a source-level diagnosis, not a claim of native reproduction.

## Contract

- Left-button drag inside the terminal viewport creates a visible, half-open,
  row-major selection. Reverse drag works. Mouse release, including outside the
  viewport, ends dragging while preserving selection. A click clears selection.
- Cmd+C in Terminal copies selected text, with newlines between selected rows;
  no selection retains the existing whole-visible-screen copy behavior. Wide
  continuation cells are not duplicated. Partial-line spaces are preserved;
  trailing padding at a selected row end is removed as for screen copying.
- Explicit Terminal-scoped Cmd+C / Cmd+V override unrelated/global bindings.
  Ctrl+C remains PTY interrupt. No automatic clipboard writes or shell commands.
- Input, restart, scrolling, viewport resize and a new terminal snapshot clear
  selection to prevent copying coordinates against changed content. Selection
  is confined to the visible snapshot; no cross-scrollback drag/autoscroll,
  word/line multiclick selection or terminal mouse reporting is added. Viewport
  geometry is independent of PTY resize acknowledgement; exited-shell output
  remains selectable after resizing, and copy never includes clipped rows/cells.
- Paint each line background before glyphs. Both light and dark themes preserve
  default, ANSI/RGB and inverse backgrounds; selection uses the existing brand
  foreground/background token pair and overrides terminal attributes locally.
- Existing PTY service, shell configuration, input limits, bracketed-paste and
  permission boundaries are unchanged. No new dependencies or product copy.

## Implementation plan

1. `crates/vega_ui/src/terminal.rs`: viewport geometry, drag endpoints and mouse
   handlers; selection-aware copy and safe invalidation; background paint pass.
2. `crates/vega_ui/src/lib.rs`: Terminal-scoped clipboard bindings.
3. Keep the old R11 contract as historical evidence; this contract supersedes its
   selection-deferred behavior only.

## Validation

Local checks are formatting and diff/static inspection only. Do not run Vega,
compile, build or execute tests locally. No Rust tests are introduced. Independent
read-only review and final-head required Actions are required before merge.
Native acceptance remains the user's manual check: forward/reverse multiline
selection, Chinese/wide characters, selection vs whole-screen copy, Ctrl+C,
plain/multiline pasted text with zsh inverse highlighting in light and dark
mode, ANSI backgrounds, selection invalidation and exited-shell selection after
resizing. CI does not prove native UI
or clipboard behavior. The source-level routing fix does not prove the user's
original Cmd+C failure had only one cause.
