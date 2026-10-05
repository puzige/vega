use super::*;
use gpui_kit::{
    Bounds, Context, Entity, Render, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowHandle, WindowOptions, size,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;
use vega_store::Store;

const IMPORT_LIMIT_LABEL: &str = "每个目录最多支持 128 个 Skill 候选，请减少数量后重试，未更改授权";

fn add_import_limit_candidates(root: &Path, count: usize) {
    for index in 0..count {
        let name = format!("limit-skill-{index:03}");
        let directory = root.join(&name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: Review owned changes.\n---\nPRIVATE LIMIT BODY\n"
            ),
        )
        .unwrap();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ImportAuthority {
    settings: vega_store::skills::SkillSettings,
    sources: Vec<vega_store::skills::SkillSourceRecord>,
    approvals: Vec<vega_store::skills::SkillApprovalRecord>,
}

fn import_authority(store: &Store) -> ImportAuthority {
    ImportAuthority {
        settings: vega_store::skills::read_settings(store.conn()).unwrap(),
        sources: vega_store::skills::list_sources(store.conn()).unwrap(),
        approvals: vega_store::skills::list_approved_skills(store.conn()).unwrap(),
    }
}

#[test]
fn issue87_s22_import_limit_label_is_bounded_and_existing_errors_keep_their_labels() {
    assert_eq!(
        skill_error_label(SkillSettingsError::TooManyCandidates),
        IMPORT_LIMIT_LABEL
    );
    assert!(IMPORT_LIMIT_LABEL.len() <= 256);
    assert!(IMPORT_LIMIT_LABEL.contains("128"));
    assert!(IMPORT_LIMIT_LABEL.contains("减少数量后重试"));
    assert!(IMPORT_LIMIT_LABEL.contains("未更改授权"));
    for (error, code, label) in [
        (
            SkillSettingsError::Store,
            "storage_failed",
            "Skills 数据库读写失败，请重试",
        ),
        (
            SkillSettingsError::NotFound,
            "not_found",
            "目录或 Skill 不存在，请刷新",
        ),
        (
            SkillSettingsError::Stale,
            "changed_review_required",
            "来源或文件已变化，请重新预览并审阅",
        ),
        (
            SkillSettingsError::Invalid,
            "invalid_source",
            "目录不安全或内容无效，未更改授权",
        ),
        (
            SkillSettingsError::PreviewRequired,
            "preview_required",
            "请先预览当前内容，再确认授权",
        ),
        (
            SkillSettingsError::SelectionLimit,
            "selection_limit",
            "一个任务最多可选择三个 Skills",
        ),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(skill_error_label(error), label);
    }
}

async fn assert_import_limit_mounted(cx: &mut TestAppContext, dark: bool, width: f32, height: f32) {
    let owned = tempdir().unwrap();
    let config = owned.path().join("config");
    let imported = owned.path().join("private-limit-input");
    fs::create_dir_all(&config).unwrap();
    add_import_limit_candidates(&imported, 129);
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let before = import_authority(&store);
    let service = SkillSettingsService::new(database.clone(), config.clone(), None);
    cx.update(|cx| {
        cx.set_global(if dark {
            vega_theme::Theme::dark()
        } else {
            vega_theme::Theme::light()
        });
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
                        size(px(width), px(height)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills import limit window");
    cx.run_until_parked();
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-import-folder")
            .expect("native import action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![imported.clone()]));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert!(!view.skills.busy);
        assert!(view.skills.root_preview.is_none());
        assert!(view.skills.body_preview.is_none());
        assert_eq!(view.skills.message.as_deref(), Some(IMPORT_LIMIT_LABEL));
        let message = view.skills.message.as_ref().unwrap();
        assert!(message.len() <= 256);
        assert!(!message.contains(imported.to_str().unwrap()));
        assert!(!message.contains("limit-skill-000"));
        assert!(!message.contains("PRIVATE LIMIT BODY"));
    });
    assert_eq!(import_authority(&store), before);
    assert!(service.projection().unwrap().sources.is_empty());
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let message = visual
            .debug_bounds("skills-message")
            .expect("mounted limit message");
        let page = visual
            .debug_bounds("settings-section-content")
            .expect("mounted Skills content viewport");
        assert!(message.size.width > px(0.));
        assert!(message.size.height > px(0.));
        assert!(message.origin.x >= page.origin.x);
        assert!(message.origin.y >= page.origin.y);
        assert!(message.right() <= page.right());
        assert!(message.bottom() <= page.bottom());
        assert!(message.bottom() <= px(height));
        assert!(visual.debug_bounds("skills-root-preview").is_none());
        assert!(visual.debug_bounds("skills-link-root").is_none());
        assert!(visual.debug_bounds("skills-body-preview").is_none());
        let import = visual
            .debug_bounds("skills-import-folder")
            .expect("retryable import action");
        visual.simulate_click(import.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(import_authority(&store), before);
    for index in 1..129 {
        fs::remove_dir_all(imported.join(format!("limit-skill-{index:03}"))).unwrap();
    }
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let button = visual
            .debug_bounds("skills-import-folder")
            .expect("reduced-input import action");
        visual.simulate_click(button.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|_| Some(vec![imported.clone()]));
    cx.run_until_parked();
    let preview = view
        .read_with(cx, |view, _| view.skills.root_preview.clone())
        .expect("reduced-input preview");
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(preview.candidates[0].name, "limit-skill-000");
    assert_eq!(preview.canonical_root, imported.canonicalize().unwrap());
    assert!(view.read_with(cx, |view, _| view.skills.message.is_none()));
    assert_eq!(import_authority(&store), before);
    {
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        assert!(visual.debug_bounds("skills-root-preview").is_some());
        assert!(visual.debug_bounds("skills-link-root").is_some());
        assert!(visual.debug_bounds("skills-message").is_none());
        let cancel = visual
            .debug_bounds("skills-cancel-root-preview")
            .expect("preview cancel action");
        let viewport = visual
            .debug_bounds("settings-section-content")
            .expect("mounted Skills content viewport");
        if cancel.bottom() > viewport.bottom() {
            visual.simulate_event(gpui_kit::ScrollWheelEvent {
                position: viewport.center(),
                delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                    px(0.),
                    viewport.bottom() - cancel.bottom() - px(8.),
                )),
                modifiers: Default::default(),
                touch_phase: gpui_kit::TouchPhase::Moved,
            });
            visual.run_until_parked();
        }
        let cancel = visual
            .debug_bounds("skills-cancel-root-preview")
            .expect("visible preview cancel action");
        assert!(cancel.origin.y >= viewport.origin.y);
        assert!(cancel.bottom() <= viewport.bottom());
        visual.simulate_click(cancel.center(), Default::default());
    }
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.skills.root_preview.is_none()));
    assert!(view.read_with(cx, |view, _| view.skills.body_preview.is_none()));
    assert_eq!(
        service.apply(
            before.settings.consent_generation,
            SkillSettingsMutation::LinkRoot {
                preview_token: preview.token,
            },
        ),
        Err(SkillSettingsError::PreviewRequired)
    );
    assert_eq!(import_authority(&store), before);
    view.update(cx, |view, _| view.close_skills_session());
    drop(service);
    drop(store);
    let reopened = Store::open(&database).unwrap();
    reopened.migrate().unwrap();
    assert_eq!(import_authority(&reopened), before);
    let reopened_service = SkillSettingsService::new(database, config, None);
    assert!(reopened_service.projection().unwrap().sources.is_empty());
    assert!(imported.join("limit-skill-000/SKILL.md").is_file());
}

#[gpui_kit::test]
async fn issue87_s22_import_limit_is_mounted_narrow_light_and_retry_can_cancel(
    cx: &mut TestAppContext,
) {
    assert_import_limit_mounted(cx, false, 960., 600.).await;
}

#[gpui_kit::test]
async fn issue87_s22_import_limit_is_mounted_narrow_dark_and_retry_can_cancel(
    cx: &mut TestAppContext,
) {
    assert_import_limit_mounted(cx, true, 960., 600.).await;
}

#[gpui_kit::test]
async fn issue87_s22_import_limit_is_mounted_wide_light_and_retry_can_cancel(
    cx: &mut TestAppContext,
) {
    assert_import_limit_mounted(cx, false, 1403., 860.).await;
}

#[gpui_kit::test]
async fn issue87_s22_import_limit_is_mounted_wide_dark_and_retry_can_cancel(
    cx: &mut TestAppContext,
) {
    assert_import_limit_mounted(cx, true, 1403., 860.).await;
}

fn candidate_view(name: &str, model_winner: bool) -> vega_conversation::types::SkillCandidateView {
    let content_sha256 = "a".repeat(64);
    vega_conversation::types::SkillCandidateView {
        name: name.into(),
        description: Some("Review changes.".into()),
        content_sha256: Some(content_sha256.clone()),
        approved_sha256: model_winner.then_some(content_sha256),
        size_bytes: Some(64),
        enabled: model_winner,
        automatic: model_winner,
        model_winner,
        diagnostic: (!model_winner).then(|| "review_required".into()),
    }
}

fn source_view(
    id: &str,
    scope: vega_conversation::types::SkillUiScope,
    source_label: &str,
    candidates: Vec<vega_conversation::types::SkillCandidateView>,
) -> vega_conversation::types::SkillSourceView {
    vega_conversation::types::SkillSourceView {
        id: id.into(),
        scope,
        project_id: None,
        configured_root: PathBuf::new(),
        canonical_root: PathBuf::new(),
        source_label: source_label.into(),
        enabled: false,
        automatic: false,
        diagnostic: None,
        candidates,
    }
}

fn projection_with_global_winner() -> SkillSettingsProjection {
    let mut projection = SkillSettingsProjection {
        consent_generation: 0,
        revocation_generation: 0,
        global_enabled: false,
        automatic_enabled: false,
        selected_project_id: None,
        project_enabled: false,
        project_automatic: false,
        sources: vec![
            source_view(
                "project-source",
                vega_conversation::types::SkillUiScope::Project,
                "project-label",
                vec![candidate_view("reviewer", false)],
            ),
            source_view(
                "global-source",
                vega_conversation::types::SkillUiScope::VegaGlobal,
                "global-label",
                vec![candidate_view("reviewer", true)],
            ),
            source_view(
                "import-source",
                vega_conversation::types::SkillUiScope::Imported,
                "skill-0123456789abcdef",
                vec![candidate_view("reviewer", false)],
            ),
        ],
    };
    projection.global_enabled = true;
    projection.automatic_enabled = true;
    projection.sources[1].enabled = true;
    projection.sources[1].automatic = true;
    projection.sources[1].candidates[0] = candidate_view("reviewer", true);
    projection
}

#[test]
fn issue87_s04_global_winner_labels_project_and_import_duplicates() {
    let projection = projection_with_global_winner();
    let expected = Some("被 Vega 全局同名项遮蔽".to_owned());

    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[0].candidates[0]),
        expected
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[2].candidates[0]),
        expected
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[1].candidates[0]),
        None
    );
}

#[test]
fn issue87_s04_imported_winner_label_uses_opaque_source_label() {
    let mut projection = projection_with_global_winner();
    projection.global_enabled = false;
    projection.automatic_enabled = false;
    projection.sources[1].enabled = false;
    projection.sources[1].automatic = false;
    projection.sources[1].candidates[0].model_winner = false;
    projection.sources[2].enabled = true;
    projection.sources[2].automatic = true;
    projection.sources[2].candidates[0] = candidate_view("reviewer", true);
    projection.sources[2].configured_root = PathBuf::from("/private/imported-skills");
    projection.sources[2].canonical_root = PathBuf::from("/private/imported-skills");

    let label = skill_shadow_label(&projection, &projection.sources[0].candidates[0]);

    assert_eq!(
        label.as_deref(),
        Some("被外部导入来源「skill-0123456789abcdef」的同名项遮蔽")
    );
    let label = label.unwrap();
    assert!(!label.contains("/private/imported-skills"));
    assert!(!label.contains("Review changes."));
}

#[test]
fn issue87_s04_candidate_without_model_winner_is_not_marked_shadowed() {
    let mut projection = projection_with_global_winner();
    projection.global_enabled = false;
    projection.automatic_enabled = false;
    projection.sources[1].enabled = false;
    projection.sources[1].automatic = false;
    projection.sources[1].candidates[0].model_winner = false;

    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[0].candidates[0]),
        None
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[2].candidates[0]),
        None
    );
}

#[test]
fn issue87_s04_project_winner_labels_global_and_import_duplicates() {
    let mut projection = projection_with_global_winner();
    projection.sources[0].enabled = true;
    projection.sources[0].automatic = true;
    projection.project_enabled = true;
    projection.project_automatic = true;
    projection.sources[0].candidates[0] = candidate_view("reviewer", true);
    projection.sources[1].candidates[0].model_winner = false;

    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[1].candidates[0]),
        Some("被当前项目同名项遮蔽".into())
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[2].candidates[0]),
        Some("被当前项目同名项遮蔽".into())
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[0].candidates[0]),
        None
    );
}

#[test]
fn issue87_s04_invalid_same_name_diagnostic_is_not_marked_shadowed() {
    let mut projection = projection_with_global_winner();
    let mut invalid = candidate_view("reviewer", false);
    invalid.content_sha256 = None;
    invalid.diagnostic = Some("malformed_yaml".into());
    projection.sources.push(source_view(
        "invalid-import-source",
        vega_conversation::types::SkillUiScope::Imported,
        "skill-fedcba9876543210",
        vec![invalid],
    ));

    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[3].candidates[0]),
        None
    );
    assert_eq!(
        skill_shadow_label(&projection, &projection.sources[0].candidates[0]),
        Some("被 Vega 全局同名项遮蔽".into())
    );

    let mut invalid_winner_projection = projection_with_global_winner();
    invalid_winner_projection.sources[1].candidates[0].content_sha256 = None;
    invalid_winner_projection.sources[1].candidates[0].diagnostic = Some("malformed_yaml".into());
    assert_eq!(
        skill_shadow_label(
            &invalid_winner_projection,
            &invalid_winner_projection.sources[0].candidates[0]
        ),
        None
    );
}

#[gpui_kit::test]
async fn issue87_s04_shadow_label_is_mounted_for_shadowed_settings_rows(cx: &mut TestAppContext) {
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
        view.skills.projection = Some(projection_with_global_winner());
    });
    let root = view.clone();
    let window: WindowHandle<Harness> = cx
        .update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1403.), px(1800.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("Skills Settings window");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    assert!(
        visual
            .debug_bounds("skills-shadow-project-source-reviewer")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("skills-shadow-import-source-reviewer")
            .is_some()
    );
    assert!(
        visual
            .debug_bounds("skills-shadow-global-source-reviewer")
            .is_none()
    );
}

struct Harness(Entity<SettingsView>);

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn assert_s21_intro_is_bounded(
    cx: &mut TestAppContext,
    theme: vega_theme::Theme,
    width: f32,
    height: f32,
    sidebar_width: f32,
) {
    let owned = tempdir().unwrap();
    let config = owned.path().join("config");
    fs::create_dir_all(config.join("skills")).unwrap();
    let database = owned.path().join("vega.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let service = SkillSettingsService::new(database, config, None);
    let before = service.projection().unwrap();
    assert!(!before.global_enabled);
    assert!(!before.automatic_enabled);
    assert!(!before.project_enabled);
    assert!(!before.project_automatic);
    assert!(before.sources.is_empty());

    cx.update(|cx| {
        cx.set_global(theme);
        cx.set_global(super::super::SettingsOpen(true));
        cx.set_global(crate::sidebar::SidebarWidth(sidebar_width));
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
                        size(px(width), px(height)),
                        cx,
                    ))),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Harness(root)),
            )
        })
        .expect("S21 Skills Settings window");
    cx.run_until_parked();

    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let navigation = visual
        .debug_bounds("settings-navigation")
        .expect("Settings navigation");
    assert_eq!(navigation.size.width, px(sidebar_width));
    assert!(visual.debug_bounds("settings-page-skills").is_some());
    let content = visual
        .debug_bounds("settings-content-column")
        .expect("Settings content column");
    assert_eq!(
        content.size.width,
        px((width - sidebar_width - 48.).min(vega_theme::Layout::SETTINGS_CONTENT_MAX_WIDTH))
    );
    let viewport = visual
        .debug_bounds("settings-section-content")
        .expect("Settings section viewport");
    assert!(viewport.size.width > px(0.));
    assert!(viewport.size.height > px(0.));
    let intro = visual
        .debug_bounds("skills-intro")
        .expect("Skills introduction");
    assert!(intro.size.width > px(0.));
    assert!(intro.size.height > px(0.));
    assert!(
        intro.size.height < px(30.),
        "S21 test-platform introduction height: {:?}",
        intro.size.height
    );
    assert!(intro.left() >= content.left());
    assert!(intro.right() <= content.right());
    assert!(intro.top() >= content.top());
    assert!(intro.bottom() <= content.bottom());
    assert!(intro.bottom() <= px(height));
    assert!(intro.left() >= viewport.left());
    assert!(intro.right() <= viewport.right());
    assert!(intro.top() >= viewport.top());
    assert!(intro.bottom() <= viewport.bottom());
    let refresh = visual
        .debug_bounds("skills-reload")
        .expect("Skills Refresh after introduction");
    assert!(refresh.top() >= intro.bottom());
    assert!(refresh.size.width > px(0.));
    assert!(refresh.size.height > px(0.));
    assert!(refresh.left() >= content.left());
    assert!(refresh.right() <= content.right());
    assert!(refresh.bottom() <= content.bottom());
    assert!(refresh.bottom() <= px(height));
    assert!(refresh.left() >= viewport.left());
    assert!(refresh.right() <= viewport.right());
    assert!(refresh.top() >= viewport.top());
    assert!(refresh.bottom() <= viewport.bottom());
    assert!(visual.debug_bounds("skills-global-enabled").is_some());
    assert!(visual.debug_bounds("skills-global-automatic").is_some());
    assert!(visual.debug_bounds("skills-import-folder").is_some());
    assert!(view.read_with(cx, |view, _| {
        !view.skills.busy
            && view.skills.message.is_none()
            && view.skills.root_preview.is_none()
            && view.skills.body_preview.is_none()
            && view.skills.projection.as_ref() == Some(&before)
    }));
    assert!(service.projection().unwrap() == before);
}

#[gpui_kit::test]
async fn issue87_s21_intro_minimum_window_light(cx: &mut TestAppContext) {
    assert_s21_intro_is_bounded(
        cx,
        vega_theme::Theme::light(),
        960.,
        600.,
        vega_theme::Layout::SIDEBAR_MAX_WIDTH,
    );
}

#[gpui_kit::test]
async fn issue87_s21_intro_minimum_window_dark(cx: &mut TestAppContext) {
    assert_s21_intro_is_bounded(
        cx,
        vega_theme::Theme::dark(),
        960.,
        600.,
        vega_theme::Layout::SIDEBAR_MAX_WIDTH,
    );
}

#[gpui_kit::test]
async fn issue87_s21_intro_normal_window_light(cx: &mut TestAppContext) {
    assert_s21_intro_is_bounded(
        cx,
        vega_theme::Theme::light(),
        1403.,
        860.,
        vega_theme::Layout::SIDEBAR_WIDTH,
    );
}

#[gpui_kit::test]
async fn issue87_s21_intro_normal_window_dark(cx: &mut TestAppContext) {
    assert_s21_intro_is_bounded(
        cx,
        vega_theme::Theme::dark(),
        1403.,
        860.,
        vega_theme::Layout::SIDEBAR_WIDTH,
    );
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
