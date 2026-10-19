use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Context, Render, TestAppContext, WindowHandle};
use std::cell::RefCell;
use std::rc::Rc;

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
            .child(
                self.settings
                    .update(cx, |view, cx| view.render_reasoning(cx)),
            )
    }
}

fn profile(endpoint: &str, model: &str) -> ReasoningProfileProjection {
    vega_conversation::types::chat_compatibility(Some(endpoint), model).map_or_else(
        || ReasoningProfileProjection::unknown("fixture", model),
        |facts| ReasoningProfileProjection::from_chat_compatibility("fixture", model, facts),
    )
}

fn ready(
    profile: ReasoningProfileProjection,
    error: Option<vega_conversation::types::ReasoningSettingsErrorCode>,
) -> vega_conversation::types::ReasoningSettingsProjection {
    vega_conversation::types::ReasoningSettingsProjection::Ready {
        generation: 1,
        profiles: vec![profile],
        error,
    }
}

type Saves = Rc<RefCell<Vec<ReasoningProfileSaveRequested>>>;

fn setup(
    cx: &mut TestAppContext,
    profile: ReasoningProfileProjection,
) -> (
    Entity<SettingsView>,
    WindowHandle<Harness>,
    Saves,
    Rc<RefCell<usize>>,
) {
    let saves = Rc::new(RefCell::new(Vec::new()));
    let observed = saves.clone();
    let reloads = Rc::new(RefCell::new(0));
    let observed_reloads = reloads.clone();
    let settings = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        let settings = cx.new(|cx| {
            let mut view = SettingsView::from_config(AppConfig::default(), None, cx);
            view.section = 2;
            view.model_settings = Some((profile.provider.clone(), profile.model.clone()));
            view.apply_reasoning_projection(ready(profile, None), cx);
            view
        });
        cx.subscribe(
            &settings,
            move |_, event: &ReasoningProfileSaveRequested, _| {
                observed.borrow_mut().push(event.clone())
            },
        )
        .detach();
        cx.subscribe(&settings, move |_, _: &ReasoningReloadRequested, _| {
            *observed_reloads.borrow_mut() += 1
        })
        .detach();
        settings
    });
    let owned = settings.clone();
    let window = cx.add_window(move |_, _| Harness { settings: owned });
    cx.run_until_parked();
    (settings, window, saves, reloads)
}

fn labels(window: &Window) -> Vec<String> {
    gpui_kit::base::test_support::snapshots(window)
        .iter()
        .filter_map(|snapshot| snapshot.label().map(str::to_string))
        .collect()
}

fn choose(
    cx: &mut TestAppContext,
    window: WindowHandle<Harness>,
    settings: &Entity<SettingsView>,
    profile: ReasoningProfileProjection,
    name: &str,
) {
    cx.update_window(window.into(), |_, window, cx| {
        settings.update(cx, |view, cx| {
            view.apply_reasoning_projection(ready(profile, None), cx)
        });
        window.render_frame(cx);
        let view = settings.read(cx);
        let vega_conversation::types::ReasoningSettingsProjection::Ready { profiles, .. } =
            &view.reasoning
        else {
            panic!("ready")
        };
        let choices = super::reasoning_state::reasoning_choices(&profiles[0]);
        let option = choices
            .iter()
            .position(|choice| format!("{choice:?}") == name)
            .expect("actual available choice");
        let reload = view.reasoning_focuses[0].1.clone();
        reload.focus(window, cx);
        for _ in 0..=option {
            window.press("tab", cx);
        }
        assert_eq!(
            window
                .find(format!("reasoning-preference-0-option-{option}"))
                .focused(),
            Some(true)
        );
        window.press(if option % 2 == 0 { "enter" } else { "space" }, cx);
    })
    .expect("keyboard selection");
}

#[gpui_kit::test]
fn boolean_models_offer_only_real_on_off_choices_and_each_keyboard_action_saves_its_choice(
    cx: &mut TestAppContext,
) {
    for (endpoint, model) in [
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
        ),
        ("https://api.together.ai/v1", "Qwen/Qwen3.5-9B"),
        ("https://inference.baseten.co/v1", "moonshotai/Kimi-K2.6"),
    ] {
        let profile = profile(endpoint, model);
        let (settings, window, saves, _) = setup(cx, profile.clone());
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let rendered = labels(window);
            assert!(rendered.iter().any(|label| label == "开启思考"));
            assert!(rendered.iter().any(|label| label == "关闭思考"));
            assert!(rendered.iter().all(|label| {
                ![
                    "最少",
                    "低",
                    "中",
                    "高",
                    "很高",
                    "最高",
                    "协议模板",
                    "允许档位",
                    "支持范围",
                    "高级协议",
                ]
                .contains(&label.as_str())
            }));
            assert_eq!(settings.read(cx).reasoning_focuses.len(), 4);
            assert!(window.try_find("reasoning-advanced-0").is_none());
        })
        .expect("boolean render");
        for expected in ["Enabled", "Disabled"] {
            choose(cx, window, &settings, profile.clone(), expected);
            let saved = saves
                .borrow()
                .last()
                .expect("typed preference save")
                .clone();
            assert_eq!(format!("{:?}", saved.profile.preference), expected);
            assert!(saved.profile.freeze().is_ok());
            let mut only_preference = saved.base.clone();
            only_preference.preference = saved.profile.preference.clone();
            assert_eq!(saved.profile, only_preference);
        }
    }
}

#[gpui_kit::test]
fn unknown_gpt_and_future_model_settings_show_and_keyboard_save_all_six_real_efforts(
    cx: &mut TestAppContext,
) {
    for model in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "future-model-not-in-any-catalog",
    ] {
        let profile = profile("https://proxy.example/v1", model);
        let (settings, window, saves, _) = setup(cx, profile.clone());
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(settings.read(cx).reasoning_focuses.len(), 8);
            let rendered = labels(window);
            for label in ["提供方默认", "最少", "低", "中", "高", "很高", "最高"] {
                assert!(
                    rendered.iter().any(|actual| actual == label),
                    "{model}/{label}"
                );
            }
            assert!(
                !rendered
                    .iter()
                    .any(|label| label == "开启思考" || label == "关闭思考")
            );
        })
        .expect("fallback render");
        for effort in ["minimal", "low", "medium", "high", "xhigh", "max"] {
            choose(
                cx,
                window,
                &settings,
                profile.clone(),
                &format!("Effort({effort:?})"),
            );
            let saved = saves.borrow().last().expect("saved effort").clone();
            assert_eq!(
                saved.profile.preference,
                ReasoningChoice::Effort(effort.into())
            );
            let frozen = saved.profile.freeze().expect("real freeze");
            let body = vega_runtime::test_support::encode(
                "https://proxy.example/v1",
                false,
                &vega_runtime::ChatRequest {
                    model: model.into(),
                    messages: vec![],
                    tools: vec![],
                    max_tokens: None,
                    reasoning: Some(frozen),
                },
            )
            .expect("real wire");
            assert_eq!(body["reasoning_effort"], effort);
        }
    }
}

#[gpui_kit::test]
fn invalid_known_preference_is_visible_reloadable_and_repairable_to_real_enabled(
    cx: &mut TestAppContext,
) {
    let endpoint = "https://api.together.ai/v1";
    let model = "Qwen/Qwen3.5-9B";
    let mut invalid = profile(endpoint, model);
    invalid.preference = ReasoningChoice::Effort("high".into());
    let (settings, window, saves, reloads) = setup(cx, invalid.clone());
    cx.update_window(window.into(), |_, window, cx| {
        settings.update(cx, |view, cx| {
            view.apply_reasoning_projection(
                ready(
                    invalid,
                    Some(vega_conversation::types::ReasoningSettingsErrorCode::Conflict),
                ),
                cx,
            )
        });
        window.render_frame(cx);
        let rendered = labels(window);
        assert!(
            rendered
                .iter()
                .any(|label| label.contains("已保存的思考偏好") && label.contains("重新选择"))
        );
        assert!(rendered.iter().any(|label| label.contains("并发修改")));
        let reload = settings.read(cx).reasoning_focuses[0].1.clone();
        window.click(gpui_kit::ElementId::from(&reload), cx);
    })
    .expect("invalid render and recovery");
    assert_eq!(*reloads.borrow(), 1);
    choose(cx, window, &settings, profile(endpoint, model), "Enabled");
    assert_eq!(
        format!(
            "{:?}",
            saves
                .borrow()
                .last()
                .expect("repair event")
                .profile
                .preference
        ),
        "Enabled"
    );
}

#[gpui_kit::test]
fn known_effort_and_uncontrollable_models_do_not_inherit_the_unknown_six_level_fallback(
    cx: &mut TestAppContext,
) {
    for (endpoint, model, count, labels_expected) in [
        (
            "https://proxy.example/v1",
            "step-5-preview",
            5,
            vec!["提供方默认", "低", "中", "高"],
        ),
        (
            "https://api.ant-ling.com/v1",
            "Ring-2.6-1T",
            4,
            vec!["提供方默认", "高", "很高"],
        ),
        (
            "https://integrate.api.nvidia.com/v1",
            "moonshotai/kimi-k2.6",
            2,
            vec!["提供方默认"],
        ),
    ] {
        let (settings, window, _, _) = setup(cx, profile(endpoint, model));
        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(settings.read(cx).reasoning_focuses.len(), count);
            let rendered = labels(window);
            for label in labels_expected {
                assert!(rendered.iter().any(|actual| actual == label));
            }
            assert!(!rendered.iter().any(|label| label == "最高"));
        })
        .expect("known facts render");
    }
}
