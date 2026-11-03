use crate::types::{PendingMessage, PendingMessageBehavior, SkillMention, SkillSelectionIntent};
use crate::{SkillSettingsError, SkillSettingsService, pending_messages};
use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use vega_runtime::RuntimeSteeringSource;
use vega_runtime::skills::{MAX_RUN_ACTIVATIONS, SkillSelection, SkillSource};
use vega_store::{Store, skills};

fn intent(source: &str) -> SkillSelectionIntent {
    SkillSelectionIntent {
        source_id: source.into(),
        name: "alpha".into(),
        content_sha256: "a".repeat(64),
        expected_consent_generation: 7,
    }
}

fn mention(range: Range<usize>, text: &str, intent: SkillSelectionIntent) -> SkillMention {
    SkillMention {
        range,
        text: text.into(),
        intent,
    }
}

#[test]
fn exact_bound_mentions_deduplicate_but_plain_text_does_not_select() {
    let choice = intent("source-one");
    let text = "$alpha $alpha";
    let first = mention(0..6, "$alpha", choice.clone());
    let second = mention(7..13, "$alpha", choice.clone());
    assert_eq!(
        SkillMention::selections(text, &[first.clone(), second.clone()]),
        Some(vec![choice.clone()])
    );
    assert_eq!(
        SkillMention::selections(text, &[second, first]),
        Some(vec![choice.clone()])
    );
    assert_eq!(
        SkillMention::selections(text, &[mention(7..13, "$alpha", choice)])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(SkillMention::selections(text, &[]), Some(Vec::new()));
}

#[test]
fn selection_identity_includes_source_hash_and_authority_generation() {
    let original = intent("source-one");
    for alternative in [
        intent("source-two"),
        SkillSelectionIntent {
            content_sha256: "b".repeat(64),
            ..original.clone()
        },
        SkillSelectionIntent {
            expected_consent_generation: 8,
            ..original.clone()
        },
    ] {
        assert_eq!(
            SkillMention::selections(
                "$alpha $alpha",
                &[
                    mention(0..6, "$alpha", original.clone()),
                    mention(7..13, "$alpha", alternative.clone()),
                ]
            ),
            Some(vec![original.clone(), alternative])
        );
    }
}

#[test]
fn malformed_ranges_and_text_mismatches_fail_closed_without_panicking() {
    for (content, range, text) in [
        ("$alpha", 0..7, "$alpha"),
        ("$alpha", Range { start: 5, end: 2 }, "$alpha"),
        ("$alpha", 0..0, ""),
        ("$alpha", usize::MAX..usize::MAX, "$alpha"),
        ("$alpha", 0..6, "$other"),
        ("é$alpha", 1..8, "$alpha"),
        ("$alpha", 0..1, "$alpha"),
    ] {
        assert_eq!(
            SkillMention::selections(content, &[mention(range, text, intent("source"))]),
            None
        );
    }
    assert_eq!(
        SkillMention::selections("é$alpha", &[mention(2..8, "$alpha", intent("source"))]),
        Some(vec![intent("source")])
    );
}

#[test]
fn overlapping_bindings_are_rejected_even_when_each_slice_matches() {
    assert_eq!(
        SkillMention::selections(
            "$alpha",
            &[
                mention(0..6, "$alpha", intent("one")),
                mention(1..6, "alpha", intent("two")),
            ]
        ),
        None
    );
    assert_eq!(
        SkillMention::selections(
            "$alpha",
            &[
                mention(0..6, "$alpha", intent("one")),
                mention(0..6, "$alpha", intent("one")),
            ]
        ),
        None
    );
}

#[test]
fn selection_limit_counts_distinct_intents_not_number_of_mentions() {
    let text = "$alpha ".repeat(MAX_RUN_ACTIVATIONS + 1);
    let mut mentions = (0..=MAX_RUN_ACTIVATIONS)
        .map(|index| {
            mention(
                index * 7..index * 7 + 6,
                "$alpha",
                intent(&format!("source-{index}")),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(SkillMention::selections(&text, &mentions), None);
    for item in &mut mentions {
        item.intent = intent("same-source");
    }
    assert_eq!(
        SkillMention::selections(&text, &mentions),
        Some(vec![intent("same-source")])
    );
}

struct Fixture {
    store: Store,
    root: PathBuf,
    project_id: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("vega-submission-{}", ulid::Ulid::generate()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let store = Store::open(root.join("state.sqlite")).unwrap();
        store.migrate().unwrap();
        let project =
            vega_store::projects::create(store.conn(), root.to_str().unwrap(), "development", None)
                .unwrap();
        vega_store::threads::create(
            store.conn(),
            vega_store::threads::NewThread {
                reasoning_preferences: None,
                id: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                project_id: &project.id,
                title: "development",
                mode: "execute",
                permission_mode: "confirm",
                permission_revision: 0,
                full_access_grant: None,
                model: "development-model",
                status: "active",
                pinned: false,
                unread: false,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        Self {
            store,
            root,
            project_id: project.id,
        }
    }

    fn service(&self) -> SkillSettingsService {
        SkillSettingsService::new(
            self.root.join("state.sqlite"),
            self.root.clone(),
            Some(self.project_id.clone()),
        )
    }

    fn reviewed_skill(&self) -> (SkillMention, SkillSelection) {
        let root = self.root.join(".agents/skills");
        fs::create_dir_all(root.join("alpha")).unwrap();
        self.write_skill("reviewed-body");
        let source = SkillSource::project_approved(&self.root).unwrap().unwrap();
        let candidate = source.discover().unwrap().candidates.remove(0);
        let identity = source.identity();
        let generation = skills::read_settings(self.store.conn())
            .unwrap()
            .consent_generation;
        skills::set_project_settings(self.store.conn(), generation, &self.project_id, true, false)
            .unwrap();
        let generation = skills::read_settings(self.store.conn())
            .unwrap()
            .consent_generation;
        skills::link_source(
            self.store.conn(),
            generation,
            skills::NewSkillSource {
                id: "reviewed-source",
                scope: "project",
                project_id: Some(&self.project_id),
                configured_root: root.to_str().unwrap(),
                canonical_root: identity.canonical_root().to_str().unwrap(),
                root_dev: &identity.device().to_string(),
                root_ino: &identity.inode().to_string(),
                import_order: 0,
                enabled: true,
                automatic: false,
                created_at: 1,
            },
        )
        .unwrap();
        let generation = skills::read_settings(self.store.conn())
            .unwrap()
            .consent_generation;
        skills::approve_skill(
            self.store.conn(),
            generation,
            skills::NewSkillApproval {
                source_id: "reviewed-source",
                name: "alpha",
                approved_sha256: &candidate.sha256,
                source_label: "reviewed-source",
                enabled: true,
                automatic: false,
                reviewed_at: 1,
            },
        )
        .unwrap();
        let generation = skills::read_settings(self.store.conn())
            .unwrap()
            .consent_generation;
        (
            mention(
                0..6,
                "$alpha",
                SkillSelectionIntent {
                    source_id: "reviewed-source".into(),
                    name: "alpha".into(),
                    content_sha256: candidate.sha256.clone(),
                    expected_consent_generation: generation,
                },
            ),
            SkillSelection::from_candidate(&candidate),
        )
    }

    fn write_skill(&self, body: &str) {
        fs::write(
            self.root.join(".agents/skills/alpha/SKILL.md"),
            format!("---\nname: alpha\ndescription: Reviewed fixture\n---\n{body}\n"),
        )
        .unwrap();
    }

    fn enqueue(
        &self,
    ) -> (
        pending_messages::QueueSteeringSource,
        PendingMessage,
        CancellationToken,
    ) {
        self.enqueue_mentions(Vec::new())
    }

    fn enqueue_mentions(
        &self,
        mentions: Vec<SkillMention>,
    ) -> (
        pending_messages::QueueSteeringSource,
        PendingMessage,
        CancellationToken,
    ) {
        let thread = vega_store::threads::find(self.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .unwrap();
        let fence = crate::permission_contracts::capture_fence(&self.store, &thread).unwrap();
        let message = PendingMessage {
            id: "pending".into(),
            thread_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            generation: "generation".into(),
            behavior: PendingMessageBehavior::Steer,
            content: if mentions.is_empty() {
                "Keep this message"
            } else {
                "$alpha"
            }
            .into(),
            images: Vec::new(),
            references: Vec::new(),
            reference_mentions: Vec::new(),
            skill_mentions: mentions,
            permission_fence: Some(fence.clone()),
        };
        pending_messages::enqueue(&self.store, &message).unwrap();
        let admission =
            crate::thread_access::acquire(&self.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let cancel = CancellationToken::new();
        let source = pending_messages::QueueSteeringSource::new(
            self.root.join("state.sqlite"),
            self.root.clone(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            "generation".into(),
            Arc::new(|| Ok(Vec::new())),
            cancel.clone(),
        )
        .with_thread_admission(admission);
        assert!(source.authority_slot().set(fence).is_ok());
        (source, message, cancel)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn insert_streaming_assistant(fixture: &Fixture) {
    vega_store::messages::insert(
        fixture.store.conn(),
        &vega_store::messages::MessageRow {
            id: "active-assistant".into(),
            thread_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            seq: 0,
            role: "assistant".into(),
            kind: "text".into(),
            content: "original response".into(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
}

fn persist_acceptance(
    fixture: &Fixture,
    input: &vega_runtime::RuntimeSteeringInput,
) -> Result<Vec<crate::types::ConversationEvent>, vega_runtime::VegaError> {
    crate::agent::persist_runtime_event(
        &fixture.store,
        &fixture.project_id,
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "active-assistant",
        "development-model",
        false,
        "completed response",
        &mut 0,
        &vega_runtime::RuntimeEvent::SteeringAccepted(input.clone()),
        None,
        None,
    )
}

async fn acceptance_rolls_back(fault: &str) {
    use vega_runtime::skills::{RunBinding, SkillApproval, SkillCatalog, SkillRun};

    let fixture = Fixture::new();
    let (mention, selection) = fixture.reviewed_skill();
    insert_streaming_assistant(&fixture);
    let source = SkillSource::project_approved(&fixture.root)
        .unwrap()
        .unwrap();
    let candidates = source.discover().unwrap().candidates;
    let approvals = candidates
        .iter()
        .map(|candidate| {
            SkillApproval::reviewed(candidate, "reviewed-source", true, false).unwrap()
        })
        .collect::<Vec<_>>();
    let catalog = SkillCatalog::freeze(candidates, &approvals, true).unwrap();
    let settings = skills::read_settings(fixture.store.conn()).unwrap();
    let binding = RunBinding::for_catalog(
        "active-assistant",
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        settings.consent_generation,
        settings.revocation_generation,
        &catalog,
    )
    .unwrap();
    let live = SkillRun::new_bound(catalog, true, binding).unwrap();
    let before = live.export_snapshot().unwrap();
    let binding = live.binding().unwrap();
    skills::save_snapshot(
        fixture.store.conn(),
        skills::NewSkillSnapshot {
            run_id: binding.run_id(),
            thread_id: binding.thread_id(),
            consent_generation: binding.consent_generation(),
            revocation_generation: binding.revocation_generation(),
            catalog_sha256: binding.catalog_sha256(),
            snapshot_sha256: before.sha256(),
            bytes: before.bytes(),
            updated_at: 1,
        },
    )
    .unwrap();
    let (source, original, _) = fixture.enqueue_mentions(vec![mention]);
    let mut input = source.take(vec![selection.clone()]).await.unwrap().unwrap();
    let mut candidate = live.clone();
    let activation = candidate.load_explicit(&selection, |_| true);
    assert_eq!(activation.receipt.status, "loaded");
    assert!(!activation.must_pause);
    candidate.continue_at(&input.assistant_message_id).unwrap();
    input.skills = Some(Box::new(vega_runtime::RuntimeSteeringSkills {
        binding: candidate.binding().unwrap().clone(),
        snapshot: candidate.export_snapshot().unwrap(),
        audits: vec![activation.audit],
    }));
    source.acknowledge(&input).await.unwrap();
    let queued = vega_store::pending_messages::find(
        fixture.store.conn(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        &original.id,
    )
    .unwrap()
    .unwrap();
    match fault {
        "epoch" => {
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE skill_settings SET consent_generation = consent_generation + 1, \
                 revocation_generation = revocation_generation + 1 WHERE id = 1",
                    [],
                )
                .unwrap();
        }
        "fence" => {
            fixture.store.conn().execute(
                "UPDATE threads SET permission_revision = permission_revision + 1 WHERE id = 'thread'",
                [],
            ).unwrap();
        }
        table => fixture
            .store
            .conn()
            .execute_batch(&format!(
                "CREATE TEMP TRIGGER reject_acceptance BEFORE INSERT ON {table} \
             BEGIN SELECT RAISE(ABORT, 'injected acceptance failure'); END;"
            ))
            .unwrap(),
    }
    let error = persist_acceptance(&fixture, &input).unwrap_err();
    if fault.starts_with("skill_") {
        assert!(
            matches!(&error, vega_runtime::VegaError::Store(_)),
            "{error}"
        );
        assert!(
            error.to_string().contains("injected acceptance failure"),
            "{error}"
        );
    }
    let retained = vega_store::pending_messages::find(
        fixture.store.conn(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        &original.id,
    )
    .unwrap()
    .unwrap();
    assert!(retained == queued);
    assert!(!retained.payload.is_empty());
    let recovered = pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].content, original.content);
    assert_eq!(recovered[0].skill_mentions, original.skill_mentions);
    assert!(
        vega_store::messages::find(fixture.store.conn(), &input.id)
            .unwrap()
            .is_none()
    );
    assert!(
        vega_store::messages::find(fixture.store.conn(), &input.assistant_message_id)
            .unwrap()
            .is_none()
    );
    let previous = vega_store::messages::find(fixture.store.conn(), "active-assistant")
        .unwrap()
        .unwrap();
    assert_eq!(previous.status, "streaming");
    assert_eq!(previous.content, "original response");
    let stored: Vec<u8> = fixture
        .store
        .conn()
        .query_row(
            "SELECT bytes FROM skill_run_snapshots WHERE run_id = 'active-assistant'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, before.bytes());
    let next_snapshots: i64 = fixture
        .store
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM skill_run_snapshots WHERE run_id = ?1",
            [&input.assistant_message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(next_snapshots, 0);
    assert!(
        skills::list_activation_audits(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_empty()
    );
    assert_eq!(live.export_snapshot().unwrap(), before);
    source.release(&input).await.unwrap();
    if fault != "fence" {
        assert_eq!(source.ready().await.unwrap(), Some(original.id.clone()));
    }
    if fault.starts_with("skill_") {
        fixture
            .store
            .conn()
            .execute_batch("DROP TRIGGER reject_acceptance;")
            .unwrap();
        let mut retry = source.take(vec![selection]).await.unwrap().unwrap();
        candidate.continue_at(&retry.assistant_message_id).unwrap();
        let mut skills = input.skills.take().unwrap();
        skills.binding = candidate.binding().unwrap().clone();
        skills.snapshot = candidate.export_snapshot().unwrap();
        retry.skills = Some(skills);
        input = retry;
        source.acknowledge(&input).await.unwrap();
        let metadata = persist_acceptance(&fixture, &input).unwrap();
        assert!(metadata.len() >= 2);
        assert!(
            pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .unwrap()
                .is_empty()
        );
        let snapshot =
            skills::load_recoverable_snapshot(fixture.store.conn(), &input.assistant_message_id)
                .unwrap()
                .unwrap();
        assert_eq!(
            snapshot.bytes,
            input.skills.as_ref().unwrap().snapshot.bytes()
        );
        let audits =
            skills::list_activation_audits(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].run_id, input.assistant_message_id);
        assert_eq!(audits[0].origin, "explicit_user");
        assert_eq!(
            vega_store::messages::find(fixture.store.conn(), "active-assistant")
                .unwrap()
                .unwrap()
                .status,
            "done"
        );
        assert_eq!(
            vega_store::messages::find(fixture.store.conn(), &input.assistant_message_id)
                .unwrap()
                .unwrap()
                .status,
            "streaming"
        );
    }
}

#[tokio::test]
async fn stale_skill_epoch_after_validation_rolls_back_whole_acceptance() {
    acceptance_rolls_back("epoch").await;
}

#[tokio::test]
async fn stale_permission_fence_after_claim_rolls_back_whole_acceptance() {
    acceptance_rolls_back("fence").await;
}

#[tokio::test]
async fn snapshot_storage_failure_rolls_back_whole_acceptance() {
    acceptance_rolls_back("skill_run_snapshots").await;
}

#[tokio::test]
async fn audit_storage_failure_rolls_back_snapshot_and_whole_acceptance() {
    acceptance_rolls_back("skill_activation_audits").await;
}

#[test]
fn worker_validation_is_read_only_and_only_frozen_exact_identity_skips_changed_disk() {
    let fixture = Fixture::new();
    let (mention, selection) = fixture.reviewed_skill();
    let admission =
        crate::thread_access::acquire(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let service = fixture.service();
    let settings = skills::read_settings(fixture.store.conn()).unwrap();
    assert_eq!(
        service
            .validate_mentions_with_owner(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "$alpha",
                std::slice::from_ref(&mention),
                &admission,
                &[]
            )
            .unwrap(),
        vec![selection.clone()]
    );
    fixture.write_skill("unreviewed-new-bytes");
    assert_eq!(
        service.validate_mentions_with_owner(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "$alpha",
            std::slice::from_ref(&mention),
            &admission,
            &[]
        ),
        Err(SkillSettingsError::Stale)
    );
    assert_eq!(
        service
            .validate_mentions_with_owner(
                "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "$alpha",
                std::slice::from_ref(&mention),
                &admission,
                std::slice::from_ref(&selection)
            )
            .unwrap(),
        vec![selection]
    );
    let mut stale = mention;
    stale.intent.expected_consent_generation += 1;
    assert_eq!(
        service.validate_mentions_with_owner(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "$alpha",
            &[stale],
            &admission,
            &[]
        ),
        Err(SkillSettingsError::Stale)
    );
    assert_eq!(
        skills::read_settings(fixture.store.conn()).unwrap(),
        settings
    );
    assert!(
        skills::list_thread_pins(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn revoked_generation_rejects_even_an_exact_frozen_selection() {
    let fixture = Fixture::new();
    let (mention, selection) = fixture.reviewed_skill();
    let admission =
        crate::thread_access::acquire(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let settings = skills::read_settings(fixture.store.conn()).unwrap();
    skills::set_project_settings(
        fixture.store.conn(),
        settings.consent_generation,
        &fixture.project_id,
        false,
        false,
    )
    .unwrap();
    assert_eq!(
        fixture.service().validate_mentions_with_owner(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "$alpha",
            &[mention],
            &admission,
            &[selection]
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(
        skills::list_thread_pins(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn failed_steer_ack_keeps_original_queued_and_can_retry_original_input() {
    let fixture = Fixture::new();
    let (source, original, _) = fixture.enqueue();
    let input = source.take(Vec::new()).await.unwrap().unwrap();
    let queued = pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].content, original.content);
    let mut changed = input.clone();
    changed.original_content = "tampered".into();
    let error = source.acknowledge(&changed).await.unwrap_err();
    assert!(
        matches!(&error, vega_runtime::VegaError::RuntimeFailure(detail)
        if detail.stage == vega_runtime::RuntimeFailureStage::PendingAdmission
        && detail.reason == vega_runtime::RuntimeFailureReason::Admission),
        "{error}"
    );
    let retained = pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].content, original.content);
    assert_eq!(retained[0].permission_fence, original.permission_fence);
    assert_eq!(source.ready().await.unwrap(), Some(original.id));
    assert_eq!(
        vega_store::messages::next_seq(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        input.user_message_seq
    );
    source.acknowledge(&input).await.unwrap();
    assert_eq!(source.ready().await.unwrap(), None);
}

#[tokio::test]
async fn cancelled_steer_ack_preserves_dispatchable_message() {
    let fixture = Fixture::new();
    let (source, original, cancel) = fixture.enqueue();
    let input = source.take(Vec::new()).await.unwrap().unwrap();
    let queued = vega_store::pending_messages::next_steer(
        fixture.store.conn(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "generation",
    )
    .unwrap()
    .unwrap();
    cancel.cancel();
    let error = source.acknowledge(&input).await.unwrap_err();
    assert!(
        matches!(&error, vega_runtime::VegaError::RuntimeFailure(detail)
        if detail.stage == vega_runtime::RuntimeFailureStage::PendingAdmission
        && detail.reason == vega_runtime::RuntimeFailureReason::Admission),
        "{error}"
    );
    let retained = vega_store::pending_messages::next_steer(
        fixture.store.conn(),
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "generation",
    )
    .unwrap()
    .unwrap();
    assert!(retained == queued);
    assert_eq!(retained.id, original.id);
    assert_eq!(
        pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()[0].content,
        original.content
    );
}

#[tokio::test]
async fn skill_revocation_between_take_and_ack_preserves_message_and_binding() {
    let fixture = Fixture::new();
    let (mention, selection) = fixture.reviewed_skill();
    let (source, original, _) = fixture.enqueue_mentions(vec![mention]);
    let input = source.take(vec![selection.clone()]).await.unwrap().unwrap();
    assert_eq!(input.skill_selections, vec![selection]);
    let settings = skills::read_settings(fixture.store.conn()).unwrap();
    skills::set_project_settings(
        fixture.store.conn(),
        settings.consent_generation,
        &fixture.project_id,
        false,
        false,
    )
    .unwrap();
    let error = source.acknowledge(&input).await.unwrap_err();
    assert!(
        matches!(&error, vega_runtime::VegaError::Tool { tool, message }
        if tool == "skill" && message.contains("Settings") && message.contains("retry")),
        "{error}"
    );
    let retained = pending_messages::load(&fixture.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].content, original.content);
    assert_eq!(retained[0].skill_mentions, original.skill_mentions);
    assert_eq!(source.ready().await.unwrap(), Some(original.id));
    assert_eq!(
        vega_store::messages::next_seq(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        input.user_message_seq
    );
    assert!(
        skills::list_thread_pins(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn worker_rejects_an_admission_from_another_database() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let (mention, _) = fixture.reviewed_skill();
    let foreign_admission =
        crate::thread_access::acquire(&other.store, "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    assert_eq!(
        fixture.service().validate_mentions_with_owner(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "$alpha",
            &[mention],
            &foreign_admission,
            &[]
        ),
        Err(SkillSettingsError::Stale)
    );
    assert!(
        skills::list_thread_pins(fixture.store.conn(), "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .is_empty()
    );
}
