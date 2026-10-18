use super::*;
use gpui_kit::{Render, TestAppContext};

fn provider(name: &str, models: &[&str]) -> ProviderConfig {
    ProviderConfig {
        api: Default::default(),
        enabled: true,
        name: name.to_string(),
        base_url: format!("https://{name}.example.com"),
        models: models.iter().map(|m| m.to_string()).collect(),
        key_ref: name.to_string(),
    }
}

#[gpui_kit::test]
async fn prediction_preference_preserves_other_fields_and_conflicts_without_overwrite(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    AppConfig {
        providers: vec![provider("owned", &["model"])],
        ..Default::default()
    }
    .save_to(&path)
    .unwrap();
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    config::update_from(&path, |config| {
        config.providers[0].models.push("concurrent".into());
        config.ui.sidebar_collapsed = true;
    })
    .unwrap();
    view.update(cx, |view, cx| view.toggle_composer_predictions(cx));
    cx.run_until_parked();
    let saved = config::read_from(&path).unwrap();
    assert!(!saved.agent.composer_predictions_enabled);
    assert_eq!(saved.providers[0].models, ["model", "concurrent"]);
    assert!(saved.ui.sidebar_collapsed);
    cx.update(|cx| {
        assert!(
            !cx.global::<crate::conversation_stream::ComposerPredictionsEnabled>()
                .0
        )
    });
    config::update_from(&path, |config| {
        config.agent.composer_predictions_enabled = true
    })
    .unwrap();
    view.update(cx, |view, cx| view.toggle_composer_predictions(cx));
    cx.run_until_parked();
    assert!(
        config::read_from(&path)
            .unwrap()
            .agent
            .composer_predictions_enabled
    );
    assert!(view.read_with(cx, |view, _| {
        view.error
            .as_ref()
            .is_some_and(|error| error.contains("已更改"))
    }));
    cx.update(|cx| {
        assert!(
            !cx.global::<crate::conversation_stream::ComposerPredictionsEnabled>()
                .0
        )
    });
}

struct PredictionSettingsHarness(Entity<SettingsView>);

impl Render for PredictionSettingsHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.0
            .update(cx, |view, cx| view.render_composer_predictions_switch(cx))
    }
}

#[gpui_kit::test]
async fn prediction_settings_switch_uses_keyboard_and_disables_activation_while_saving(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config.toml");
    AppConfig::default().save_to(&path).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        crate::init(cx);
    });
    let view = cx.new(|cx| SettingsView::from_path(Some(path.clone()), cx));
    let root_view = view.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |_, cx| {
            cx.new(|_| PredictionSettingsHarness(root_view))
        })
        .unwrap()
    });
    cx.run_until_parked();
    window
        .update(cx, |_, window, cx| window.focus_next(cx))
        .unwrap();
    cx.simulate_keystrokes(window.into(), "space");
    cx.run_until_parked();
    assert!(
        !config::read_from(&path)
            .unwrap()
            .agent
            .composer_predictions_enabled
    );
    view.update(cx, |view, cx| {
        view.provider_management.saving = true;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(window.into(), "enter");
    cx.run_until_parked();
    assert!(
        !config::read_from(&path)
            .unwrap()
            .agent
            .composer_predictions_enabled
    );
    assert!(view.read_with(cx, |view, _| view.provider_management.saving));
}
