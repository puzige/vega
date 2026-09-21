# Vega R19 Codex UI parity — phase 1 freeze

**Status:** phase 1 implementation contract
**Frozen:** 2026-09-08 (Asia/Shanghai)
**Scope:** the Vega main-window shell only. Vega keeps its name, blue accent, real
controllers, and the Session (no directory) / Project (directory-backed) IA.

This document freezes the measurable phase-1 target before implementation. It
uses Codex Desktop only as a layout, density, and interaction reference. A row
is rendered only when Vega owns the corresponding state or action; no
Subagents, source attachments, commit/push state, branch name, or change count
may be invented.

## 1. Reference freeze

| Reference | Native pixels | Phase-1 evidence used |
|---|---:|---|
| `Snipaste_2026-09-06_07-38-52.png` | 1400 × 900 | conversation header, floating Environment card, composer |
| `Snipaste_2026-09-06_07-41-44.png` | 1691 × 922 (foreground app about 1404 × 854) | persistent Review pane and bottom terminal dock |
| `Snipaste_2026-09-06_07-40-26.png` | 1400 × 900 | Settings baseline recorded for a later phase |
| `vega-r18-master-final.png` | 1200 × 760 | Vega phase-0 comparison |
| `vega-r18-master-terminal-final.png` | 1200 × 760 | Vega phase-0 terminal comparison |
| earlier `1-Photo-1.jpg` | unavailable in the current temp attachment store | prior measured proportions only: 260–270 sidebar, 288–300 Environment |

Phase-1 visual capture uses a **1400 × 900 logical-pixel window** in light
appearance. The 1691 × 922 desktop capture is not compared edge-to-edge because
it includes other applications and desktop chrome; the foreground Codex window
is measured independently.

## 2. Geometry and visual tokens

Ordinary fixed geometry has a tolerance of **±1 logical px**. Text rasterization
and native titlebar traffic-light placement are platform-owned. A measured
range below is implemented at its named target.

| Region/token | Measured reference | Vega target | Tolerance / rule |
|---|---:|---:|---|
| Native window | 1400 × 900 | 1400 × 900 initial | native resize remains enabled |
| Sidebar | 260–270 wide | **260** | ±1; fixed while visible |
| Main content outer gap | 4 | **4** | ±1 |
| Main header | 46–48 high | **46** | ±1; 1px bottom separator |
| Environment rail | 284–300 wide | **292** | ±1 |
| Environment card inset | 14–18 top/right | **16** | ±1 |
| Environment card radius | 18–20 | **18** | ±1; subtle border and small shadow |
| Persistent Review pane | about 674 at 1404-wide foreground window | adaptive 43% with existing 270 minimum | divider 5; user resize persists |
| Bottom terminal | about 273 high | **272** default | existing 150 minimum and resize persist |
| Readable conversation column | about 820 maximum | **768** | centered; issue #100 changed 820 → 768 |
| Composer | about 736 × 99 | **768 max** (equal to the body column), 100 minimum shell | issue #100 changed 736 → 768; content may grow for multiline/error states |
| Composer bottom inset | about 15 | **16** | ±1 |
| Composer radius | about 20 | **20** | ±1 |
| Sidebar row | 32–34 | **32** | ±1 |
| Workspace tab header | 38–40 | **40** | ±1 |

Light colors stay on Vega semantic tokens: base/elevated `#FFFFFFFF`, sidebar
`#F3F3F3FF`, hover `#ECECECFF`, active/brand-soft `#EAF2FCFF`, separator
`#E8E8E8FF`, primary text `#202020FF`, secondary text `#676767FF`, tertiary
text `#8A8A8AFF`, and Vega accent `#3478D8FF`. Dark appearance keeps the
existing paired semantic tokens; phase 1 requires legibility, correct borders,
and no light-only surfaces rather than pixel parity.

Typography uses the existing platform sans/mono stack. The shell uses 13px
body/row text, 12px metadata, and 16px semibold page titles. The empty-state
title remains 28px semibold. Controls use the shared 16px GPUI Kit/Lucide SVG
icons. Unicode glyphs and custom `PathBuilder` icons are outside the contract.

## 3. Region contract

### Sidebar

The native traffic lights remain in the transparent titlebar. The sidebar
toolbar aligns below/alongside that native area and keeps the real sidebar
toggle. New task, search, standalone Sessions, directory-backed Projects, and
Settings retain their existing routes. Rows become 32px with tighter section
gaps; project/thread data and actions remain store-backed.

### Main header

Every non-Settings main route reserves a 46px header. Its left side shows a
folder icon and the loaded project label when a project is selected, followed
by the current task title; an empty route uses the selected project plus
`新建任务`, or `新建任务` alone. Its right side exposes only real actions:
Review when the current task is project-backed, terminal when a project is
selected, and the Environment visibility control. The header has a 1px bottom
separator and truncates labels before action controls.

### Center and composer

Conversation content remains a centered readable column with an 820px cap.
The composer uses a clear two-level hierarchy: a growing input area, then one
action row containing context add, mode, permission, model, thinking, and
send/stop. The existing handlers, keyboard scopes, loading/error states, and
guards stay intact. No token/cost indicator is rendered. The empty route uses
the same real composer control surface after a task is created; it must not
degrade to a decorative input-only card.

> ⚠️ **部分取代（2026-09-15，R69）**：上句的「**after a task is created**」已被 [R69 Home lazy draft composer](vega-r69-home-lazy-draft-composer.md) 取代——首页现在**无需先创建任务**即渲染同一套真实 composer 控件面（任务改为首次提交时惰性落库）。「不得退化为装饰性 input-only 卡片」这条约束仍然成立，并被 R69 强化。

### Environment rail

With a project selected and no persistent right workspace, a 292px rail is
visible on wide windows. Its inset card contains only:

- the loaded project label;
- the live branch selector supplied by the current conversation, when a
  project-backed task is open;
- `Changes / Review`, wired to the existing diff controller for that task;
- `Local terminal`, wired to the existing selected-project terminal action;
- a close action that collapses the rail.

Rows whose authority is missing do not render. A selected project without an
opened task therefore offers Local terminal but no branch or Review row. A
standalone task has no Environment rail. At narrow widths the fixed rail hides
automatically; its header/toolbar entry remains available and opens the same
card as a temporary overlay. The breakpoint is derived from sidebar + 600px
usable center + rail + shell gaps, currently **1180px**.

### Persistent workspace and terminal dock

An opened Diff/file/artifact/right terminal replaces the Environment rail with
the existing persistent right workspace. Its tab, close, move, maximize, hide,
and resize handlers remain active. A bottom terminal is a sibling below the
entire center-plus-right row, so it spans both center and Environment/Review.
Its phase-1 default height is 272px; resize, move, maximize, hide, and close
remain owned by the current workspace model.

## 4. State matrix

| Route / width / workspace | Header | Center | Right | Bottom |
|---|---|---|---|---|
| no project, no task | `新建任务` + sidebar control as needed | onboarding empty state | absent | absent |
| selected project, no task, wide | project / `新建任务`; terminal + Environment actions | project empty state | Environment with Project + Local | existing terminal if opened |
| project task, wide | project / task; Review + terminal + Environment | conversation + full composer | Environment with real branch/Review/Local | existing bottom pane |
| project task, narrow | same real actions, labels truncate | conversation + compact composer | auto-hidden; entry opens overlay | existing bottom pane if height ≥480 |
| project task + right workspace | project / task | conversation narrows | persistent workspace replaces Environment | existing bottom pane spans center + right |
| standalone task | task title; no project-only actions | conversation + full composer | absent | absent |
| Settings | existing Settings route | unchanged in phase 1 | absent | absent |

The user-collapse state and the width-driven auto-hide state are separate: a
resize must not erase the user's choice. Switching projects/tasks keeps the
workspace's existing route fences and never reuses a branch/diff projection
from another route.

## 5. Acceptance and phased remainder

Phase 1 covers the main shell, light appearance, live state bindings, responsive
rail, and existing workspace/terminal behavior. It does **not** claim completion
for these later phases:

1. Settings layout parity against the frozen 1400 × 900 Settings capture.
2. Menu/popover sizing, complete hover/focus/pressed-state visual parity, and
   keyboard focus-ring polish.
3. Dark-mode color-by-color visual parity and contrast audit.
4. Remaining conversation-row, Markdown, diff syntax, terminal typography,
   native window-menu, and animation details.

Automated acceptance covers fixed layout tokens, responsive Environment mode,
route truthfulness, and production action wiring where GPUI visual tests can
observe it. Final acceptance still requires a real macOS launch to click
Environment collapse/reopen, open Review, open/move/resize/maximize/close a
terminal, resize through the breakpoint, and compare light/dark captures.
