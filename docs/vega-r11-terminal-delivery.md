# R11 real terminal delivery

## Freeze
- verified_at_utc: 2026-09-06T02:53:00Z; local: 2026-09-06 10:53 +08:00.
- Source: `codex/r11-terminal`, based on the accepted R10 integration and the companion palette service/UI dependency. Apply the terminal implementation commit once after the palette dependency; this report does not duplicate the companion implementation.
- implementation_diff_sha256 against companion dependency: `dcbf4318e9e8867359a408938dd5322b80cd33b2e0bf76d4201884c57dabb75c`.
- task_contract: `docs/vega-r11-terminal.md`, written before code and updated for architect review.
- macOS arm64; Rust/Cargo 1.98.0, Git 2.55.0.

## Implementation

A dedicated conversation-layer worker owns a native PTY and persistent login shell. The app supplies its registered project ID and database path; project lookup and canonical cwd resolution run off the UI thread. Human text/control keys flow through the real PTY writer, and vt100 supplies the bounded screen/cursor/color/alternate-screen state. Resizing updates both the kernel terminal and parser. Shell editing/history/Ctrl+C are genuine terminal behavior; no per-command subprocess or fabricated transcript exists.

The UI retains terminal entities across hide, dock movement and task changes. Session IDs are isolated by selected project, with up to eight sessions and a visible close-all action covering hidden/unavailable projects. Review and Artifact tabs retain their existing behavior. A narrow File tab hook accepts the companion palette's validated read-only entity. The terminal provides restart, status, bounded scrollback, UTF-8/IME commit input, and explicitly labeled copy-current-screen behavior (Cmd+C).

Close/Drop signal cancellation without joining a worker on the UI thread. The worker kills foreground/shell process groups and reaps the owned shell. Stop is checked before/after project/root resolution and immediately before spawn. An app-quit future waits for all registered workers' cleanup. The pinned GPUI framework limits quit observers to 200ms; ordinary cleanup is verified, but OS-stalled spawn/IO/wait is an explicit residual rather than a universal bounded-shutdown claim.

Approved dependencies: portable-pty 0.9.0 and vt100 0.16.2. Primary API/source references and rationale are in the task contract. `libc` remains the already approved Unix lifecycle primitive. Terminal output and snapshots stay in memory; they are neither logged nor written to SQLite. This human terminal creates no agent-tool permission bypass.

## Results

| requirement | evidence class | exact command | result / duration |
|---|---|---|---|
| Formatting | STATIC | `cargo fmt --all -- --check` | PASS |
| Strict affected lint | STATIC | `cargo clippy -p vega_conversation -p vega_ui -p vega --all-targets -- -D warnings` | PASS, 3.08s |
| Build | BUILD | `cargo build -p vega` | PASS, 16.38s |
| Real login PTY | E2E-REAL | `cargo test -p vega_conversation terminal::tests` | 2 passed |
| Actual UI input handlers | E2E-REAL | `cargo test -p vega_ui terminal::tests` | 1 passed, 0.34s |
| Dock/focus/project routes + existing Review | E2E-REAL | `cargo test -p vega --bin vega window::workspace` | 2 passed, 0.44s |
| UI regression target | E2E-REAL + UNIT | `cargo test -p vega_ui --lib` | 142 passed, 0.57s |
| Conversation regression target | E2E-REAL + UNIT | `cargo test -p vega_conversation --lib` | 269 passed, 53.61s |
| App regression target | E2E-REAL + UNIT | `cargo test -p vega --bin vega -- --test-threads=1` | 64 passed, 20.91s |

Real service evidence covers an owned cwd marker, persistent environment/cd, `stty size` 31×97, interrupting `sleep`, ANSI colors, exit 7, restart and shell PID absence after close/reaping. Real UI evidence uses the production platform input handler and dispatched Backspace/Enter/Ctrl+C keys to write an owned marker through the shell, then observes exit/restart. Workspace production handlers prove multiple sessions, hide/restore, right→bottom movement, retained entity identity, terminal focus, project isolation and explicit close. Tests run the actual code in subprocesses with owned HOME/ZDOTDIR so they never load the user's shell startup files.

## Retained failure history

- Initial UI check caught a local `size` binding shadowing the GPUI function; corrected.
- Initial GPUI tests imported the attribute macro via a wildcard, recursively expanding `test`; scoped imports fixed it without increasing recursion limits.
- The first UI cleanup test awaited an external-thread oneshot on GPUI's deterministic test scheduler. The test now observes the actual worker terminal state with a bounded real-clock loop; production asynchronous quit cleanup is unchanged.
- A compile gate caught the existing store-path helper's module-private visibility; it is now `pub(super)` for the workspace controller.
- The first full app run (`vega-r11-terminal-app-all.log`) had 63 passed and the pre-existing `diff_refresh_intents_keep_content_during_background_and_retry` GitFailed failure. The exact retry passed (`vega-r11-terminal-app-diff-retry.log`, 0.75s), and the complete serialized target passed 64/64. No assertion or timeout was weakened; the first parallel failure remains reported.

## Evidence logs

Raw logs are retained under `/private/tmp`; no real secret was used:
- `vega-r11-terminal-service-tests3.log`: SHA256 `4d1fcface2c4074814e789b3d1451700d81684e1ac6dbab39e87f73ef30b32a3`.
- `vega-r11-terminal-workspace-tests3.log`: SHA256 `fb54f32b2c2a0e501ffbe2cb427ec36a329ca53cb8214d6e56680d5759dadff6`.
- `vega-r11-terminal-ui-all.log`: SHA256 `453eadbee05f84bf7ceba7405d0bf1dedeffb0409b243ccf365c4d785d7984ca`.
- `vega-r11-terminal-clippy-pass.log`: SHA256 `ee6efc111c56593ba74bee31b7fff9c198fa4433195a82bc1f3798d1554c407c`.
- `vega-r11-terminal-build.log`: SHA256 `81293ab970b22b38e503342e861532612f7e60ee9c9602beef27b3fb2016ffbe`.

## Residuals

- NOT RUN by executor: native application CUA, real user project commands/credentials/network, integrated whole-workspace suite, performance benchmarks. Main owns native acceptance after complete R11 integration.
- LIMIT: advanced terminal image protocols, OSC clipboard/link actions, mouse-reporting TUI support, selection/search UI and persistent session restoration are deferred. Cmd+C copies the visible screen, not a selection. No claim of full xterm feature parity.
- LIMIT: process groups are killed/reaped on normal close. Deliberately detached descendants and OS-stalled operations exceed this guarantee; the 200ms framework quit timeout remains a residual. No unbounded join runs on a UI handler or render path.
- LIMIT: removed-project sessions remain isolated and can be closed through the all-session menu; they do not silently disappear when merely switching projects.
- Existing upstream `block` future-compatibility notice remains; strict current lint/build pass.

## Native follow-up: reveal selected workspace tabs

Main's 960×600 native acceptance found that moving a terminal into the right dock containing README left its selected label/close control clipped. The fix adds independent horizontal scroll handles, reveal requests for selection/move/resize, and viewport-bounded tabs with a shrinking label and fixed close button. Header tool buttons stay outside the scrolling strip. The all-tabs menu lists real open tab labels and their docks.

The existing production workspace test now creates README and a real terminal at 960×600, moves the terminal bottom→right, retains original tab order, and checks actual rendered child bounds plus scroll offset: the entire selected tab (including its close control) is within the viewport. `cargo test -p vega --bin vega window::workspace` passes 2/2 in 0.44s; log `vega-r11-tabs-workspace-tests.log` SHA256 `220ed0c72a18c129bc5c98535813f48a018b60643c57c40e79d4296cbfedb3ba`. Final fmt and `cargo clippy -p vega --all-targets -- -D warnings` pass.

A broad workspace lint on this branch encountered the older companion palette dependency's `vega_tools` items-after-test-module issue; that source is owned and updated by the palette branch, so this fix does not modify it or suppress lint. Main's integrated workspace gate must use the latest palette fixes. Native recheck remains main-owned.
