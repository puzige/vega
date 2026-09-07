# R11 integrated terminal

Approved by main architect 2026-09-06 before implementation. User requests a real integrated terminal based on the native ZCode audit: workspace `+` → terminal, login zsh in the current project, multiple sessions, right/bottom dock movement, visibility toggle and close.

## Contract
- Production service in `vega_conversation` owns a persistent native PTY and `/bin/zsh -l` on macOS (Unix fallback `/bin/sh -l`), cwd canonicalized from the trusted selected project path. No invocation for merely opening a thread. Only explicit human terminal creation/input starts or controls a shell; this does not grant any agent-tool execution authority.
- Shared snapshots/status/size/cell types live in `vega_conversation::types`; no GPUI dependency in the service/runtime. Output is parsed incrementally by vt100, bounded to 2,000 scrollback rows and at most 240×400 visible cells. Input is bounded to 64 KiB per action, queued with backpressure/error; ANSI/UTF-8/alternate-screen/cursor state is real parser output, never fake command transcripts. Terminal content is in memory only and never logged or stored in SQLite.
- Worker owns PTY writes/resizes/process lifecycle, with separate bounded read processing. UI polls snapshots at 30ms while alive; no read/write/spawn/wait on render. Interactive text and Enter/arrows/backspace/tab/Ctrl+C/Ctrl+D are sent to the PTY. Resize uses actual viewport geometry. Scroll wheel navigates bounded parser scrollback; typing returns to live screen. Status is Starting/Running/Exited/Failed and explicit restart creates a new shell in the original trusted root.
- Sessions are keyed by selected project identity. Task changes within a project retain shell and tab state. Project switches cannot show another project's output; hidden project sessions remain owned until explicit close/window teardown. Moving/hiding panels does not close sessions. Explicit close and window drop signal/kill and reap the child, including foreground process group on Unix. No background orphan claim without real tests.
- Extend existing workspace tabs with Terminal; preserve Review/artifacts/drafts and reuse right/bottom movement, hide/restore, maximize, resizing. Add `+` menu terminal action and top terminal visibility action. File preview integration is delegated separately; terminal executor owns the narrow workspace hooks.
- Native main acceptance must prove actual shell interaction/docking/multiple sessions. Owned temporary production service tests prove cwd/persistent state/interrupt/resize/exit/restart/reaping; UI handler tests prove dispatch. No real project command execution, credentials, network or performance tests by implementation agent.

## Dependency approval and evidence
Main approved exact `portable-pty = 0.9.0` and `vt100 = 0.16.2` before adding them. portable-pty is WezTerm's native PTY master/slave/child abstraction; vt100 provides a maintained screen model and bounded scrollback instead of a homegrown escape parser. No direct vte dependency required.
- https://docs.rs/portable-pty/0.9.0/portable_pty/ (native PTY, cloned reader, writer, child wait/kill, resize).
- https://docs.rs/vt100/0.16.2/vt100/ (incremental parser, terminal cells, cursor, screen, resize/scrollback).

## Limits
Terminal emulation targets ordinary interactive shells and ANSI TUIs; advanced image protocols, OSC clipboard/hyperlink actions, mouse-reporting applications and search/copy-selection UI are deferred. OSC content never opens links or writes clipboard automatically. Login shell startup executes user shell configuration when the human explicitly opens a terminal, matching normal native terminal semantics. Session state is not restored after app exit.

## Review refinements
- Registered project resolution (database path + selected ID) occurs in the conversation worker; workspace handlers do no project SQL. Checks before/after resolution and immediately before spawn reject a closed Starting session.
- Close/Drop only signal cancellation. The worker performs kill/wait and app quit registers an asynchronous all-session cleanup observer. Ordinary owned cleanup is verified; OS-stalled spawn/IO/wait can outlast GPUI's shutdown timeout, so universal bounded shutdown is not claimed.
- Cmd+C and the labeled toolbar action copy the current visible screen, not a selection; selection UI remains deferred. IME composition prevents special-key dispatch until committed.
- A visible workspace menu action closes all terminal sessions, including other projects, to release sessions of removed/unreachable projects and the global eight-session limit.

## Native acceptance correction: selected workspace tab visibility
At 960×600, moving a terminal into a right dock containing README left the selected terminal label/close control horizontally clipped. Each dock now owns a horizontal scroll handle; opening/selecting/moving, replacing selection after close, and resizing reveal the selected tab without reordering. Individual tabs fit within the scroll viewport and retain their fixed close control; header tools remain outside the scrolling region. The all-tabs menu lists actual open labels/docks for direct access. Acceptance checks the real production move handler and rendered child bounds at the narrow window size.
