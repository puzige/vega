use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Context, Render, TestAppContext, WindowHandle};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use vega_conversation::types::{
    ModelCapabilityPatch, ModelCapabilityProjection, ModelCapabilitySettingsErrorCode,
    ModelCapabilitySettingsProjection,
};
use vega_runtime::model_compat::{
    ModelApi, ModelCapabilityOverride, ModelIdentity, ModelRoute, decode_model_catalog,
    resolve_model,
};

struct Harness {
    settings: Entity<SettingsView>,
}
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    window.focus_next(cx);
                    cx.stop_propagation();
                }
            })
            .child(self.settings.update(cx, |view, cx| {
                if view.section == 0 {
                    view.render_model_basics(cx)
                } else {
                    view.render_reasoning(cx)
                }
            }))
    }
}

type Saves = Rc<RefCell<Vec<ModelCapabilitySaveRequested>>>;

fn projection(
    allowed: Option<Vec<String>>,
    image: Option<bool>,
    known: bool,
) -> ModelCapabilityProjection {
    let source = json!({"version":1,"models":[{"provider":"fixture","api":"openai-completions","id":"model","baseUrl":"https://fixture.invalid/v1","reasoning":true,"input":["text"],"thinkingLevelMap":if known{json!({"minimal":null,"medium":null,"xhigh":null,"max":null,"low":"low","high":"high"})}else{json!({"xhigh":"xhigh","max":"max"})},"compat":{"supportsReasoningEffort":true}}]});
    let identity = ModelIdentity {
        provider: "fixture".into(),
        model: "model".into(),
    };
    let source = decode_model_catalog(&serde_json::to_vec(&source).unwrap())
        .unwrap()
        .lookup(
            &ModelRoute::new(ModelApi::ChatCompletions, "https://fixture.invalid/v1").unwrap(),
            &identity,
        )
        .unwrap();
    let user_override = (allowed.is_some() || image.is_some()).then_some(ModelCapabilityOverride {
        identity,
        allowed_efforts: allowed,
        image_input: image,
    });
    let resolved = resolve_model(&source, user_override.as_ref()).unwrap();
    ModelCapabilityProjection {
        resolved: Box::new(resolved),
        user_override,
    }
}

fn ready(
    model: ModelCapabilityProjection,
    error: Option<ModelCapabilitySettingsErrorCode>,
) -> ModelCapabilitySettingsProjection {
    ModelCapabilitySettingsProjection::Ready {
        generation: 4,
        models: vec![model],
        error,
    }
}

fn setup(
    cx: &mut TestAppContext,
    model: ModelCapabilityProjection,
) -> (
    Entity<SettingsView>,
    WindowHandle<Harness>,
    Saves,
    Rc<RefCell<usize>>,
) {
    let saves = Rc::new(RefCell::new(Vec::new()));
    let observed = saves.clone();
    let reloads = Rc::new(RefCell::new(0));
    let observed_reload = reloads.clone();
    let settings = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        let settings = cx.new(|cx| {
            let mut view = SettingsView::from_config(
                AppConfig {
                    providers: vec![ProviderConfig {
                        name: "fixture".into(),
                        base_url: "https://fixture.invalid/v1".into(),
                        models: vec!["model".into()],
                        key_ref: "key".into(),
                        api: config::ProviderApi::ChatCompletions,
                        enabled: true,
                    }],
                    ..Default::default()
                },
                None,
                cx,
            );
            view.model_settings = Some(("fixture".into(), "model".into()));
            view.section = 2;
            view.apply_model_capability_projection(ready(model, None), cx);
            view
        });
        cx.subscribe(
            &settings,
            move |_, event: &ModelCapabilitySaveRequested, _| {
                observed.borrow_mut().push(event.clone())
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            move |_, _: &ModelCapabilityReloadRequested, _| *observed_reload.borrow_mut() += 1,
        )
        .detach();
        settings
    });
    let owned = settings.clone();
    let window = cx.add_window(move |_, _| Harness { settings: owned });
    cx.run_until_parked();
    (settings, window, saves, reloads)
}

fn upsert(event: &ModelCapabilitySaveRequested) -> &ModelCapabilityOverride {
    match &event.patch {
        ModelCapabilityPatch::Upsert(record) => record,
        _ => panic!("expected capability upsert"),
    }
}

#[gpui_kit::test]
fn effort_controls_edit_a_set_in_draft_and_save_without_freezing_image_inheritance(
    cx: &mut TestAppContext,
) {
    let (_, window, saves, _) = setup(cx, projection(None, None, false));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(true)
        );
        window.click("model-capability-effort-0-high", cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(false)
        );
        assert_eq!(
            window.find("model-capability-effort-0-low").checked(),
            Some(true)
        );
        assert!(saves.borrow().is_empty());
        window.click("model-capability-save-0", cx);
    })
    .unwrap();
    assert_eq!(saves.borrow().len(), 1);
    let saves = saves.borrow();
    let patch = upsert(&saves[0]);
    assert_eq!(
        patch.allowed_efforts.as_ref().unwrap(),
        &vec!["minimal", "low", "medium", "xhigh", "max"]
    );
    assert_eq!(patch.image_input, None);
    assert_eq!(patch.identity.provider, "fixture");
    assert_eq!(patch.identity.model, "model");
}

#[gpui_kit::test]
fn image_control_edits_basic_information_without_freezing_effort_inheritance(
    cx: &mut TestAppContext,
) {
    let (settings, window, saves, _) = setup(cx, projection(None, None, false));
    cx.update_window(window.into(), |_, window, cx| {
        settings.update(cx, |view, cx| {
            view.section = 0;
            view.open_model_settings("fixture", "model", 0, cx);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("model-capability-image-0").checked(),
            Some(false)
        );
        window.click("model-capability-image-0", cx);
        assert!(saves.borrow().is_empty());
        window.click("model-capability-save-0", cx);
    })
    .unwrap();
    assert_eq!(saves.borrow().len(), 1);
    let saves = saves.borrow();
    let patch = upsert(&saves[0]);
    assert_eq!(patch.allowed_efforts, None);
    assert_eq!(patch.image_input, Some(true));
}

#[gpui_kit::test]
fn cancel_discards_draft_and_restore_automatic_emits_only_exact_reset(cx: &mut TestAppContext) {
    let (_, window, saves, _) = setup(
        cx,
        projection(Some(vec!["low".into(), "high".into()]), Some(true), false),
    );
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("model-capability-effort-0-high", cx);
        window.click("model-capability-cancel-0", cx);
        assert!(saves.borrow().is_empty());
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(true)
        );
        window.click("model-capability-reset-0", cx);
    })
    .unwrap();
    assert_eq!(saves.borrow().len(), 1);
    assert!(
        matches!(&saves.borrow()[0].patch,ModelCapabilityPatch::Reset{identity} if identity.provider=="fixture"&&identity.model=="model")
    );
}

#[gpui_kit::test]
fn unavailable_wire_levels_are_disabled_and_cannot_enter_save_patch(cx: &mut TestAppContext) {
    let (_, window, saves, _) = setup(cx, projection(None, None, true));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let disabled = window.find("model-capability-effort-0-medium");
        assert_eq!(disabled.disabled(), Some(true));
        assert!(
            disabled
                .label()
                .is_some_and(|label| label.contains("不支持"))
        );
        window.click("model-capability-effort-0-medium", cx);
        assert!(saves.borrow().is_empty());
        window.click("model-capability-effort-0-high", cx);
        window.click("model-capability-save-0", cx);
    })
    .unwrap();
    assert_eq!(
        upsert(&saves.borrow()[0]).allowed_efforts.as_deref(),
        Some(["low".to_string()].as_slice())
    );
}

#[gpui_kit::test]
fn keyboard_activation_uses_real_focus_and_matches_pointer_set_edit(cx: &mut TestAppContext) {
    let (settings, window, saves, _) = setup(cx, projection(None, None, false));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let focus = settings
            .read(cx)
            .model_capability_focus(&ModelCapabilityFocusTarget::Effort {
                index: 0,
                effort: "high".into(),
            })
            .unwrap();
        window.focus(&focus, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").focused(),
            Some(true)
        );
        window.press("space", cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(false)
        );
        window.press("enter", cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(true)
        );
        window.press("tab", cx);
        assert_ne!(
            window.find("model-capability-effort-0-high").focused(),
            Some(true)
        );
        assert!(saves.borrow().is_empty());
    })
    .unwrap();
}

#[gpui_kit::test]
fn saving_prevents_duplicates_and_failed_conflict_retains_edit_for_reload(cx: &mut TestAppContext) {
    let base = projection(None, None, false);
    let (settings, window, saves, reloads) = setup(cx, base.clone());
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        window.click("model-capability-effort-0-high", cx);
        window.click("model-capability-save-0", cx);
        let event = saves.borrow()[0].clone();
        assert!(
            settings
                .read(cx)
                .model_capability_save_is_current(event.generation, event.operation_id)
        );
        assert!(
            !settings
                .read(cx)
                .model_capability_save_is_current(event.generation + 1, event.operation_id)
        );
        assert_eq!(
            window.find("model-capability-save-0").disabled(),
            Some(true)
        );
        settings.update(cx, |view, cx| {
            view.apply_model_capability_projection(
                ready(
                    base.clone(),
                    Some(ModelCapabilitySettingsErrorCode::Conflict),
                ),
                cx,
            )
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("model-capability-effort-0-high").checked(),
            Some(false)
        );
        assert_eq!(saves.borrow().len(), 1);
        window.click("model-capability-reload", cx);
        assert_eq!(*reloads.borrow(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_show_capabilities_without_default_preference_or_protocol_controls(
    cx: &mut TestAppContext,
) {
    let (_, window, _, _) = setup(cx, projection(None, None, false));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("model-capability-effort-0-high").is_some());
        for old in [
            "reasoning-preference-0-option-0",
            "reasoning-protocol-0",
            "reasoning-template-0",
        ] {
            assert!(window.try_find(old).is_none());
        }
    })
    .unwrap();
}
