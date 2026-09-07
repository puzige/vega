# R11 Command palette and read-only file preview

2026-09-06 user-authorized workflow parity with ZCode: visible sidebar Search and
Cmd+K open a centered search panel with All / Actions / Tasks / Files scopes.
Search input is a real TextInput with IME composition preserved. Scoped Up/Down,
Enter and Escape navigate, activate and dismiss; dismissal restores prior focus
without rebuilding the conversation or clearing its draft.

Actions route to existing production New Task, Open Workspace, Settings, Sidebar,
Terminal, Preview and Review handlers. Unavailable context-dependent actions are
excluded. Task results query persisted registered-project tasks via conversation;
activation uses the normal open-thread operation and updates the route. File search
uses the current project only and a query-aware variant of its bounded, ignore-aware walker (8192 yielded items, cooperative two-second wall budget). Matching happens before the 30-result cap, so matching files after the first 512 entries remain discoverable; the existing @file index is unchanged. Search
results cap at 30 tasks and 30 files; queries cap at 256 characters. Partial index
or inaccessible-project failures are explicit; no whole workspace/file payloads
enter a prompt or network request.

Files open as read-only text in a workspace side pane and retain the central
conversation. Preview reuses the tool path fence, rejects absolute/parent paths,
symlinks, nonregular files, binary/NUL text and content above 128 KiB. Header reveal
uses the same project/path validation before Finder. Existing userspace path-fence
TOCTOU limits apply; pre/post descriptor identity checks reject observed changes.

Database queries, indexing and reads execute on bounded background workers.
Cancellation and generation plus selected-project/thread-route identity fence
results, task activations and previews. UI never queries SQLite. Search and file
errors are content-free. Existing workspace libc is approved for vega_tools read-only preview descriptor O_NOFOLLOW/O_NONBLOCK open and regular-file verification only (R11 architect approval). No new external packages, editor, remote search or fake actions.

Acceptance: owned migrated database and owned project service tests; UI keyboard,
query/filter/activation/dismiss events; production app controller action routing;
fmt, affected clippy/build/test. Main agent owns full-suite and native acceptance.

### Native acceptance refinements
Empty persisted task titles display the sidebar's 未命名任务 fallback without changing the task title. Opening a folder activates its registered project (including an existing registration); cancelling preserves the underlying project, task and Settings route. Cmd+N defers global dispatch until the active window borrow ends and invokes the existing new-task handler. The palette card uses intrinsic height up to its viewport cap.

Read-only Markdown list text shrinks within its row so bullets wrap at narrow workspace widths; code retains its existing scrolling behavior.
