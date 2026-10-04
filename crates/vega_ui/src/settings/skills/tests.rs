use super::*;
use gpui_kit::{
    Bounds, Context, Entity, Render, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowHandle, WindowOptions, size,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use vega_store::Store;

struct Harness(Entity<SettingsView>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn stale_skills_projection() -> SkillSettingsProjection {
    SkillSettingsProjection {
        consent_generation: 1,
        revocation_generation: 0,
        global_enabled: false,
        automatic_enabled: false,
        selected_project_id: None,
        project_enabled: false,
        project_automatic: false,
        sources: vec![SkillSourceView {
            id: "stale-source".into(),
            scope: SkillUiScope::Imported,
            project_id: None,
            configured_root: PathBuf::from("/stale/configured"),
            canonical_root: PathBuf::from("/stale/canonical"),
            source_label: "stale-source".into(),
            enabled: false,
            automatic: false,
            diagnostic: None,
            candidates: Vec::new(),
        }],
    }
}

#[gpui_kit::test]
async fn issue74_native_folder_picker_previews_exact_root_before_link(cx: &mut TestAppContext) {
    let owned = tempdir().unwrap();
    let config = owned.path().join("config");
    let imported = owned.path().join("external-skills");
    let candidate = imported.join("reviewer");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&candidate).unwrap();
    fs::write(
        candidate.join("SKILL.md"),
        "---\nname: reviewer\ndescription: Review changes.\n---\nPRIVATE BODY\n",
    )
    .unwrap();
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let service = SkillSettingsService::new(database, config, None);
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(super::super::SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(
            vega_theme::Layout::SIDEBAR_WIDTH,
        ));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.section = 6;
        view.set_skills_service(Some(service.clone()), cx);
    });
    let root = view.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(1200.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills Settings window");
    cx.run_until_parked();
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-page-skills")
            .is_some()
    );
    assert!(service.projection().unwrap().sources.is_empty());
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-import-folder")
            .expect("native import action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    view.update(cx, |view, cx| {
        view.skills_section_changed(5, cx);
        view.section = 5;
    });
    cx.simulate_path_prompt_response(|_| Some(vec![imported.clone()]));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.skills.root_preview.is_none()));
    assert!(service.projection().unwrap().sources.is_empty());
    view.update(cx, |view, cx| {
        view.skills_section_changed(6, cx);
        view.section = 6;
    });
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-import-folder")
            .expect("native import action after reentry");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![imported.clone()]));
    cx.run_until_parked();
    let preview = view.read_with(cx, |view, _| view.skills.root_preview.clone());
    let preview = preview.expect("exact root preview");
    assert_eq!(preview.canonical_root, imported.canonicalize().unwrap());
    assert_eq!(preview.candidates.len(), 1);
    assert!(service.projection().unwrap().sources.is_empty());
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("skills-link-root")
            .is_some()
    );
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-link-root")
            .expect("link action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    let linked = service.projection().unwrap();
    assert_eq!(linked.sources.len(), 1);
    assert_eq!(
        linked.sources[0].canonical_root,
        imported.canonicalize().unwrap()
    );
    assert_eq!(linked.sources[0].source_label, preview.source_label);
    let source_id = linked.sources[0].id.clone();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let selector: &'static str =
            Box::leak(format!("skills-review-{source_id}-reviewer").into_boxed_str());
        let button = visual
            .debug_bounds(selector)
            .expect("candidate review action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    let body = view.read_with(cx, |view, _| view.skills.body_preview.clone());
    let body = body.expect("full Skill preview");
    assert!(body.body.contains("name: reviewer"));
    assert!(body.body.contains("PRIVATE BODY"));
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-approve-body")
            .expect("SHA approval action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    let approved = service.projection().unwrap();
    assert_eq!(
        approved.sources[0].candidates[0].approved_sha256.as_deref(),
        Some(body.content_sha256.as_str())
    );
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let selector: &'static str =
            Box::leak(format!("skills-unlink-{source_id}").into_boxed_str());
        let button = visual.debug_bounds(selector).expect("unlink action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(service.projection().unwrap().sources.is_empty());
    assert!(candidate.join("SKILL.md").is_file());
}

#[gpui_kit::test]
async fn issue74_skills_settings_import_is_keyboard_reachable(cx: &mut TestAppContext) {
    let owned = tempdir().unwrap();
    let config = owned.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let service = SkillSettingsService::new(database, config, None);
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(super::super::SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(
            vega_theme::Layout::SIDEBAR_WIDTH,
        ));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.section = 6;
        view.set_skills_service(Some(service.clone()), cx);
    });
    let root = view.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(1200.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills Settings window");
    cx.run_until_parked();
    let section_focus = view.read_with(cx, |view, _| view.section_focuses[6].clone());
    window
        .update(cx, |_, window, cx| window.focus(&section_focus, cx))
        .expect("focus Skills navigation row");
    let before_tab = view.read_with(cx, |view, _| view.skills.request_generation);
    cx.simulate_keystrokes(window.into(), "tab");
    assert!(
        !window
            .update(cx, |_, window, _| section_focus.is_focused(window))
            .expect("Skills navigation focus after Tab"),
        "Tab should move from Skills navigation into the Skills controls"
    );
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.skills.request_generation) > before_tab,
        "Tab should focus Refresh so Enter invokes its read-only reload"
    );
    cx.simulate_keystrokes(window.into(), "tab");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        service.projection().unwrap().global_enabled,
        "Tab from Refresh should focus the next visible Skills control"
    );
    let before_refresh = view.read_with(cx, |view, _| view.skills.request_generation);
    cx.simulate_keystrokes(window.into(), "shift-tab");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.skills.request_generation) > before_refresh,
        "Shift+Tab should return from the toggle to Refresh"
    );
    cx.simulate_keystrokes(window.into(), "shift-tab");
    assert!(
        window
            .update(cx, |_, window, _| section_focus.is_focused(window))
            .expect("Skills navigation focus after Shift+Tab"),
        "Shift+Tab from Refresh should return to Skills navigation"
    );
    cx.simulate_keystrokes(window.into(), "tab tab tab tab tab");
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "keyboard traversal should reach the external-folder import action"
    );
}

#[gpui_kit::test]
async fn issue74_s06_reload_hides_stale_projection_while_pending_and_after_failure(
    cx: &mut TestAppContext,
) {
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let service = SkillSettingsService::new(PathBuf::new(), PathBuf::new(), None);
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(super::super::SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(
            vega_theme::Layout::SIDEBAR_WIDTH,
        ));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, _| {
        view.section = 6;
        view.skills.service = Some(service);
        view.skills.projection = Some(stale_skills_projection());
        view.skills.reload_response_override = Some(response_rx);
    });
    let root = view.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(1200.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills Settings window");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view
            .skills
            .projection
            .as_ref()
            .map(|projection| projection.sources[0].id == "stale-source")),
        Some(true)
    );
    assert!(
        VisualTestContext::from_window(window.into(), cx)
            .debug_bounds("settings-skills")
            .is_some()
    );
    view.update(cx, |view, cx| {
        view.handle_skills_action(SkillAction::Reload, cx)
    });
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.skills.busy));
    assert!(view.read_with(cx, |view, _| view.skills.projection.is_none()));
    assert!(response_tx.send(Err(SkillSettingsError::Store)).is_ok());
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.skills.busy));
    assert!(view.read_with(cx, |view, _| view.skills.projection.is_none()));
    assert_eq!(
        view.read_with(cx, |view, _| view.skills.message.clone()),
        Some("Skills 数据库读写失败，请重试".into())
    );
}

fn skills_ui_fixture() -> (tempfile::TempDir, SkillSettingsService) {
    let owned = tempdir().unwrap();
    let config = owned.path().join("config");
    let candidate = config.join("skills/reviewer");
    fs::create_dir_all(&candidate).unwrap();
    fs::write(
        candidate.join("SKILL.md"),
        "---\nname: reviewer\ndescription: Review changes.\n---\nPRIVATE BODY\n",
    )
    .unwrap();
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    (owned, SkillSettingsService::new(database, config, None))
}

fn open_skills_settings(
    cx: &mut TestAppContext,
    service: SkillSettingsService,
) -> (Entity<SettingsView>, WindowHandle<Harness>) {
    cx.update(|cx| {
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(super::super::SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(
            vega_theme::Layout::SIDEBAR_WIDTH,
        ));
        crate::init(cx);
    });
    let view = cx.new(SettingsView::new_for_test);
    view.update(cx, |view, cx| {
        view.section = 6;
        view.set_skills_service(Some(service), cx);
    });
    let root = view.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(1200.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills Settings window");
    cx.run_until_parked();
    (view, window)
}

fn gated_first_skills_operation() -> (
    Arc<SkillsOperationGate>,
    tokio::sync::oneshot::Receiver<bool>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (entered, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    (
        Arc::new(SkillsOperationGate::new(entered, release_rx)),
        entered_rx,
        release_tx,
    )
}

#[gpui_kit::test]
async fn issue87_skills_settings_discards_late_result_after_page_leave(cx: &mut TestAppContext) {
    let (_owned, service) = skills_ui_fixture();
    let (view, _window) = open_skills_settings(cx, service);
    assert!(view.read_with(cx, |view, _| view.skills.projection.is_some()));
    let (gate, mut entered, release) = gated_first_skills_operation();
    view.update(cx, |view, cx| {
        view.skills.test_operation_gate = Some(gate);
        view.skills_operation(SkillOperation::PreviewGlobal, cx);
    });
    cx.run_until_parked();
    assert!(entered.try_recv().expect("completed global preview"));
    view.update(cx, |view, cx| {
        view.skills_section_changed(5, cx);
        view.section = 5;
    });
    let left_generation = view.read_with(cx, |view, _| view.skills.request_generation);
    assert!(!view.read_with(cx, |view, _| view.skills.busy));
    release.send(()).expect("release late preview result");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.skills.request_generation, left_generation);
        assert!(!view.skills.busy);
        assert!(view.skills.root_preview.is_none());
        assert!(view.skills.body_preview.is_none());
        assert!(view.skills.message.is_none());
    });
}

#[gpui_kit::test]
async fn issue87_skills_settings_discards_late_result_after_new_generation(
    cx: &mut TestAppContext,
) {
    let (_owned, service) = skills_ui_fixture();
    let (view, _window) = open_skills_settings(cx, service);
    let (gate, mut entered, release) = gated_first_skills_operation();
    view.update(cx, |view, cx| {
        view.skills.test_operation_gate = Some(gate);
        view.skills_operation(SkillOperation::PreviewGlobal, cx);
    });
    cx.run_until_parked();
    assert!(entered.try_recv().expect("completed global preview"));
    let old_generation = view.read_with(cx, |view, _| view.skills.request_generation);
    view.update(cx, |view, cx| {
        view.skills_operation(SkillOperation::Reload, cx)
    });
    cx.run_until_parked();
    let (new_generation, current_projection, busy, root_preview) = view.read_with(cx, |view, _| {
        (
            view.skills.request_generation,
            view.skills
                .projection
                .as_ref()
                .map(|projection| projection.consent_generation),
            view.skills.busy,
            view.skills.root_preview.clone(),
        )
    });
    assert!(new_generation > old_generation);
    assert!(!busy);
    assert!(current_projection.is_some());
    assert!(root_preview.is_none());
    release.send(()).expect("release superseded preview result");
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.skills.request_generation, new_generation);
        assert_eq!(
            view.skills
                .projection
                .as_ref()
                .map(|projection| projection.consent_generation),
            current_projection
        );
        assert!(!view.skills.busy);
        assert!(view.skills.root_preview.is_none());
    });
}
