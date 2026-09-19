use super::*;
use vega_conversation::types::{ActiveSkillView, SkillComposerCandidate, SkillUiScope};

fn projection(thread_id: &str) -> SkillComposerProjection {
    SkillComposerProjection {
        thread_id: thread_id.into(),
        consent_generation: 9,
        candidates: vec![SkillComposerCandidate {
            source_id: "reviewed-source".into(),
            name: "reviewer".into(),
            description: "Review changes".into(),
            source_label: "project:reviewed".into(),
            content_sha256: "a".repeat(64),
            scope: SkillUiScope::Project,
            selected: false,
        }],
        pins: Vec::new(),
    }
}

#[gpui_kit::test]
async fn issue74_draft_choice_is_pure_intent_and_duplicate_submit_is_single_flight(
    cx: &mut TestAppContext,
) {
    let (_, stream, _) = open_controller_stream(cx, "skill-draft");
    let events = Arc::new(Mutex::new(Vec::<ComposerSubmitted>::new()));
    let captured = events.clone();
    cx.update(|cx| {
        cx.subscribe(&stream, move |_, event: &ComposerSubmitted, _| {
            captured
                .lock()
                .expect("submitted capture")
                .push(event.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.set_draft_route(true, cx);
        stream.apply_skill_projection(0, Ok(projection("skill-draft")), cx);
        stream
            .input
            .update(cx, |input, cx| input.set_text("review it", cx));
        let candidate = stream.skill_projection.as_ref().unwrap().candidates[0].clone();
        stream.choose_skill_candidate(&candidate, cx);
        let first = stream.skill_intent.clone();
        assert!(first.is_some());
        assert!(!stream.skill_mutation_pending);
        stream.submit_message(cx);
        stream.submit_message(cx);
        assert_eq!(stream.skill_intent, first);
        assert!(stream.composer_submit_pending);
    });
    let submitted = events.lock().expect("submitted capture");
    assert_eq!(submitted.len(), 1);
    assert!(submitted[0].skill_intent.is_some());
}

#[gpui_kit::test]
async fn issue74_loaded_skill_indicator_is_current_run_only_and_stop_is_real(
    cx: &mut TestAppContext,
) {
    let (window, stream, _) = open_controller_stream(cx, "skill-active");
    let mutations = Arc::new(Mutex::new(Vec::<SkillComposerMutationRequested>::new()));
    let captured = mutations.clone();
    let stops = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_stops = stops.clone();
    cx.update(|cx| {
        cx.subscribe(
            &stream,
            move |_, event: &SkillComposerMutationRequested, _| {
                captured
                    .lock()
                    .expect("mutation capture")
                    .push(event.clone());
            },
        )
        .detach();
        cx.subscribe(&stream, move |_, event: &ComposerStopRequested, _| {
            captured_stops
                .lock()
                .expect("stop capture")
                .push(event.thread_id.clone());
        })
        .detach();
    });
    stream.update(cx, |stream, cx| {
        stream.apply_skill_projection(0, Ok(projection("skill-active")), cx);
        stream.apply_event(
            ConversationEvent::MessageStarted {
                message_id: "run-one".into(),
                seq: 1,
            },
            cx,
        );
        stream.actions.running = true;
        stream.apply_event(
            ConversationEvent::SkillActivated {
                message_id: "other-run".into(),
                skill: ActiveSkillView {
                    name: "wrong".into(),
                    source_label: "project:wrong".into(),
                    content_sha256: "b".repeat(64),
                },
            },
            cx,
        );
        assert!(stream.active_skills.is_empty());
        stream.apply_event(
            ConversationEvent::SkillActivated {
                message_id: "run-one".into(),
                skill: ActiveSkillView {
                    name: "reviewer".into(),
                    source_label: "project:reviewed".into(),
                    content_sha256: "a".repeat(64),
                },
            },
            cx,
        );
        assert_eq!(stream.active_skills.len(), 1);
    });
    cx.run_until_parked();
    let mut visual = gpui_kit::VisualTestContext::from_window(window.into(), cx);
    assert!(visual.debug_bounds("active-skill-stop").is_some());
    assert!(visual.debug_bounds("active-skill-disable").is_some());
    stream.update(cx, |stream, cx| {
        stream.disable_active_skill("reviewer".into(), cx);
        assert!(stream.skill_mutation_pending);
        stream.request_composer_stop(cx);
        stream.apply_event(
            ConversationEvent::Interrupted {
                message_id: "run-one".into(),
            },
            cx,
        );
        assert!(stream.active_skills.is_empty());
    });
    let mutations = mutations.lock().expect("mutation capture");
    assert_eq!(mutations.len(), 1);
    assert_eq!(stops.lock().expect("stop capture").len(), 1);
    assert!(matches!(
        &mutations[0].mutation,
        SkillComposerMutation::DisableFuture { name, source_label, content_sha256 }
            if name == "reviewer" && source_label == "project:reviewed" && content_sha256 == &"a".repeat(64)
    ));
}
