use super::*;
use gpui_kit::TestAppContext;
use vega_conversation::types::{ReasoningProtocol, ReasoningSupport, TaskBackend, ThreadStatus};

fn thread() -> Thread {
    Thread {
        parent_thread_id: None,
        forked_from_thread_id: None,
        forked_from_message_id: None,
        id: "thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::Confirm,
        permission_revision: 0,
        full_access_grant: None,
        default_permission: None,
        model: "a".into(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 0,
        updated_at: 0,
    }
}

fn profile(model: &str, effort: &str) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown("provider", model);
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec![effort.into(), "high".into()];
    profile.preference = ReasoningChoice::Effort(effort.into());
    profile
}

#[gpui_kit::test]
fn staged_menu_keeps_current_submission_and_routes_edits_to_selected_model(
    cx: &mut TestAppContext,
) {
    let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let captured = events.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerDefaultsRequested, _| {
            captured.borrow_mut().push(event.defaults.clone());
        })
    });
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(profile("a", "low"), cx);
        stream.apply_selector_reasoning_profile(Some(profile("a", "low")), cx);
        let frozen = stream.frozen_reasoning_for_submit().unwrap();
        stream.request_model_selection("b", cx);
        let request_id = stream.model_selection_pending.as_ref().unwrap().0;
        stream.apply_thread_model_staged("thread", request_id, "b", cx);
        stream.apply_selector_reasoning_profile(Some(profile("b", "medium")), cx);
        stream.open_model_picker_slider(cx);
        assert_eq!(stream.model_picker_level, ModelPickerLevel::Slider);
        assert_eq!(stream.thinking_slider.read(cx).tiers(), &["medium", "high"]);
        assert_eq!(
            stream.thinking_slider.read(cx).choice_name(),
            Some("medium")
        );
        stream.apply_thinking_tier_selected(
            &ThinkingTierSelected {
                model: "b".into(),
                choice: "high".into(),
            },
            cx,
        );
        assert_eq!(
            stream.selector_reasoning.as_ref().unwrap().preference,
            ReasoningChoice::Effort("high".into())
        );
        assert_eq!(stream.frozen_reasoning_for_submit().unwrap(), frozen);
        assert_eq!(stream.displayed_model(), "a");
        assert_eq!(stream.selector_model(), "b");
        stream.model_picker_level = ModelPickerLevel::Closed;
        stream.sync_thinking_slider(cx);
        stream.open_model_picker_slider(cx);
        assert_eq!(stream.thinking_slider.read(cx).choice_name(), Some("high"));
    });
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].model, "b");
    assert_eq!(events.borrow()[0].thinking, "high");
    assert_eq!(events.borrow()[0].reasoning.as_ref().unwrap().model, "b");
    stream.update(cx, |stream, cx| {
        stream.apply_selector_reasoning_profile(Some(profile("b", "medium")), cx);
        assert_eq!(
            stream.thinking_slider.read(cx).choice_name(),
            Some("medium")
        );
        stream.apply_thinking_tier_selected(
            &ThinkingTierSelected {
                model: "b".into(),
                choice: "high".into(),
            },
            cx,
        );
        stream.apply_selector_reasoning_profile(None, cx);
        assert!(!stream.thinking_slider.read(cx).has_tiers());
        assert_eq!(
            stream.composer_defaults.reasoning.as_ref().unwrap().model,
            "a"
        );
    });
    assert_eq!(events.borrow().len(), 2);
}

#[gpui_kit::test]
fn restored_selection_rejects_stale_profiles_and_events_and_clears_missing_tiers(
    cx: &mut TestAppContext,
) {
    let stream = cx.new(|cx| ConversationStream::new(thread(), cx));
    stream.update(cx, |stream, cx| {
        stream.apply_reasoning_profile(profile("a", "low"), cx);
        stream.restore_next_turn_model(Some("b".into()), cx);
        stream.apply_selector_reasoning_profile(Some(profile("b", "medium")), cx);
        stream.restore_next_turn_model(Some("c".into()), cx);
        assert!(!stream.thinking_slider.read(cx).has_tiers());
        stream.apply_selector_reasoning_profile(Some(profile("b", "medium")), cx);
        assert!(!stream.thinking_slider.read(cx).has_tiers());
        stream.apply_selector_reasoning_profile(Some(profile("c", "low")), cx);
        stream.apply_thinking_tier_selected(
            &ThinkingTierSelected {
                model: "b".into(),
                choice: "high".into(),
            },
            cx,
        );
        assert_eq!(
            stream.selector_reasoning.as_ref().unwrap().preference,
            ReasoningChoice::Effort("low".into())
        );
        stream.apply_selector_reasoning_profile(None, cx);
        stream.open_model_picker_slider(cx);
        assert_eq!(stream.model_picker_level, ModelPickerLevel::List);
        assert!(!stream.thinking_slider.read(cx).has_tiers());
        stream.restore_next_turn_model(None, cx);
        stream.apply_selector_reasoning_profile(Some(profile("a", "low")), cx);
        stream.open_model_picker_slider(cx);
        assert_eq!(stream.model_picker_level, ModelPickerLevel::Slider);
        assert_eq!(stream.selector_model(), "a");
    });
}
