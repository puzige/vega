# A8-01 — New task selects its project in Composer

Status: user-authorized, 2026-09-17. This spec supersedes R49 §2.4/§2.5 and R69 R13/R15 only where they require a project binding before showing the utility bar or the separate project-less home guidance. The existing Composer geometry, chip states, project registration service, and lazy draft guarantees remain in force.

## Observed defect

On the installed Vega build, clicking **新建任务** with no `SelectedProject` shows the text Composer but no project chip. Expanding a sidebar project does not select it. The user therefore has to open an old task first to obtain project context. Even on a project-bound draft, choosing a different project from the Composer menu makes the utility bar disappear: `SelectedProject` changes, but the cached `ConversationStream` still holds the previous `thread.project_id`. A separate project-less home prompt says `先添加一个项目` and offers `添加项目文件夹以开始…`; it competes with the real Composer and is not the desired default entry.

## Product contract

1. **Composer is the default new-task surface.** New task, Cmd+N, and the initial home route render the real, immediately usable Composer without a preliminary folder choice. Keep lazy materialization: no task row until first submit.
2. **Project choice is always available on an empty draft.** The 37px Composer utility bar mounts above the card even with no selected project. Its project chip reads `选择项目` with the existing Folder icon when unbound; when bound, it shows the actual project name. The existing upward menu lists registered projects and permits choosing one. No fabricated default project, implicit first-row selection, or automatic filesystem picker.
3. **Draft rebind is real, not visual-only.** Selecting a project, switching projects, or choosing `不关联项目` updates the same draft ID, its project binding, the cached Composer/controller context, label, and Git branch affordance together. Preserve already typed Composer text, mode/model/permission choices, and focus. Close stale project/branch menus and cancel stale project-scoped operations. A branch request must never use the prior project. No database write before submit; first submit creates exactly one task under the final chosen project (or standalone if detached).
4. **Committed tasks are not rebound by the chip.** Project selection from an existing task must not silently change its durable `project_id`. The new project-selection behavior is for the unmaterialized new-task draft; session pages with messages continue to omit the utility bar. Existing project-bound empty-session behavior remains safe.
5. **One entry, no duplicate prompt.** Remove the project-less home's separate `先添加一个项目` / `添加项目文件夹以开始…` guidance and its dedicated click handler. The center keeps the ordinary `今天想做些什么？` empty state and the Composer; the optional sidebar-reveal affordance may remain when the sidebar is hidden. Keep the Sidebar `+`/explicit Add Project path for registering a *new* local folder, including its native folder picker; do not summon that picker merely by clicking New Task. Do not delete user projects or files.
6. **Do not redesign.** Keep the existing card, utility bar, menu width/placement, 28px capsule states, branch selector semantics, colors, type scale, and send row. An unbound draft has only the project chip; the branch chip appears only when a real Git project context exists. Project popup and branch popup remain mutually exclusive.

## Acceptance

- Production GPUI/root test: no selected project + registered projects → visible Composer and project chip, no separate project prompt; clicking the chip shows the registered rows.
- Production GPUI/root test: type a draft, choose a project → same draft ID and text/focus, visible project/branch context, zero database task rows; submit → one row with that project binding.
- Production GPUI/root test: project A → B → detached on the same draft → no stale branch context or duplicate popup; submit → one standalone row. Cover project B as final binding as well.
- Regression: clicking New Task never opens the OS folder picker; existing task binding/history is not mutated; empty-session utility visibility and all existing R49/R68/R69 tests remain coherent with this explicit supersession.
- `./scripts/cargo-lock.sh test --workspace`, `cargo fmt --all -- --check`, `./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings`, package, and native click-path verification on the installed app. Report any accepted residual honestly.

No new dependencies or non-test `unwrap`/`expect`; use existing store and controller boundaries. Update older tests whose only assertion is the superseded absence of the no-project bar, but do not weaken unrelated tests.
