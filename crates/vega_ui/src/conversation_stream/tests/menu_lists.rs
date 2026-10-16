//! R62 §8: the two utility-bar dropdowns carry real list structure.
//!
//! Contract source: `docs/vega-r62-slider-card-three-rows.md` §8 (R10/R11) and
//! §9 (A12/A13). Every case mounts the real [`ConversationStream`] in a window
//! and reads the rendered frame — no test-only seam decides any branch.
//!
//! The defect these tests pin: R49 shipped both dropdowns as bare text lists,
//! so they had no search field, no per-row icon, no checkmark, no separator and
//! no trailing actions. R62 R11 also fixes the rule the structure must respect:
//! filtering may hide rows but must never change what "select" means.

use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
use vega_conversation::types::BranchSnapshot;

/// The `permission_thread()` fixture's durable project binding.
const PROJECT_BINDING: &str = "project-safe-id";

/// Owned store with the same project rows the sidebar renders, plus the
/// selection global the composer's project context resolves through.
///
/// The first row is inserted with the fixture thread's own binding id, so the
/// "current project" the dropdown marks is a row the store really lists — the
/// same shape `vega`'s branch tests build with raw SQL.
fn install_utility_globals(
    cx: &mut TestAppContext,
    projects: &[(&str, &str)],
    selected: Option<&str>,
) -> Vec<String> {
    let store = vega_store::Store::open(":memory:").expect("owned utility store");
    store.migrate().expect("owned utility migrations");
    store
        .conn()
        .execute(
            "INSERT INTO projects(id,path,name,created_at,last_opened_at) \
             VALUES(?1,'/tmp/r62-bound','bound',0,0)",
            [PROJECT_BINDING],
        )
        .expect("owned bound project");
    let ids = std::iter::once(PROJECT_BINDING.to_string())
        .chain(projects.iter().map(|(path, name)| {
            vega_store::projects::create(store.conn(), path, name, None)
                .expect("owned utility project")
                .id
        }))
        .collect();
    cx.update(|cx| {
        cx.set_global(crate::sidebar::VegaStore(Ok(store)));
        cx.set_global(crate::sidebar::SelectedProject(selected.map(str::to_owned)));
        cx.refresh_windows();
    });
    cx.run_until_parked();
    ids
}

fn mounted(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> bool {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .is_some()
}

fn bounds(
    window: WindowHandle<StreamHarness>,
    selector: &'static str,
    cx: &mut TestAppContext,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    cx.run_until_parked();
    VisualTestContext::from_window(window.into(), cx)
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"))
}

fn click(window: WindowHandle<StreamHarness>, selector: &'static str, cx: &mut TestAppContext) {
    let b = bounds(window, selector, cx);
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.simulate_click(b.center(), Modifiers::default());
    visual.run_until_parked();
}

pub(super) fn branch_snapshot(labels: &[&str]) -> BranchSnapshot {
    let root = tempfile::tempdir().expect("owned branch fixture repo");
    let canonical = root.path().canonicalize().unwrap();
    let metadata = canonical.join("metadata");
    std::fs::create_dir(&metadata).unwrap();
    let oid = "1111111111111111111111111111111111111111";
    let current = labels.first().copied().unwrap_or("main");
    let status = format!("# branch.oid {oid}\0# branch.head {current}\0");
    let mut branches = labels.to_vec();
    if branches.is_empty() {
        branches.push("main");
    }
    branches.sort_unstable();
    let refs = branches
        .iter()
        .map(|label| format!("{oid}\0refs/heads/{label}\0\n"))
        .collect::<String>();
    let expected_root = canonical.clone();
    let _guard = vega_conversation::register_git_test_executor(
        &canonical,
        std::sync::Arc::new(move |command, input, limit| {
            assert_eq!(command.get_current_dir(), Some(expected_root.as_path()));
            let args: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_str().expect("UTF-8 fixture argument"))
                .collect();
            let prefix = [
                "--no-pager",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "color.ui=false",
                "-c",
                "maintenance.auto=false",
                "-c",
                "maintenance.autoDetach=false",
                "-c",
                "gc.auto=0",
                "--no-optional-locks",
            ];
            assert!(
                args.starts_with(&prefix),
                "unexpected branch command: {args:?}"
            );
            let tail = &args[prefix.len()..];
            assert_eq!(
                input,
                (tail.first() == Some(&"check-attr")).then_some(&b""[..])
            );
            let response = match tail {
                ["rev-parse", "--show-toplevel"] => format!("{}\n", expected_root.display()),
                ["rev-parse", "--absolute-git-dir" | "--git-common-dir"] => {
                    format!("{}\n", metadata.display())
                }
                ["rev-parse", "--git-path", marker]
                    if [
                        "MERGE_HEAD",
                        "CHERRY_PICK_HEAD",
                        "REVERT_HEAD",
                        "BISECT_START",
                        "BISECT_LOG",
                        "rebase-merge",
                        "rebase-apply",
                        "sequencer",
                    ]
                    .contains(marker) =>
                {
                    format!("{}\n", metadata.join(marker).display())
                }
                [
                    "for-each-ref",
                    "--sort=refname",
                    "--format=%(objectname)%00%(refname)%00",
                    "refs/heads/",
                ] => refs.clone(),
                [
                    "status",
                    "--porcelain=v2",
                    "-z",
                    "--branch",
                    "--renames",
                    "--untracked-files=all",
                ] => status.clone(),
                ["ls-files", "-z", "--cached", "--deduplicate"]
                | ["check-attr", "-z", "--stdin", "--all"] => String::new(),
                _ => panic!("unexpected branch command: {args:?}"),
            };
            assert!(response.len() <= limit);
            Ok((response.into_bytes(), false))
        }),
    )
    .expect("register branch fixture");
    let service = vega_conversation::BranchWorkspaceService::new(root.path())
        .expect("owned branch fixture service");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("owned branch fixture runtime");
    runtime
        .block_on(service.refresh(tokio_util::sync::CancellationToken::new()))
        .expect("owned branch fixture snapshot")
}
