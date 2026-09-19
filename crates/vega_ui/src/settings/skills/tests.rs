use super::*;
use gpui_kit::{
    Bounds, Context, Entity, Render, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowHandle, WindowOptions, size,
};
use std::fs;
use tempfile::tempdir;
use vega_store::Store;

struct Harness(Entity<SettingsView>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
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
    let section_focus = view.read_with(cx, |view, _| view.section_focuses[6].clone());
    window
        .update(cx, |_, window, cx| window.focus(&section_focus, cx))
        .expect("focus Skills navigation row");
    for _ in 0..5 {
        window
            .update(cx, |_, window, cx| window.focus_next(cx))
            .expect("advance keyboard focus");
    }
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "native import must activate by keyboard"
    );
}
