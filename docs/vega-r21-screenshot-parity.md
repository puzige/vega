# Vega R21 current Codex screenshot parity — phase 2 freeze

**Status:** implementation contract
**Frozen:** 2026-09-09 (Asia/Shanghai)
**Scope:** current main-window shell, Settings shell, Composer chrome, and the
shared geometry of transient menus. Vega keeps its own brand, information
architecture, controllers, and native GPUI implementation.

This phase is based only on pixels visible in user-supplied Retina screenshots.
The source captures are not checked into the repository because they contain
personal task names, account information, and local paths. No CSS, JavaScript,
private token name, icon, or packaged asset from the reference application is
an implementation input.

## 1. Screenshot evidence and inference

All full-window captures are 2806 × 1720 device pixels, or 1403 × 860 logical
pixels at Retina 2×. Ordinary geometry below has a ±1 logical-pixel tolerance.

| Observable | Device-pixel evidence | Vega target |
|---|---:|---:|
| Main header | 92 high | 46 high |
| Left Sidebar, current narrow captures | 608 and 617 wide | 304 default |
| Left Sidebar, earlier capture | about 730 wide | user-resizable, up to 365 |
| Conversation readable column | about 1640 wide | 768 maximum (issue #100) |
| Composer outer width | about 1476 wide | 768 maximum, equal to the body column (issue #100) |
| Composer radius | about 40 | 20 |
| Composer bottom inset | about 32 | 16 |
| Send control | 56 × 56 | 28 × 28 |
| Environment card | about 608 wide | 304 wide |
| Environment right inset | about 32 | 16 |
| Settings content card | about 1488 wide | 744 wide |
| Settings switch | about 64 × 40 | 32 × 20 |
| Large account-style popover | about 700 wide | 350 wide, 18 radius |

The two new captures show the same left Sidebar with the Environment surface
closed and open. Therefore the shell is not a fixed three-column grid:

- the left Sidebar owns a user-adjustable width;
- the Environment control independently opens or closes a right utility rail;
- opening the rail reduces the center width without changing the left width;
- the readable conversation and Composer remain centered inside the remaining
  center region, subject to their maximum widths.

The earlier 365px Sidebar measurement is an observed resized state, not a safe
default. R19's fixed 260px interpretation is superseded for this phase.

## 2. Authority and truthfulness

1. Vega copies no reference source code, internal token names, brand assets, or
   inaccessible application internals. Screenshot measurements describe only
   public, visible output.
2. Vega retains its approved sapphire / ice-blue brand tokens. In particular,
   the reference application's visible `#3983F7` action blue is not copied;
   Vega continues to use `brand_primary` and `brand_primary_strong`.
3. Rows and controls render only when backed by current Vega state and a real
   handler. The Settings navigation must not add fake Account, Voice, Pets,
   Plugins, Import, Browser, or other reference-only features.
4. Existing safety gates, route identity checks, controller ownership, focus
   paths, keyboard actions, and persistence behavior remain authoritative.

## 3. Shared visual tokens

### 3.1 Light appearance

The visible neutral shell is updated to the current screenshots:

| Semantic token | Target | Use |
|---|---:|---|
| `bg_base` / `bg_elevated` | `#FFFFFF` | center, cards, Composer, menus |
| `bg_sidebar` | `#FAF9F9` | main and Settings navigation rails |
| `bg_hover` | `#ECECEC` | hover and neutral selected surfaces |
| `border_subtle` | `#E8E8E8` | one-pixel separators and outlines |
| `text_primary` | `#191C1F` | primary labels and body copy |
| `text_secondary` | existing semantic gray | metadata and secondary labels |
| `brand_primary` | `#3478D8` | Vega primary action and focus |
| `brand_primary_strong` | `#245AAF` | Vega primary hover emphasis |

Dark appearance keeps its existing semantic pairing in this phase. No
component may branch on appearance or introduce a hexadecimal literal.

### 3.2 Geometry

| Token | Target | Contract |
|---|---:|---|
| Sidebar default | 304 | initial and legacy-config width |
| Sidebar minimum / maximum | 240 / 365 | pointer resize clamps in this range |
| Sidebar resize hit area | 5 | one-pixel visible separator centered in it |
| Main header | 46 | unchanged |
| Main content outer gap | 0 | current flat split shell; no rounded page frame |
| Conversation max | 768 | issue #100: 820 → 768, shared with the Composer |
| Composer max / radius / bottom inset | 768 / 20 / 16 | issue #100: 736 → 768, now equal to the body column |
| Composer send | 28 × 28 | circular; icon remains shared SVG |
| Environment outer rail | 320 | includes a 304px card and 16px right inset |
| Environment card radius | 18 | unchanged |
| Settings content max | 744 | main settings column/card width |
| Settings navigation | current Sidebar width | same persisted resize value |
| Large menu/popover max | 350 | content may choose a smaller semantic width |
| Large menu/popover radius | 18 | border plus restrained small shadow |

The stored Sidebar width is a finite number clamped to the range on both read
and write. A legacy configuration without the value resolves to 304. Dragging
changes width only; it must never toggle the Sidebar or Environment.

## 4. Main shell

### 4.1 Sidebar

- Render at the stored width, default 304px, with 12px padding and existing
  32px rows.
- Use the light `bg_sidebar` surface directly against the white main content,
  separated by one subtle line. Remove the rounded, bordered main-page frame
  and its four-pixel moat.
- Preserve the current real actions and data: navigation, new task, search,
  Sessions, Projects, task management, and Settings. Do not reproduce the
  reference application's unrelated global navigation.
- A five-pixel drag target at the trailing edge uses a column-resize cursor.
  Pointer movement updates the width continuously; pointer-up ends the drag.
  The accepted width is persisted through the existing config update path.
- Cmd+B and width-driven auto-collapse remain distinct from resize. Resize does
  not overwrite either collapse choice.

### 4.2 Header and center

- The 46px header remains flush with the main white surface and keeps a single
  bottom separator. Existing project/task truncation and real actions remain.
- Conversation rows and Composer center within the currently available center
  region. Opening either the Environment rail or a real workspace pane narrows
  that region; it does not move or resize the Sidebar.
- The Composer retains its real context, mode, permission, model, thinking,
  send/stop, errors, keyboard guards, and multiline behavior. Its send/stop
  hitbox is exactly 28px square and circular. The surface uses one border and
  one restrained shadow rather than nested elevation.

### 4.3 Environment

- The header Environment button controls the same user choice as R19; it is not
  driven by Sidebar resize.
- A wide project route may show a 320px rail whose white card is 304px wide and
  16px from the right edge. The card begins 16px below the header/content row,
  uses an 18px radius, and preserves the existing truthfulness rules.
- Opening the rail consumes center width. A persistent Review/file/terminal
  workspace continues to replace it. At narrow windows the same real rows open
  as an overlay, without erasing the user's wide-layout choice.
- Recompute the responsive breakpoint from the effective visible Sidebar,
  600px minimum center, 320px Environment rail, resize hit area, and separator.
  With the default 304px Sidebar the target is 1230px and tests cover 1229 and
  1230 exactly. The breakpoint moves one-for-one with a resized Sidebar (1291px
  at the 365px maximum); a collapsed Sidebar contributes zero width.

## 5. Settings shell

Settings becomes a full-height two-column route instead of a 64px back strip
containing another framed mini-sidebar.

### Navigation rail

- Width equals the current Sidebar width and uses `bg_sidebar`.
- It starts below the native traffic lights with the existing Back action.
- It lists only the five real destinations: General, Providers, Reasoning,
  Pricing, and Usage. The existing numeric section state may remain internal,
  but the visible order begins with General.
- The selected row is a 32px neutral surface; Vega brand color remains for
  focus and primary actions. Every row keeps its keyboard activation path.
- A search field is not rendered until Settings search has real behavior.

### Content region

- The selected page title is shown at 24px semibold. The content column is
  744px maximum, horizontally centered in the remaining white region, with at
  least 24px horizontal padding and responsive vertical padding.
- Existing Provider, defaults, reasoning, pricing, usage, error, loading,
  saving, retry, and focus behavior is preserved.
- Related rows may be grouped into 12px-radius, one-pixel bordered white cards;
  separators run between rows. Do not invent switch states merely to resemble
  the screenshot. Current button/select controls remain controls unless a real
  boolean setting exists.
- Settings opens on General. Requests that explicitly target Pricing still
  open Pricing. Escape and Back return through the existing action.

## 6. Menus and popovers

Existing model, mode, permission, branch, task-action, and command surfaces use
one visual grammar: white/elevated background, subtle border, 18px radius for
large floating cards, a restrained small shadow, 32px rows, 13px primary text,
and 12px supporting metadata. A menu may remain narrower than 350px when its
content is compact; 350px is the cap and reference size for multi-section
cards, not a requirement to stretch every menu.

No Account or profile popover is added in this phase because Vega has no
corresponding account controller.

## 7. State matrix

| State | Left | Center | Right |
|---|---|---|---|
| default project task at 1403px | stored/default 304 | centered thread + Composer | 320 Environment when user choice is open |
| Environment closed | unchanged | expands; max-width content stays centered | absent |
| Sidebar resized | 240–365 | reflows continuously | unchanged choice |
| width 1230+ at default Sidebar | current Sidebar choice | normal | Environment may be persistent |
| width 1229- at default Sidebar | current Sidebar/auto-collapse rules | normal | Environment uses temporary overlay |
| persistent workspace open | unchanged | narrows | workspace replaces Environment |
| standalone task | unchanged | thread + Composer | no project-only surface |
| Settings | same stored rail width | 744px max selected settings page | absent |

## 8. Implementation task and acceptance

This document is the R21 task card. Allowed production scope is
`vega_theme`, `vega_store` configuration, `vega_ui` Sidebar/Settings/Composer
and existing menus, and the `vega` window shell/workspace. No dependency,
database migration, runtime/provider, tool, or credential change is allowed.

Automated acceptance must cover:

1. exact shared token values and finite Sidebar width clamping/defaults;
2. config round-trip plus legacy default for Sidebar width;
3. mounted production shell at default, minimum, and maximum Sidebar widths;
4. drag resize changes only width and persists on completion;
5. Environment open/closed geometry, exact 1229/1230 default boundary, and
   one-for-one breakpoint movement at the Sidebar bounds;
6. Settings opens on General, explicit Pricing routing still works, navigation
   order/width/content cap are observable, and all existing controller tests
   continue to pass;
7. Composer geometry and real send/stop guards remain intact;
8. `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
   and `cargo test --workspace` pass.

Final visual acceptance requires a real packaged macOS launch at 1403 × 860 in
light mode, captures with Environment closed/open, Sidebar at 304 and 365,
Settings General and Providers, one real existing popover, and a dark-mode
legibility check. Screenshot comparison may ignore platform-owned font
antialiasing and traffic-light placement; geometry follows the ±1px contract.
