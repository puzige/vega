use std::{fs, path::Path};

use gpui_kit::TestAppContext;
use tempfile::TempDir;

use super::*;

fn configuration() -> (TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.yaml");
    fs::write(&path, "schema_version: 1\ncredential_bindings: []\nproviders: []\ndefaults:\n  model: ''\n  permission_mode: confirm\n  permission_revision: 0\n  full_access_grant: null\nui:\n  theme: dark\n  language: en\n  sidebar_collapsed: false\n  sidebar_width: 304.0\n  projects_collapsed: false\n  sessions_collapsed: false\nagent:\n  composer_predictions_enabled: true\n  running_message_behavior: queue\n  desktop_use_enabled: false\n  browser_use_enabled: false\n  turn_limit: 0\n  codex_acp_profile: null\n").unwrap();
    let loaded = config::read_from(&path).unwrap();
    assert_eq!(loaded.ui.sidebar_width, config::SIDEBAR_WIDTH_DEFAULT);
    (directory, path)
}

fn assert_preferences(path: &Path, collapsed: bool, width: f32, projects: bool, sessions: bool) {
    let saved = config::read_from(path).unwrap();
    assert_eq!(saved.ui.sidebar_collapsed, collapsed);
    assert_eq!(saved.ui.sidebar_width, width);
    assert_eq!(saved.ui.projects_collapsed, projects);
    assert_eq!(saved.ui.sessions_collapsed, sessions);
}

#[gpui_kit::test]
fn callbacks_return_while_configuration_edit_is_locked(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    cx.update(|cx| initialize(path.clone(), cx));
    let mut edit = config::begin_existing_edit(&path).unwrap();
    cx.update(|cx| {
        submit(Patch::SidebarCollapsed(true), cx);
        submit(Patch::Width(350.0), cx);
        submit(Patch::ProjectsCollapsed(true), cx);
        submit(Patch::SessionsCollapsed(true), cx);
        assert!(cx.global::<SidebarCollapsed>().0);
        assert_eq!(cx.global::<SidebarWidth>().0, 350.0);
        assert!(cx.global::<ProjectsCollapsed>().0);
        assert!(cx.global::<SessionsCollapsed>().0);
        assert!(status(cx).saving);
        assert!(take_next(cx.global_mut::<SaveState>()).is_none());
    });
    assert!(!edit.config.ui.sidebar_collapsed);
    assert_eq!(edit.config.ui.sidebar_width, config::SIDEBAR_WIDTH_DEFAULT);
    edit.config.ui.theme = "light".to_string();
    edit.save().unwrap();
    drop(edit);
    cx.run_until_parked();
    assert_preferences(&path, true, 350.0, true, true);
    assert_eq!(config::read_from(&path).unwrap().ui.theme, "light");
    cx.read(|cx| {
        assert!(!status(cx).saving);
        assert!(status(cx).error.is_none());
    });
}

#[gpui_kit::test]
fn latest_fields_merge_without_losing_other_writers_or_yaml(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    let mut source = fs::read_to_string(&path).unwrap();
    source.push_str("\nexternal_flag: 'keep me'\n# retain external formatting\n");
    fs::write(&path, source).unwrap();
    cx.update(|cx| initialize(path.clone(), cx));
    let mut other_writer = config::begin_existing_edit(&path).unwrap();
    cx.update(|cx| {
        submit(Patch::Width(250.0), cx);
        let first = cx.global::<SaveState>().in_flight.clone().unwrap();
        submit(Patch::Width(320.0), cx);
        submit(Patch::ProjectsCollapsed(true), cx);
        submit(Patch::SessionsCollapsed(true), cx);
        submit(Patch::Width(340.0), cx);
        submit(Patch::SidebarCollapsed(true), cx);
        let current = cx.global::<SaveState>().in_flight.as_ref().unwrap();
        assert!(Arc::ptr_eq(&first.token, &current.token));
        assert_eq!(cx.global::<SidebarWidth>().0, 340.0);
    });
    other_writer.config.ui.theme = "light".to_string();
    other_writer.config.ui.language = "zh-CN".to_string();
    other_writer.config.defaults.model = "concurrent-model".to_string();
    other_writer.save().unwrap();
    drop(other_writer);
    cx.run_until_parked();
    assert_preferences(&path, true, 340.0, true, true);
    let saved = config::read_from(&path).unwrap();
    assert_eq!(saved.ui.theme, "light");
    assert_eq!(saved.ui.language, "zh-CN");
    assert_eq!(saved.defaults.model, "concurrent-model");
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("external_flag: 'keep me'"));
    assert!(source.contains("# retain external formatting"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    }
    cx.read(|cx| {
        assert_eq!(cx.global::<SidebarWidth>().0, 340.0);
        assert!(!status(cx).saving);
        assert!(!status(cx).can_retry);
        assert!(status(cx).error.is_none());
    });
}

#[gpui_kit::test]
fn failed_fields_survive_and_retry_saves_latest_values(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    let valid = fs::read(&path).unwrap();
    fs::write(&path, "ui: [broken").unwrap();
    cx.update(|cx| {
        initialize(path.clone(), cx);
        submit(Patch::SidebarCollapsed(true), cx);
        submit(Patch::Width(320.0), cx);
        submit(Patch::ProjectsCollapsed(true), cx);
        submit(Patch::SessionsCollapsed(true), cx);
    });
    cx.run_until_parked();
    cx.read(|cx| {
        let status = status(cx);
        assert!(status.error.is_some());
        assert!(status.can_retry);
        assert!(!status.saving);
        assert!(cx.global::<SidebarCollapsed>().0);
        assert_eq!(cx.global::<SidebarWidth>().0, 320.0);
    });
    cx.update(retry);
    cx.run_until_parked();
    cx.read(|cx| assert!(status(cx).can_retry));
    fs::write(&path, valid).unwrap();
    config::update_from(&path, |config| {
        config.ui.theme = "light".to_string();
    })
    .unwrap();
    cx.update(|cx| {
        retry(cx);
        assert!(status(cx).saving);
        assert!(status(cx).error.is_some());
        assert!(!status(cx).can_retry);
        let request = cx.global::<SaveState>().in_flight.clone().unwrap();
        retry(cx);
        assert!(Arc::ptr_eq(
            &request.token,
            &cx.global::<SaveState>().in_flight.as_ref().unwrap().token
        ));
        submit(Patch::Width(360.0), cx);
        submit(Patch::SidebarCollapsed(false), cx);
    });
    cx.run_until_parked();
    assert_preferences(&path, false, 360.0, true, true);
    assert_eq!(config::read_from(&path).unwrap().ui.theme, "light");
    cx.read(|cx| {
        assert!(status(cx).error.is_none());
        assert!(!status(cx).can_retry);
        assert!(!status(cx).saving);
        assert!(!cx.global::<SidebarCollapsed>().0);
        assert_eq!(cx.global::<SidebarWidth>().0, 360.0);
    });
}

#[gpui_kit::test]
fn show_reveals_all_blocks_and_saves_normalized_width(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    config::update_from(&path, |config| {
        config.ui.sidebar_collapsed = true;
        config.ui.projects_collapsed = true;
        config.ui.sessions_collapsed = true;
    })
    .unwrap();
    cx.update(|cx| {
        initialize(path.clone(), cx);
        submit(Patch::Show, cx);
        submit(Patch::Width(f32::NAN), cx);
        assert!(!cx.global::<SidebarCollapsed>().0);
        assert!(!cx.global::<ProjectsCollapsed>().0);
        assert!(!cx.global::<SessionsCollapsed>().0);
        assert!(cx.global::<SidebarExplicitlyShown>().0);
        assert_eq!(cx.global::<SidebarWidth>().0, config::SIDEBAR_WIDTH_DEFAULT);
    });
    cx.run_until_parked();
    assert_preferences(&path, false, config::SIDEBAR_WIDTH_DEFAULT, false, false);
}

#[gpui_kit::test]
fn configuration_switch_waits_for_old_worker_without_applying_its_failure(cx: &mut TestAppContext) {
    let (_old_directory, old_path) = configuration();
    let (_new_directory, new_path) = configuration();
    fs::write(&old_path, "ui: [broken").unwrap();
    cx.update(|cx| {
        initialize(old_path.clone(), cx);
        submit(Patch::SidebarCollapsed(true), cx);
        let old = cx.global::<SaveState>().in_flight.clone().unwrap();
        initialize(new_path.clone(), cx);
        submit(Patch::Show, cx);
        submit(Patch::Width(345.0), cx);
        let current = cx.global::<SaveState>().in_flight.as_ref().unwrap();
        assert!(Arc::ptr_eq(&old.token, &current.token));
        assert!(status(cx).error.is_none());
    });
    cx.run_until_parked();
    assert_preferences(&new_path, false, 345.0, false, false);
    assert_eq!(fs::read_to_string(&old_path).unwrap(), "ui: [broken");
    cx.read(|cx| {
        assert!(!cx.global::<SidebarCollapsed>().0);
        assert_eq!(cx.global::<SidebarWidth>().0, 345.0);
        assert!(status(cx).error.is_none());
        assert!(!status(cx).saving);
    });
}

#[gpui_kit::test]
fn configuration_switch_does_not_merge_old_success_into_new_patch(cx: &mut TestAppContext) {
    let (_old_directory, old_path) = configuration();
    let (_new_directory, new_path) = configuration();
    config::update_from(&new_path, |config| config.ui.sidebar_width = 330.0).unwrap();
    cx.update(|cx| {
        initialize(old_path.clone(), cx);
        submit(Patch::Width(260.0), cx);
        initialize(new_path.clone(), cx);
        cx.set_global(SidebarWidth(330.0));
        submit(Patch::SessionsCollapsed(true), cx);
    });
    cx.run_until_parked();
    assert_preferences(&old_path, false, 260.0, false, false);
    assert_preferences(&new_path, false, 330.0, false, true);
    cx.read(|cx| assert_eq!(cx.global::<SidebarWidth>().0, 330.0));
}

#[gpui_kit::test]
fn late_completion_cannot_clear_new_failure_or_replace_new_memory(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    cx.update(|cx| {
        initialize(path, cx);
        let state = cx.global_mut::<SaveState>();
        state.pending = Patch::Width(250.0).into();
        let old = take_next(state).unwrap();
        assert!(!complete(
            state,
            &old,
            Err(LocalizedCopy::new("settings.config.save_failed")),
        ));
        state.pending.merge(Patch::Width(355.0).into());
        let new = take_next(state).unwrap();
        assert!(!complete(
            state,
            &new,
            Err(LocalizedCopy::new("settings.config.save_failed")),
        ));
        cx.set_global(SidebarWidth(355.0));
        let state = cx.global_mut::<SaveState>();
        assert!(!complete(state, &old, Ok(())));
        assert_eq!(state.pending.width, Some(355.0));
        assert!(state.error.is_some());
        assert_eq!(cx.global::<SidebarWidth>().0, 355.0);
        assert!(status(cx).can_retry);
    });
}

#[gpui_kit::test]
fn successful_older_batch_keeps_error_until_latest_patch_is_saved(cx: &mut TestAppContext) {
    let (_directory, path) = configuration();
    cx.update(|cx| {
        initialize(path, cx);
        let state = cx.global_mut::<SaveState>();
        state.pending = Patch::Width(280.0).into();
        let first = take_next(state).unwrap();
        assert!(!complete(
            state,
            &first,
            Err(LocalizedCopy::new("settings.config.save_failed")),
        ));
        let retry = take_next(state).unwrap();
        state.pending.merge(Patch::Width(350.0).into());
        assert!(complete(state, &retry, Ok(())));
        assert!(state.error.is_some());
        assert_eq!(state.pending.width, Some(350.0));
        let latest = take_next(state).unwrap();
        assert!(complete(state, &latest, Ok(())));
        assert!(state.error.is_none());
        assert!(state.pending.is_empty());
    });
}
