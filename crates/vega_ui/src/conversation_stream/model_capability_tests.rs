use super::*;
use gpui_kit::{AppContext, TestAppContext};
use vega_runtime::model_compat::{
    ModelApi, ModelCapabilityOverride, ModelIdentity, ModelRoute, ResolvedModel,
    lookup_model_definition, resolve_model,
};

fn thread(model: &str) -> Thread {
    Thread {
        parent_thread_id: None,
        id: "thread".into(),
        backend: TaskBackend::Native,
        project_id: String::new(),
        title: "Fixture".into(),
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

fn effective(api: ModelApi, name: &str, efforts: Option<Vec<String>>) -> ResolvedModel {
    let definition = lookup_model_definition(
        &ModelRoute::new(api, "https://unknown.invalid/v1").unwrap(),
        &ModelIdentity {
            provider: "custom".into(),
            model: name.into(),
        },
    )
    .unwrap();
    let patch = efforts.map(|allowed_efforts| ModelCapabilityOverride {
        identity: definition.identity.clone(),
        allowed_efforts: Some(allowed_efforts),
        image_input: None,
    });
    resolve_model(&definition, patch.as_ref()).unwrap()
}

fn stream(
    cx: &mut TestAppContext,
    model: &ResolvedModel,
    preference: ReasoningChoice,
) -> Entity<ConversationStream> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(vega_theme::Theme::light());
        let entity = cx.new(|cx| ConversationStream::new(thread(&model.identity.model), cx));
        entity.update(cx, |stream, cx| {
            stream.apply_reasoning_profile(
                ReasoningProfileProjection::from_resolved_model(model, preference),
                cx,
            )
        });
        entity
    })
}

#[gpui_kit::test]
fn composer_allows_only_effective_subset_and_preserves_actual_wire_choice(cx: &mut TestAppContext) {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let model = effective(api, "future", Some(vec!["low".into(), "high".into()]));
        let stream = stream(cx, &model, ReasoningChoice::ProviderDefault);
        let slider = cx.update(|cx| stream.read(cx).thinking_slider.clone());
        cx.update(|cx| {
            assert_eq!(slider.read(cx).tiers(), ["low", "high"]);
            assert!(slider.update(cx, |slider, cx| slider.select_index(2, cx)));
        });
        cx.run_until_parked();
        let (reasoning, snapshot) = cx.update(|cx| {
            (
                stream.read(cx).frozen_reasoning_for_submit().unwrap(),
                stream.read(cx).model_snapshot_for_submit().unwrap(),
            )
        });
        assert_eq!(
            reasoning.as_ref().unwrap().choice,
            ReasoningChoice::Effort("high".into())
        );
        assert_eq!(snapshot.as_deref(), Some(&model));
        let request = vega_runtime::ChatRequest {
            model: "future".into(),
            messages: vec![vega_runtime::ChatMessage::new(
                vega_runtime::ChatRole::User,
                "Question",
            )],
            reasoning,
            model_snapshot: snapshot,
            ..Default::default()
        };
        let body = vega_runtime::test_support::encode_request(
            "https://unknown.invalid/v1",
            &request,
            api == ModelApi::Responses,
        )
        .unwrap();
        assert_eq!(
            body.pointer(if api == ModelApi::Responses {
                "/reasoning/effort"
            } else {
                "/reasoning_effort"
            }),
            Some(&serde_json::json!("high"))
        );
        cx.update(|cx| {
            stream.update(cx, |stream, cx| {
                stream.apply_thinking_tier_selected(
                    &ThinkingTierSelected {
                        model: "future".into(),
                        choice: "medium".into(),
                    },
                    cx,
                )
            })
        });
        assert_eq!(
            cx.update(|cx| stream
                .read(cx)
                .frozen_reasoning_for_submit()
                .unwrap()
                .unwrap()
                .choice),
            ReasoningChoice::Effort("high".into())
        );
    }
}

#[gpui_kit::test]
fn capability_shrink_keeps_invalid_saved_choice_visible_until_real_default_selection(
    cx: &mut TestAppContext,
) {
    let model = effective(ModelApi::ChatCompletions, "future", Some(Vec::new()));
    let stream = stream(cx, &model, ReasoningChoice::Effort("high".into()));
    cx.update(|cx| {
        assert_eq!(stream.read(cx).composer_defaults.thinking, "high");
        assert!(stream.read(cx).frozen_reasoning_for_submit().is_err());
        stream.update(cx, |stream, cx| {
            stream.apply_thinking_tier_selected(
                &ThinkingTierSelected {
                    model: "future".into(),
                    choice: "provider_default".into(),
                },
                cx,
            )
        });
    });
    cx.run_until_parked();
    let (reasoning, snapshot) = cx.update(|cx| {
        (
            stream.read(cx).frozen_reasoning_for_submit().unwrap(),
            stream.read(cx).model_snapshot_for_submit().unwrap(),
        )
    });
    assert_eq!(
        reasoning.as_ref().unwrap().choice,
        ReasoningChoice::ProviderDefault
    );
    let body = vega_runtime::test_support::encode_request(
        "https://unknown.invalid/v1",
        &vega_runtime::ChatRequest {
            model: "future".into(),
            messages: Vec::new(),
            reasoning,
            model_snapshot: snapshot,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert!(body.get("reasoning_effort").is_none());
}

#[gpui_kit::test]
fn unknown_proxy_gpt_and_arbitrary_ids_expose_every_fallback_effort_without_whitelist(
    cx: &mut TestAppContext,
) {
    for name in [
        "gpt-6-luna",
        "gpt-6.1-sol",
        "gpt-6-astra",
        "new-unlisted",
        "step-5-preview",
        "glm-5.3",
    ] {
        let model = effective(ModelApi::ChatCompletions, name, None);
        let stream = stream(cx, &model, ReasoningChoice::ProviderDefault);
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
            cx.update(|cx| {
                assert!(slider.update(cx, |slider, cx| slider.select_index(index + 1, cx)))
            });
            cx.run_until_parked();
            let reasoning = cx.update(|cx| stream.read(cx).frozen_reasoning_for_submit().unwrap());
            let snapshot = cx.update(|cx| stream.read(cx).model_snapshot_for_submit().unwrap());
            let body = vega_runtime::test_support::encode_request(
                "https://unknown.invalid/v1",
                &vega_runtime::ChatRequest {
                    model: name.into(),
                    messages: Vec::new(),
                    reasoning,
                    model_snapshot: snapshot,
                    ..Default::default()
                },
                false,
            )
            .unwrap();
            assert_eq!(body["reasoning_effort"], effort, "{name}");
        }
    }
}

#[gpui_kit::test]
fn model_change_replaces_snapshot_controls_and_rejects_old_model_events(cx: &mut TestAppContext) {
    let first = effective(ModelApi::ChatCompletions, "first", None);
    let stream = stream(cx, &first, ReasoningChoice::Effort("high".into()));
    let second = effective(ModelApi::Responses, "second", Some(vec!["low".into()]));
    cx.update(|cx| {
        stream.update(cx, |stream, cx| {
            stream.composer_defaults.model = "second".into();
            stream.apply_reasoning_profile(
                ReasoningProfileProjection::from_resolved_model(
                    &second,
                    ReasoningChoice::ProviderDefault,
                ),
                cx,
            );
            stream.apply_thinking_tier_selected(
                &ThinkingTierSelected {
                    model: "first".into(),
                    choice: "high".into(),
                },
                cx,
            );
        })
    });
    cx.update(|cx| {
        assert_eq!(stream.read(cx).thinking_slider.read(cx).tiers(), ["low"]);
        assert_eq!(
            stream
                .read(cx)
                .model_snapshot_for_submit()
                .unwrap()
                .as_deref(),
            Some(&second)
        );
        assert_eq!(
            stream
                .read(cx)
                .frozen_reasoning_for_submit()
                .unwrap()
                .unwrap()
                .choice,
            ReasoningChoice::ProviderDefault
        );
    });
}

#[gpui_kit::test]
fn boolean_controls_remain_real_operations_when_allowed_efforts_is_empty(cx: &mut TestAppContext) {
    let route = ModelRoute::new(
        ModelApi::ChatCompletions,
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
    )
    .unwrap();
    let source = lookup_model_definition(
        &route,
        &ModelIdentity {
            provider: "qwen".into(),
            model: "qwen3.6-plus".into(),
        },
    )
    .unwrap();
    let model = resolve_model(
        &source,
        Some(&ModelCapabilityOverride {
            identity: source.identity.clone(),
            allowed_efforts: Some(Vec::new()),
            image_input: None,
        }),
    )
    .unwrap();
    let stream = stream(cx, &model, ReasoningChoice::ProviderDefault);
    let slider = cx.update(|cx| stream.read(cx).thinking_slider.clone());
    cx.update(|cx| {
        assert_eq!(slider.read(cx).tiers(), ["enabled"]);
        assert!(slider.update(cx, |slider, cx| slider.select_index(1, cx)));
    });
    cx.run_until_parked();
    let reasoning = cx.update(|cx| stream.read(cx).frozen_reasoning_for_submit().unwrap());
    let snapshot = cx.update(|cx| stream.read(cx).model_snapshot_for_submit().unwrap());
    let body = vega_runtime::test_support::encode_request(
        "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        &vega_runtime::ChatRequest {
            model: "qwen3.6-plus".into(),
            messages: Vec::new(),
            reasoning,
            model_snapshot: snapshot,
            ..Default::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(body["enable_thinking"], true);
    assert!(body.get("reasoning_effort").is_none());
}
