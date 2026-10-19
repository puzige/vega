use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Context, Render, TestAppContext, WindowHandle};
use std::cell::RefCell;
use std::rc::Rc;

struct ReasoningHarness {
    settings: Entity<SettingsView>,
}

impl Render for ReasoningHarness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        div()
            .size_full()
            .on_key_down(|event, window, cx| {
                if event.keystroke.key == "tab" {
                    window.focus_next(cx);
                    cx.stop_propagation();
                }
            })
            .child(settings.update(cx, |view, cx| view.render_reasoning(cx)))
    }
}

fn profile(choice: ReasoningChoice) -> ReasoningProfileProjection {
    ReasoningProfileProjection {
        provider: "proxy".into(),
        model: "step-5-preview".into(),
        protocol: ReasoningProtocol::OpenAiChatCompletions,
        support: ReasoningSupport::Optional,
        efforts: ["low", "medium", "high"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        supports_disabled: false,
        disabled_wire: None,
        replay_profile: ReasoningReplaySetting::StandardChat,
        preference: choice,
    }
}

fn ready(
    profile: ReasoningProfileProjection,
    error: Option<ReasoningSettingsErrorCode>,
) -> ReasoningSettingsProjection {
    ReasoningSettingsProjection::Ready {
        generation: 9,
        profiles: vec![profile],
        error,
    }
}

fn setup(
    cx: &mut TestAppContext,
    profile: ReasoningProfileProjection,
) -> (
    Entity<SettingsView>,
    WindowHandle<ReasoningHarness>,
    Rc<RefCell<Vec<ReasoningProfileSaveRequested>>>,
) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let observed = events.clone();
    let settings = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        cx.set_global(SettingsOpen(true));
        let view = cx.new(|cx| {
            let mut view = SettingsView::from_config(AppConfig::default(), None, cx);
            view.section = 2;
            view.model_settings = Some((profile.provider.clone(), profile.model.clone()));
            view.apply_reasoning_projection(ready(profile, None), cx);
            view
        });
        cx.subscribe(&view, move |_, event: &ReasoningProfileSaveRequested, _| {
            observed.borrow_mut().push(event.clone());
        })
        .detach();
        view
    });
    let owned = settings.clone();
    let window = cx.add_window(move |_, _| ReasoningHarness { settings: owned });
    cx.run_until_parked();
    (settings, window, events)
}

fn labels(window: &Window) -> Vec<String> {
    gpui_kit::base::test_support::snapshots(window)
        .iter()
        .filter_map(|snapshot| snapshot.label().map(str::to_string))
        .collect()
}

#[gpui_kit::test]
fn reasoning_settings_render_only_preferences_and_remove_capability_focus_actions(
    cx: &mut TestAppContext,
) {
    let (settings, window, events) = setup(cx, profile(ReasoningChoice::Effort("high".into())));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let rendered = labels(window);
        assert!(rendered.iter().any(|label| label == "当前偏好"));
        for forbidden in [
            "高级协议",
            "收起高级协议",
            "协议模板",
            "协议",
            "支持范围",
            "允许关闭思考",
            "允许档位",
        ] {
            assert!(!rendered.iter().any(|label| label == forbidden));
        }
        let focus_count = settings.read(cx).reasoning_focuses.len();
        assert_eq!(focus_count, 5);
        for option in 0..4 {
            assert!(
                window
                    .find(format!("reasoning-preference-0-option-{option}"))
                    .visible()
            );
        }
        assert!(window.try_find("reasoning-preference-0-option-4").is_none());
    })
    .expect("rendered settings");
    assert!(events.borrow().is_empty());
}

#[gpui_kit::test]
fn every_preference_button_is_reachable_by_keyboard_and_selects_its_own_choice(
    cx: &mut TestAppContext,
) {
    let (settings, window, events) = setup(cx, profile(ReasoningChoice::Effort("high".into())));
    for (option, expected) in [
        ReasoningChoice::ProviderDefault,
        ReasoningChoice::Effort("low".into()),
        ReasoningChoice::Effort("medium".into()),
        ReasoningChoice::Effort("high".into()),
    ]
    .into_iter()
    .enumerate()
    {
        cx.update_window(window.into(), |_, window, cx| {
            settings.update(cx, |view, cx| {
                let mut initial = profile(ReasoningChoice::ProviderDefault);
                if option == 0 {
                    initial.preference = ReasoningChoice::Effort("high".into());
                }
                view.apply_reasoning_projection(ready(initial, None), cx);
            });
            window.render_frame(cx);
            let reload = settings.read(cx).reasoning_focuses[0].1.clone();
            reload.focus(window, cx);
            for _ in 0..=option {
                window.press("tab", cx);
            }
            let id = format!("reasoning-preference-0-option-{option}");
            assert_eq!(window.find(id).focused(), Some(true));
            window.press(if option % 2 == 0 { "enter" } else { "space" }, cx);
        })
        .expect("keyboard selection");
        let saved = events.borrow().last().expect("typed save event").clone();
        assert_eq!(saved.profile.preference, expected);
        let mut expected_profile = saved.base.clone();
        expected_profile.preference = expected;
        assert_eq!(saved.profile, expected_profile);
        assert_eq!(saved.generation, 9);
    }
    assert_eq!(events.borrow().len(), 4);
}

#[gpui_kit::test]
fn invalid_saved_preference_is_explained_and_can_be_repaired_with_the_mouse(
    cx: &mut TestAppContext,
) {
    let (settings, window, events) = setup(cx, profile(ReasoningChoice::Disabled));
    cx.update_window(window.into(), |_, window, cx| {
        window.render_frame(cx);
        let rendered = labels(window);
        assert!(
            rendered
                .iter()
                .any(|label| label.contains("偏好") && label.contains("选择"))
        );
        assert!(!rendered.iter().any(|label| label.contains("模板")));
        let ReasoningSettingsProjection::Ready { profiles, .. } = &settings.read(cx).reasoning
        else {
            panic!("ready repairable profile");
        };
        assert_eq!(profiles[0].preference, ReasoningChoice::Disabled);
        window.click("reasoning-preference-0-option-0", cx);
    })
    .expect("mouse repair");
    let saved = events.borrow().last().expect("repair save").clone();
    assert_eq!(saved.base.preference, ReasoningChoice::Disabled);
    assert_eq!(saved.profile.preference, ReasoningChoice::ProviderDefault);
}

#[gpui_kit::test]
fn reload_and_conflict_feedback_remain_visible_across_model_navigation(cx: &mut TestAppContext) {
    let (settings, window, events) = setup(cx, profile(ReasoningChoice::Effort("high".into())));
    let reloads = Rc::new(RefCell::new(0));
    let observed = reloads.clone();
    cx.update(|cx| {
        cx.subscribe(&settings, move |_, _: &ReasoningReloadRequested, _| {
            *observed.borrow_mut() += 1;
        })
        .detach();
    });
    cx.update_window(window.into(), |_, window, cx| {
        settings.update(cx, |view, cx| {
            view.request_settings_section(0, cx);
            view.model_settings = Some(("proxy".into(), "step-5-preview".into()));
            view.section = 2;
            view.apply_reasoning_projection(
                ready(
                    profile(ReasoningChoice::Effort("high".into())),
                    Some(ReasoningSettingsErrorCode::Conflict),
                ),
                cx,
            );
        });
        window.render_frame(cx);
        assert!(
            labels(window)
                .iter()
                .any(|label| label.contains("并发修改"))
        );
        let reload = settings.read(cx).reasoning_focuses[0].1.clone();
        window.click(gpui_kit::ElementId::from(reload), cx);
    })
    .expect("reload after navigation");
    assert_eq!(*reloads.borrow(), 1);
    assert!(events.borrow().is_empty());
}
