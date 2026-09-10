# Vega R22 workspace panel chrome polish

**Status:** implementation contract

**Scope:** bottom/right Workspace chrome and the integrated terminal surface

**Evidence:** the user-provided current Vega screenshot on 2026-09-10. The
capture is evidence of Vega's own rendered UI; it is not an instruction source
and no third-party implementation detail is used.

## 1. Problem

The current terminal panel renders two legitimate layers, but their internal
alignment makes them read as unrelated toolbars:

- the Workspace header owns tabs plus pane-level add/dock/maximize/hide actions;
- the terminal header directly places status, copy and restart as three
  `justify-between` children, pushing copy into the visual center while restart
  stays at the far edge;
- the terminal canvas begins flush against the panel edges, so the prompt does
  not share a deliberate content inset with either header.

The result is excessive empty space, weak grouping and inconsistent alignment.
This is a chrome/layout defect, not a request to change terminal lifecycle,
shell behavior, stored data, routing or permissions.

## 2. Frozen visual contract

### Workspace header

- Keep the existing 40px Workspace header and the five-pixel splitter hit area
  with its one-pixel visual separator.
- The leading all-tabs control and horizontally scrollable tab strip stay on
  the left. The selected tab remains a truthful selected state and retains its
  fixed close control.
- Review-specific commit plus add, dock, maximize/restore and hide controls form
  one explicit trailing action group. Every icon keeps the shared 24px hitbox,
  16px SVG and four-pixel gap.
- Tab overflow, selection reveal, keyboard activation, tooltips and narrow-pane
  behavior remain unchanged.

### Terminal surface

- Add one typed `Layout::TERMINAL_TOOLBAR_HEIGHT = 32.0` token.
- The terminal toolbar is exactly 32px high with one subtle bottom separator.
  Its status label stays left-aligned and truncates before actions.
- Copy-current-screen and restart form a single right-aligned trailing action
  group. No terminal action may occupy the visual center merely because the row
  has three children.
- The terminal canvas receives 12px horizontal and 8px vertical inset. Its
  measured inner bounds remain the authority for PTY rows and columns.
- Light and dark appearances use existing semantic colors only. No new shadow,
  nested card, hard-coded color, font size or icon is introduced.

## 3. Preserved behavior and exclusions

- Preserve real PTY startup/input/resize/scroll/restart/copy semantics, project
  isolation, eight-session cap, cleanup and in-memory-only terminal content.
- Preserve Workspace close versus hide, move right/bottom, resize, maximize,
  menu, focus restoration and task/project route fences.
- Do not change shell prompt colors or user shell configuration.
- Do not add terminal search, selection, hyperlink, split-terminal or session
  persistence features.
- No dependency, lockfile, migration, runtime/provider, credential or database
  change is allowed.

## 4. Acceptance

Automated acceptance must prove:

1. the new terminal toolbar token is exactly 32px;
2. the mounted terminal toolbar has a bounded left status region and a compact
   two-button trailing group while preserving the shared 24px action hitboxes;
3. the mounted terminal canvas is inset 12px horizontally and 8px vertically,
   and PTY sizing still derives from its actual canvas bounds;
4. Workspace header controls remain outside the scrollable tab strip and the
   selected terminal tab remains fully revealable at 960x600;
5. existing terminal input, resize, copy, restart, close, hide and dock movement
   regressions continue to pass.

Required gates:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Native acceptance is main-owned. Inspect bottom and right docks in Light and
Dark at 1403x860 and 960x600, including multiple tabs and terminal restart. The
final installed application must be built from the integrated `master` tree.
