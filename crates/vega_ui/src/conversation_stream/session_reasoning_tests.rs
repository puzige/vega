use super::*;
use gpui_kit::TestAppContext;
use gpui_kit::gpui;
use vega_conversation::types::{
    ReasoningProtocol, ReasoningReplaySetting, ReasoningSupport, SessionReasoningPreference,
};

fn profile(model: &str, tier: &str) -> ReasoningProfileProjection {
    let mut profile = ReasoningProfileProjection::unknown("provider", model);
    profile.protocol = ReasoningProtocol::OpenAiChatCompletions;
    profile.replay_profile = ReasoningReplaySetting::StandardChat;
    profile.support = ReasoningSupport::Optional;
    profile.efforts = vec!["low".into(), "high".into()];
    profile.preference = ReasoningChoice::Effort(tier.into());
    profile
}

fn preferences(tier: &str) -> Vec<SessionReasoningPreference> {
    vec![SessionReasoningPreference {
        provider: "provider".into(),
        model: "model".into(),
        preference: tier.into(),
    }]
}

fn stream(tier: &str, cx: &mut TestAppContext) -> Entity<ConversationStream> {
    let mut thread = vega_conversation::threads::draft_thread(None, "model", "confirm");
    thread.reasoning_preferences = Some(preferences(tier));
    cx.new(|cx| ConversationStream::new(thread, cx))
}

fn project(stream: &Entity<ConversationStream>, cx: &mut TestAppContext) {
    stream.update(cx, |stream, cx| {
        let preferences = stream.thread.reasoning_preferences.clone().unwrap();
        let selected =
            vega_conversation::session_reasoning::project(profile("model", "high"), &preferences)
                .unwrap();
        stream.apply_reasoning_profile(selected.clone(), cx);
        stream.apply_selector_reasoning_profile(Some(selected), cx);
    });
}

#[gpui::test]
fn isolated_streams_keep_display_and_request_after_reprojection(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let a = stream("low", cx);
    let b = stream("high", cx);
    project(&a, cx);
    project(&b, cx);
    let running = a.read_with(cx, |stream, _| {
        stream.frozen_reasoning_for_submit().unwrap().unwrap()
    });
    a.update(cx, |stream, cx| {
        stream.apply_session_reasoning_preferences(preferences("high"), cx)
    });
    project(&a, cx);
    project(&b, cx);
    assert_eq!(running.choice, ReasoningChoice::Effort("low".into()));
    assert_eq!(
        a.read_with(cx, |stream, _| stream.composer_defaults.thinking.clone()),
        "high"
    );
    assert_eq!(
        b.read_with(cx, |stream, _| stream
            .frozen_reasoning_for_submit()
            .unwrap()
            .unwrap()
            .choice),
        ReasoningChoice::Effort("high".into())
    );
    b.update(cx, |stream, cx| {
        stream.apply_session_reasoning_preferences(preferences("low"), cx)
    });
    project(&b, cx);
    project(&a, cx);
    assert_eq!(
        a.read_with(cx, |stream, _| stream
            .selector_reasoning
            .clone()
            .unwrap()
            .preference),
        ReasoningChoice::Effort("high".into())
    );
    assert_eq!(
        b.read_with(cx, |stream, _| stream.composer_defaults.thinking.clone()),
        "low"
    );
    let thread = a.read_with(cx, |stream, _| stream.thread_snapshot());
    let rebuilt = cx.new(|cx| ConversationStream::new(thread, cx));
    project(&rebuilt, cx);
    assert_eq!(
        rebuilt.read_with(cx, |stream, _| stream
            .frozen_reasoning_for_submit()
            .unwrap()
            .unwrap()
            .choice),
        ReasoningChoice::Effort("high".into())
    );
}

#[gpui::test]
fn late_model_ack_preserves_latest_reasoning_and_staged_choice_is_not_current_run(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let a = stream("low", cx);
    project(&a, cx);
    let old = a.read_with(cx, |stream, _| stream.thread_snapshot());
    let running = a.read_with(cx, |stream, _| {
        stream.frozen_reasoning_for_submit().unwrap().unwrap()
    });
    a.update(cx, |stream, cx| {
        stream.request_model_selection("next", cx);
        let request = stream.model_selection_pending.as_ref().unwrap().0;
        stream.apply_thread_model_staged(&old.id, request, "next", cx);
        stream.apply_selector_reasoning_profile(Some(profile("next", "high")), cx);
        assert_eq!(stream.selector_model(), "next");
        assert_eq!(
            stream
                .frozen_reasoning_for_submit()
                .unwrap()
                .unwrap()
                .choice,
            running.choice
        );
        stream.apply_session_reasoning_preferences(preferences("high"), cx);
        stream.apply_authoritative_model(old.clone(), "model", cx);
        assert_eq!(
            stream.thread.reasoning_preferences,
            Some(preferences("high"))
        );
        stream.request_model_selection("model", cx);
        let request = stream.model_selection_pending.as_ref().unwrap().0;
        stream.apply_thread_model_acknowledged(&old.id, request + 1, "model", old.clone(), cx);
        assert!(stream.model_selection_pending.is_some());
        stream.apply_thread_model_acknowledged(&old.id, request, "model", old.clone(), cx);
        assert_eq!(
            stream.thread.reasoning_preferences,
            Some(preferences("high"))
        );
    });
    project(&a, cx);
    assert_eq!(
        a.read_with(cx, |stream, _| stream
            .frozen_reasoning_for_submit()
            .unwrap()
            .unwrap()
            .choice),
        ReasoningChoice::Effort("high".into())
    );
}

#[gpui::test]
fn uninitialized_and_illegal_session_values_cannot_be_submitted(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let a = stream("low", cx);
    a.update(cx, |stream, cx| {
        stream.thread.reasoning_preferences = None;
        stream.apply_reasoning_profile(profile("model", "low"), cx);
        assert!(stream.frozen_reasoning_for_submit().is_err());
        stream.apply_session_reasoning_preferences(preferences("max"), cx);
        stream.apply_reasoning_profile(profile("model", "max"), cx);
        assert!(stream.frozen_reasoning_for_submit().is_err());
    });
}

#[gpui::test]
fn older_save_ack_does_not_replace_newer_slider_intent(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let a = stream("low", cx);
    project(&a, cx);
    let slider = a.read_with(cx, |stream, _| stream.thinking_slider.clone());
    assert!(slider.update(cx, |slider, cx| slider.select_index(1, cx)));
    let high = a.read_with(cx, |stream, _| {
        let mut defaults = stream.composer_defaults.clone();
        defaults.model = "model".into();
        defaults.thinking = "high".into();
        ComposerDefaultsRequested {
            thread_id: stream.thread.id.clone(),
            revision: stream.reasoning_intent_revision,
            defaults,
        }
    });
    assert!(slider.update(cx, |slider, cx| slider.select_index(0, cx)));
    let low = a.read_with(cx, |stream, _| {
        let mut request = high.clone();
        request.revision = stream.reasoning_intent_revision;
        request.defaults.thinking = "low".into();
        request
    });
    a.update(cx, |stream, cx| {
        stream.acknowledge_reasoning_intent(&high, cx);
        stream.apply_session_reasoning_preferences(preferences("high"), cx);
    });
    project(&a, cx);
    assert!(slider.update(cx, |slider, cx| slider.select_index(1, cx)));
    a.update(cx, |stream, cx| {
        stream.acknowledge_reasoning_intent(&low, cx);
        stream.apply_session_reasoning_preferences(preferences("low"), cx);
    });
    project(&a, cx);
    assert_eq!(
        a.read_with(cx, |stream, _| stream
            .selector_reasoning
            .as_ref()
            .unwrap()
            .preference
            .clone()),
        ReasoningChoice::Effort("high".into())
    );
    assert!(!slider.update(cx, |slider, cx| slider.select_index(1, cx)));
}

#[gpui::test]
fn unsupported_old_choice_has_no_false_selection_and_legal_tier_can_be_saved(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| cx.set_global(vega_theme::Theme::light()));
    let a = stream("high", cx);
    a.update(cx, |stream, cx| {
        let mut profile = profile("model", "high");
        profile.efforts = vec!["low".into()];
        stream.apply_reasoning_profile(profile.clone(), cx);
        stream.apply_selector_reasoning_profile(Some(profile), cx);
        assert!(stream.frozen_reasoning_for_submit().is_err());
    });
    let slider = a.read_with(cx, |stream, _| stream.thinking_slider.clone());
    assert!(slider.update(cx, |slider, cx| slider.select_index(0, cx)));
    assert_eq!(
        a.read_with(cx, |stream, _| stream
            .selector_reasoning
            .as_ref()
            .unwrap()
            .preference
            .clone()),
        ReasoningChoice::Effort("low".into())
    );
    a.update(cx, |stream, cx| {
        stream.apply_session_reasoning_preferences(preferences("low"), cx);
        stream.apply_reasoning_profile(profile("model", "low"), cx);
        assert_eq!(
            stream
                .frozen_reasoning_for_submit()
                .unwrap()
                .unwrap()
                .choice,
            ReasoningChoice::Effort("low".into())
        );
    });
}
