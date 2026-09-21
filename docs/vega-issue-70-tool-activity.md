# Issue #70 — compact tool activity groups

Status: frozen implementation contract, 2026-09-21. This specification
supersedes the visual shape in `vega-ui-spec.md` §4.2 where the two conflict.
The strict tool projections, permission decisions, audit records and timeline
ordering remain unchanged.

## Observed problem

The current timeline paints every tool as a permanently bordered card. Even a
completed call with no useful output consumes a heading row plus one or more
code rows. A run containing several reads and shell commands becomes a stack of
large surfaces that interrupts the assistant narrative.

The Issue reference shows the intended information hierarchy:

- the resting state is a quiet, one-line activity summary;
- adjacent calls form one collapsible activity group;
- expanding the group reveals each call in chronological order;
- a call with useful detail can expand once more into a bounded output surface;
- status remains readable without keeping every output surface open.

The reference screenshots define hierarchy and density, not English product
copy or a license to copy third-party assets. Vega keeps its Chinese interface,
its own icon set, semantic theme tokens and safe typed projections.

## Contract

### 1. Group boundary and ordering

1. Consecutive `Tool` entries with no visible timeline entry between them form
   one tool activity group. Assistant text, user content, an artifact, a Plan,
   a durable Skill provenance row or a task outcome starts a new group. A
   transient permission card may appear after its exact call; removing it must
   allow the next otherwise-adjacent call to join the same group.
2. A group preserves proposal/audit order. A call updates in place while it
   moves through approval, execution and terminal states; updates must not move,
   duplicate or replay it.
3. Live events and typed history hydration produce the same grouping. History
   pagination is message-level, so a durable assistant message and its tool
   audits remain one page unit; no heuristic cross-page reconstruction is
   allowed.
4. Existing R70 text/tool chronology remains authoritative. Grouping changes
   the presentation of adjacent tool entries only and never crosses an
   assistant text segment.

### 2. Resting presentation

1. A one-call group renders the call's compact activity row directly. A group
   with two or more calls renders one aggregate row and is collapsed by default.
2. Compact rows use a 16px Vega icon, 13px text and semantic token colors on the
   conversation background. They have no permanent card fill, outline, top
   border or shadow. A disclosure chevron appears only when the row can reveal
   child calls or safe detail.
3. Long commands and summaries stay on one line and truncate at the available
   width; they never widen the conversation or wrap the resting row. The
   expanded detail retains the complete bounded command/output.
4. A completed shell call reads as `已运行 <command>` and appends a human
   duration when present. Active and failed variants use `正在运行` and
   `运行失败`; a nonzero exit code remains visible. Read/glob/grep use generic
   safe copy (`已读取文件` / `已查找文件` / `已搜索内容`) because their raw input is
   intentionally absent from the UI projection. Write/edit, MCP, Skill,
   rejected, cancelled and corrupt calls retain truthful safe summaries.
5. An aggregate row summarizes the represented categories instead of exposing
   one arbitrary child. Examples are `已运行命令`, `已读取文件` and
   `已读取文件、运行命令`. If any child is active, rejected, failed or cancelled,
   the aggregate wording/status color must remain truthful and cannot claim the
   whole group succeeded.

### 3. Progressive disclosure

1. Activating a multi-call aggregate row toggles its child list. The expanded
   list shows one compact row per call in exact order and reuses the same status
   language/icons as a standalone call.
2. Activating an expandable child toggles only that child's detail. A shell
   detail surface is labelled `Shell`, shows the complete `$ <command>`, then
   the bounded output, and ends with a status footer. Read-only and MCP detail
   surfaces show only the already-bounded result projection. Empty output does
   not create a blank output row, but a shell call may still disclose its full
   command and terminal metadata.
3. The detail surface uses existing `code_bg`, `border_subtle`, radius and
   typography tokens. Success/danger colors are reserved for icons and the
   terminal status; the full summary is not painted as a saturated status
   heading.
4. Expansion is UI-only and defaults closed on route open/restart. It does not
   persist, change audit data or trigger a tool/provider call. Height changes
   invalidate only the owning variable-height list item and preserve scroll
   anchoring.

### 4. Safety and compatibility

1. Do not widen `ToolCardInputProjection` to retain raw read/glob/grep JSON,
   absolute paths, write/edit bodies, fingerprints, provider call IDs or
   checkpoint references. Group summaries are derived only from current typed
   safe projections and terminal metadata.
2. Strict write/edit success/failure validation, invalid-input cards, corrupt
   fail-closed behavior, permission identity and approval controls remain
   unchanged. A compact row must never hide a rejected/failed/corrupt state.
3. UI code continues to consume events/history only and never reads SQLite.
   No migration, dependency, tool execution change or provider schema change is
   in scope.
4. The virtual list still owns one natural-height item per visible activity
   group. A long group is bounded by the existing page/tool limits and must not
   introduce per-frame markdown or JSON parsing.

## Non-goals

- Persisting expansion state.
- Adding copy-to-clipboard, hover-only controls or a new output viewer.
- Showing raw arguments that the current redaction boundary intentionally
  discards.
- Changing permission-card keyboard behavior, tool execution, database schema,
  token accounting, artifacts or Diff behavior.

## Acceptance

The test-first matrix and evidence requirements are in
[`vega-issue-70-tool-activity-delivery.md`](vega-issue-70-tool-activity-delivery.md).
At minimum, mounted production UI tests must prove collapse/expand behavior,
group boundaries, in-place lifecycle updates, hydration parity, safe failure
visibility and variable-height invalidation. Final acceptance also requires the
workspace gates, a packaged native build, and persistent light/dark screenshots
of a real timeline containing both a collapsed mixed group and an expanded
command detail.

