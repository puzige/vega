use super::*;
use gpui_kit::{AppContext, TestAppContext};

fn profile(endpoint: &str, model: &str) -> ReasoningProfileProjection {
    vega_conversation::types::chat_compatibility(Some(endpoint), model).map_or_else(
        || ReasoningProfileProjection::unknown("fixture", model),
        |facts| ReasoningProfileProjection::from_chat_compatibility("fixture", model, facts),
    )
}

fn thread(model: &str) -> Thread {
    Thread {
        parent_thread_id: None,
        id: "composer-fixture".into(),
        backend: TaskBackend::Native,
        project_id: "".into(),
        title: "fixture".into(),
        mode: ThreadMode::Execute,
        permission_mode: PermissionMode::ReadOnly,
        model: model.into(),
        status: vega_conversation::types::ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: 1,
        updated_at: 1,
    }
}

#[gpui_kit::test]
fn composer_slider_uses_the_same_boolean_facts_and_emits_a_real_enabled_wire(
    cx: &mut TestAppContext,
) {
    for (endpoint, model, key) in [
        (
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "qwen3.6-plus",
            "enable_thinking",
        ),
        ("https://api.together.ai/v1", "Qwen/Qwen3.5-9B", "reasoning"),
        (
            "https://inference.baseten.co/v1",
            "moonshotai/Kimi-K2.6",
            "chat_template_args",
        ),
    ] {
        let profile = profile(endpoint, model);
        let stream = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
            let stream = cx.new(|cx| ConversationStream::new(thread(model), cx));
            stream.update(cx, |stream, cx| {
                stream.apply_reasoning_profile(profile.clone(), cx)
            });
            stream
        });
        let slider = cx.update(|cx| stream.read(cx).thinking_slider.clone());
        cx.update(|cx| {
            assert_eq!(slider.read(cx).tiers(), ["enabled"]);
            assert_eq!(slider.read(cx).dot_count(), 2);
            assert!(slider.update(cx, |slider, cx| slider.select_index(1, cx)));
        });
        cx.run_until_parked();
        let frozen = cx
            .update(|cx| stream.read(cx).frozen_reasoning_for_submit())
            .expect("composer submit")
            .expect("frozen choice");
        assert_eq!(format!("{:?}", frozen.choice), "Enabled");
        let body = vega_runtime::test_support::encode(
            endpoint,
            false,
            &vega_runtime::ChatRequest {
                model: model.into(),
                messages: vec![],
                tools: vec![],
                max_tokens: None,
                reasoning: Some(frozen),
            },
        )
        .expect("encoder");
        let expected = match key {
            "enable_thinking" => serde_json::json!(true),
            "reasoning" => serde_json::json!({"enabled":true}),
            _ => serde_json::json!({"enable_thinking":true}),
        };
        assert_eq!(body[key], expected);
        assert!(body.get("reasoning_effort").is_none());
    }
    assert_eq!(
        super::super::thinking_slider::tier_display_label("enabled"),
        "开启思考"
    );
}

#[gpui_kit::test]
fn unknown_gpt_and_future_composer_selections_keep_each_effort_on_actual_wire(
    cx: &mut TestAppContext,
) {
    for model in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "future-model-not-in-any-catalog",
    ] {
        let profile = profile("https://proxy.example/v1", model);
        let stream = cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_global(vega_theme::Theme::light());
            let stream = cx.new(|cx| ConversationStream::new(thread(model), cx));
            stream.update(cx, |stream, cx| {
                stream.apply_reasoning_profile(profile.clone(), cx)
            });
            stream
        });
        let slider = cx.update(|cx| stream.read(cx).thinking_slider.clone());
        cx.update(|cx| {
            assert_eq!(
                slider.read(cx).tiers(),
                ["minimal", "low", "medium", "high", "xhigh", "max"]
            )
        });
        for (index, effort) in ["minimal", "low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .enumerate()
        {
            cx.update(|cx| assert!(slider.update(cx, |slider, cx| slider.select_index(index, cx))));
            cx.run_until_parked();
            let frozen = cx
                .update(|cx| stream.read(cx).frozen_reasoning_for_submit())
                .expect("composer submit")
                .expect("frozen");
            assert_eq!(frozen.choice, ReasoningChoice::Effort(effort.into()));
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
            .expect("actual wire");
            assert_eq!(body["reasoning_effort"], effort);
        }
    }
}
