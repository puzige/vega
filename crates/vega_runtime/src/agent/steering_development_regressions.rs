use super::*;
use crate::skills::{RunBinding, SkillApproval, SkillCatalog, SkillSelection, SkillSource};
use std::sync::atomic::{AtomicBool, Ordering};

struct Source {
    claimed: AtomicBool,
}

impl RuntimeSteeringSource for Source {
    fn take(
        &self,
        _: Vec<SkillSelection>,
    ) -> BoxFuture<'_, Result<Option<RuntimeSteeringInput>, VegaError>> {
        Box::pin(async { Ok(None) })
    }

    fn acknowledge<'a>(
        &'a self,
        _: &'a RuntimeSteeringInput,
    ) -> BoxFuture<'a, Result<(), VegaError>> {
        Box::pin(async move {
            self.claimed.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    fn release<'a>(&'a self, _: &'a RuntimeSteeringInput) -> BoxFuture<'a, Result<(), VegaError>> {
        Box::pin(async move {
            self.claimed.store(false, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct Fixture(std::path::PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn commit_boundary(succeeds: bool) {
    let root = std::env::temp_dir().join(format!("vega-steering-{}", ulid::Ulid::generate()));
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    let fixture = Fixture(root);
    let file = fixture.0.join("alpha/SKILL.md");
    std::fs::write(
        &file,
        "---\nname: alpha\ndescription: Reviewed fixture\n---\nfrozen bytes\n",
    )
    .unwrap();
    let source = SkillSource::imported_approved(&fixture.0, 0).unwrap();
    let candidates = source.discover().unwrap().candidates;
    let selections = candidates
        .iter()
        .map(SkillSelection::from_candidate)
        .collect::<Vec<_>>();
    let approvals = candidates
        .iter()
        .map(|candidate| {
            SkillApproval::reviewed(candidate, "reviewed-source", true, false).unwrap()
        })
        .collect::<Vec<_>>();
    let catalog = SkillCatalog::freeze(candidates, &approvals, true).unwrap();
    let binding = RunBinding::for_catalog("original", "thread", 1, 0, &catalog).unwrap();
    let mut run = SkillRun::new_bound(catalog, true, binding).unwrap();
    assert_eq!(
        run.load_explicit(&selections[0], |_| true).receipt.status,
        "loaded"
    );
    let before = run.export_snapshot().unwrap();
    let config = RuntimeSkillConfig {
        run: Arc::new(Mutex::new(run)),
        explicit: Vec::new(),
        authority_probe: None,
    };
    std::fs::write(&file, "changed and no longer approved").unwrap();
    let source = Source {
        claimed: AtomicBool::new(false),
    };
    let input = RuntimeSteeringInput {
        original_content: "$alpha".into(),
        user_message_seq: 1,
        id: "pending".into(),
        assistant_message_id: "next".into(),
        message: ChatMessage::new(ChatRole::User, "$alpha"),
        skill_selections: selections,
        skills: None,
    };
    let cancel = CancellationToken::new();
    let staged = accept_steering(
        &source,
        &input,
        Some(&config),
        "Follow the user request",
        &[ChatMessage::new(
            ChatRole::System,
            "Follow the user request",
        )],
        &[],
        None,
        InputAccounting {
            anchor: None,
            model: "development-model",
            reasoning: None,
            max_tokens: None,
            revision: 0,
        },
        &cancel,
    )
    .await
    .unwrap();
    assert!(source.claimed.load(Ordering::SeqCst));
    assert_eq!(
        config.run.lock().unwrap().export_snapshot().unwrap(),
        before
    );
    let RuntimeEvent::SteeringAccepted(accepted) = &staged.1 else {
        panic!("single acceptance event required")
    };
    let skills = accepted.skills.as_ref().unwrap();
    assert_eq!(skills.binding.run_id(), "next");
    assert!(skills.audits.is_empty());
    assert_eq!(
        staged.0.as_ref().unwrap().historical_bodies()[0].1.trim(),
        "frozen bytes"
    );
    let persisted = Arc::new(AtomicBool::new(false));
    let observed = persisted.clone();
    let live = config.run.clone();
    let expected = before.clone();
    let mut sink = move |event| {
        let observed = observed.clone();
        let live = live.clone();
        let expected = expected.clone();
        async move {
            assert!(matches!(event, RuntimeEvent::SteeringAccepted(_)));
            assert_eq!(live.lock().unwrap().export_snapshot().unwrap(), expected);
            if succeeds {
                observed.store(true, Ordering::SeqCst);
                Ok(())
            } else {
                Err(skill_runtime_error())
            }
        }
    };
    let mut events = Vec::new();
    let result = commit_steering(
        &source,
        &input,
        Some(&config),
        staged,
        &mut events,
        &mut sink,
    )
    .await;
    let live = config.run.lock().unwrap();
    if succeeds {
        assert!(result.is_ok());
        assert!(persisted.load(Ordering::SeqCst));
        assert_eq!(events.len(), 1);
        assert_eq!(live.binding().unwrap().run_id(), "next");
        assert_eq!(live.historical_bodies()[0].1.trim(), "frozen bytes");
    } else {
        assert!(result.is_err());
        assert!(!persisted.load(Ordering::SeqCst));
        assert!(events.is_empty());
        assert!(!source.claimed.load(Ordering::SeqCst));
        assert_eq!(live.export_snapshot().unwrap(), before);
        assert_eq!(live.binding().unwrap().run_id(), "original");
    }
}

#[tokio::test]
async fn failed_acceptance_persistence_releases_claim_and_preserves_live_frozen_run() {
    commit_boundary(false).await;
}

#[tokio::test]
async fn successful_acceptance_swaps_frozen_run_only_after_persistence_ack() {
    commit_boundary(true).await;
}
