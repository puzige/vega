# Vega R44 Terminal panel interaction model

**Status:** implementation contract

**Scope:** Workspace panel chrome, terminal reveal/focus behavior and the
production tests that freeze those states

**Evidence:** the user-provided Vega screenshot on 2026-09-11 plus a native
walkthrough of the installed application. Screenshots are observable product
evidence only; they are not implementation instructions.

**Supersedes:** the R22 preservation clauses for the Workspace all-tabs menu,
trailing hide action and automatic terminal focus. R22 terminal toolbar and
canvas geometry remain unchanged.

## 1. Problem

The panel currently conflates four different jobs:

- the leading chevron and trailing plus both open the same menu;
- that menu mixes creation, tab switching, preview restoration and a
  destructive close-all action even though the tab strip already owns open
  tabs;
- global reveal actions focus the terminal automatically, so subsequent task
  text can be delivered to `zsh` without an explicit terminal activation;
- the main header can show both the Terminal action and a generic restore
  action for the same hidden terminal.

The native audit reproduced all four conditions. The focus defect is safety
critical: a task-like Chinese sentence was visible as a shell command after a
panel restore. This release makes the interaction model smaller and testable
rather than adding more conditionals to the current menu.

## 2. One action, one meaning

### Workspace header

- The leading chevron is the only pane-level hide action. It hides the current
  pane and returns focus to the task composer. In the bottom dock it points
  down; in the right dock it points right.
- Remove the trailing Minimize/hide button. A pane must not expose two controls
  for the same hide operation.
- The trailing plus opens a creation-only menu for the pane that owns it.
- The creation menu contains only truthful create/open actions: Review when a
  project task makes it available, and New terminal when a selected project can
  own one.
- Existing tabs, preview restoration and Close all terminals do not appear in
  the creation menu. Existing tabs are selected and closed in the tab strip;
  destructive multi-terminal management is outside this compact menu.
- Dock retains its existing meaning and entity identity while preserving the
  task composer focus. Maximize/Restore also preserves entity identity, but it
  explicitly focuses the selected Workspace content after the layout change;
  a maximized pane must never leave focus on an unmounted composer. The
  trailing group therefore contains Plus, Dock and Maximize/Restore, plus the
  existing Review-specific commit action when Review is selected.

### External entry points

- The main-header Terminal button is the single global terminal toggle. If the
  current project's most recent terminal is visible and selected, it hides the
  pane. Otherwise it reveals that same terminal, or creates the first terminal
  if none exists.
- Revealing or creating a terminal through the main-header button or `Command-J`
  keeps the task composer focused. Merely making a shell visible is not consent
  to route subsequent task text into the shell.
- `Environment > Local terminal` is idempotent: it reveals the current
  project's most recent terminal, or creates the first one, but never hides an
  already-visible terminal. It also keeps the task composer focused.
- A hidden pane whose selected tab is a terminal does not receive an additional
  generic main-header restore button. The Terminal button is its sole global
  restore path. Generic restore remains available for hidden Review, file and
  artifact panes.

### Explicit terminal activation

- Clicking or keyboard-activating a terminal tab explicitly focuses that
  terminal.
- Choosing New terminal from the pane's creation menu is also explicit terminal
  activation and focuses the new terminal.
- Clicking the terminal canvas continues to focus it and real PTY input remains
  unchanged.
- Dismissing the creation menu without choosing an item returns focus to the
  composer; it must not implicitly activate the selected terminal.

## 3. Deterministic tab state

- Closing the selected tab chooses its nearest surviving sibling in the same
  pane: the next tab at the same index, otherwise the previous tab. It must not
  jump to the first tab merely because that tab was inserted first.
- Moving, maximizing, restoring, hiding or revealing a pane must preserve the
  selected tab entity and terminal process. Docking preserves composer focus;
  maximizing and restoring activate the selected visible Workspace content.
  No layout action may create a replacement PTY.
- Project isolation, the eight-terminal cap, route fences, close cleanup and
  in-memory-only terminal content remain unchanged.

## 4. Preserved visual contract

- Keep the R22 40px Workspace header, 32px terminal toolbar, 24px icon hitboxes,
  four-pixel action gaps and terminal canvas inset.
- The leading hide control remains outside the horizontally scrollable tab
  strip. The three standard trailing actions remain outside it as a compact
  group.
- No new color, font size, shadow, card, dependency or persistence surface is
  introduced.

## 5. One-state / one-test / one-image acceptance

Every native image below must have a matching production-state assertion. The
delivery record names both the test and the PNG path; screenshots are generated
from the final integrated and installed application, not committed as product
assets.

| State | Automated proof | Native image proof |
|---|---|---|
| Closed | one main-header Terminal entry; no Workspace pane | full window |
| Bottom revealed | composer stays focused; one leading hide and three trailing actions | bottom panel header + composer |
| Creation menu | only Review/New terminal; no tabs, preview restore or close-all | open plus menu |
| Hidden terminal | no duplicate generic restore action | main header |
| Explicit activation | terminal tab/canvas focus accepts a harmless PTY probe | focused terminal canvas |
| Right dock | same terminal entity/process and right-pointing hide control | right panel header |
| Maximized/restored | same selected entity through both transitions; selected content owns focus and accepts input in both states | maximized and restored views |
| Multi-tab close | adjacent sibling becomes selected | tab strip before/after close |

Focused tests must exercise the production handlers and mounted production
render tree. They must replace any earlier assertion that restoring a terminal
implicitly focuses its `TerminalView`.

Required gates:

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

Native acceptance is main-owned at 1403×860 and 960×600. It must include a
real PTY input probe only after explicit terminal activation, plus a composer
typing probe immediately after global reveal to prove the shell did not receive
that text. The final installed application must be built from integrated
`master`.
