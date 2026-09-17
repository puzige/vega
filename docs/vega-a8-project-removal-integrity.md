# A8-02 — Remove a registered project without losing tasks

Status: user-authorized bug fix, 2026-09-18. Supersedes the A1-03/S2 removal implementation only where it assumes that deleting a `projects` row is sufficient after tasks have been created. The existing `移除项目（保留文件）` action, no-confirmation ruling, and standalone-task model remain in force.

## Defect and cause

The installed app reports `项目移除失败：projects store error: FOREIGN KEY constraint failed` for a project that owns tasks. `projects::remove` issues only `DELETE FROM projects`; migrated `threads.project_id` is nullable but still has a foreign key to `projects(id)` with no `ON DELETE` action. Thus a project with any task blocks the deletion. The current store test covers only an empty project and cannot catch this failure.

## Contract

1. Removing a project unregisters that project from Vega and removes its sidebar project-order entry. It never deletes or modifies the local project folder, Git checkout, or other workspace files. Do not automatically remove a different registered project.
2. Existing tasks under the removed project become standalone tasks by setting their `threads.project_id` to SQL `NULL`, not the compatibility empty string. Preserve each task ID, title, status, pin, timestamps, messages, tool calls, token usage, and sidebar group membership. Archived tasks remain archived. They must remain discoverable through the standalone/Recents projection and reopen without the removed project as a workspace authority. No automatic reattachment if the same folder is registered again under a new ID.
3. Detach tasks and remove the project inside one SQLite transaction with foreign keys enabled. Any failure rolls back both operations. Removing an unknown ID returns `false` without changing any task. Preserve the existing `projects::remove(&Connection, id) -> Result<bool, ProjectsError>` API and UI error surfacing. Avoid a schema migration when a transactional update suffices.
4. On successful removal, clear the selected-project/active-project route if it names the removed ID, invalidate project-scoped view state, and reload the sidebar. Do not leave a stale branch/file authority attached to a retained standalone task. Existing navigation guards continue to prevent losing unsent user input. Do not delete an open task merely because its project is removed.
5. Existing project-scoped permission rows are not transferred to the standalone tasks or a re-registered project. Their opaque old project ID cannot authorize work after removal; this bug fix does not broaden into a permission-history cleanup or alter tool approval rules.

## Acceptance

- Owned temporary migrated database through the production store API: remove a project with active, archived, and pinned tasks plus messages/tool calls/token usage and an unrelated project; assert the target registration is gone, all target tasks have SQL `NULL` binding, related records and relevant metadata are unchanged, the unrelated project/tasks remain bound, and `PRAGMA foreign_key_check` is clean. Verify re-registration of the same path does not rebind history.
- Empty-project removal and unknown-ID idempotence still pass. Inject a transactional failure (or use an equivalent owned fixture) and prove rollback leaves both the project and bindings intact; do not rely on a test that merely calls the SQL primitive with no task.
- Mounted sidebar/root production test: invoke the existing Remove Project action for a project with tasks, confirm no error bar, the folder vanishes, and the retained tasks appear once under standalone/Recents with no project route. The active-project case must clear stale selection; unrelated project remains available.
- `cargo fmt --all -- --check`, `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`, `./scripts/cargo-lock.sh test --workspace`, package/signature and installed-app/native check. Native validation may use a disposable registered project; never delete one of the user's existing project registrations as a test.

No new dependency, no non-test `unwrap`/`expect`, no direct UI SQLite writes, and no user-file deletion.
