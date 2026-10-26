use super::*;
use crate::types::{DelegatedPermissionSource, RunCredentialReader};
use vega_runtime::{ChatMessage, ChatRequest, ChatRole, CredentialReader, VegaError};
use vega_store::keystore;

fn confirmed(fixture: &Fixture) {
    crate::permission_contracts::change_thread_permission(
        &fixture.store,
        &fixture.thread,
        0,
        PermissionMode::FullAccess,
        true,
        2,
    )
    .expect("confirmed full access");
}

fn reader(fixture: &Fixture, refs: &[&str], required: CredentialReader) -> RunCredentialReader {
    RunCredentialReader::new(
        fixture.database.clone(),
        fixture.thread.clone(),
        Some(fixture.root.clone()),
        refs.iter().map(|value| (*value).to_owned()).collect(),
        required,
    )
}

fn empty_required() -> CredentialReader {
    Arc::new(|| Ok(Vec::new()))
}

fn safe_store(fixture: &Fixture) {
    keystore::set_key(&fixture.root, "present", "synthetic-owner-value")
        .expect("isolated safe credential document");
}

#[test]
fn optional_bindings_require_verified_execute_full_access_and_keep_present_values_redacted() {
    for mode in ["ask", "plan", "execute"] {
        for permission in [
            PermissionMode::ReadOnly,
            PermissionMode::Confirm,
            PermissionMode::Auto,
            PermissionMode::FullAccess,
        ] {
            let fixture = Fixture::new();
            fixture
                .store
                .conn()
                .execute(
                    "UPDATE threads SET mode=?2 WHERE id=?1",
                    (&fixture.thread, mode),
                )
                .expect("run mode");
            if permission != PermissionMode::Confirm {
                crate::permission_contracts::change_thread_permission(
                    &fixture.store,
                    &fixture.thread,
                    0,
                    permission,
                    true,
                    2,
                )
                .expect("chosen permission");
            }
            safe_store(&fixture);
            let authority = reader(&fixture, &["absent", "present"], empty_required());
            let shared = authority.reader();
            let guard = crate::agent::OwnerCredentialProvider::pre_attempt_guard(shared.clone());
            assert!(shared().is_err());
            assert!(matches!(
                guard(&ChatRequest::default()),
                Err(VegaError::CredentialCheckUnavailable)
            ));
            let config = PersistenceActorConfig::default()
                .with_run_credential_reader(authority.clone())
                .with_credential_exceptions(Some(authority.exception_filter()));
            let before = fixture.rows();
            let result = fixture.prepare_with_config(None, config.clone());
            if mode != "execute" || permission != PermissionMode::FullAccess {
                assert!(
                    matches!(result, Err(ConversationError::Runtime(error)) if matches!(error.as_ref(), VegaError::CredentialCheckUnavailable))
                );
                assert_eq!(fixture.rows(), before);
                assert!(shared().is_err());
                continue;
            }
            let run = result.expect("accepted P2 run");
            assert_eq!(run.permission_fence.run_mode, mode);
            assert_eq!(run.permission_fence.permission_mode, "full_access");
            assert!(guard(&ChatRequest::default()).is_ok());
            let request = ChatRequest {
                messages: vec![ChatMessage::new(ChatRole::User, "synthetic-owner-value")],
                ..Default::default()
            };
            assert!(matches!(
                guard(&request),
                Err(VegaError::CredentialExposureBlocked)
            ));
            let runtime = run
                .request
                .tool_config
                .credential_reader()
                .expect("runtime reader");
            let known = runtime().expect("live runtime values");
            assert!(known.iter().any(|value| value == "synthetic-owner-value"));
            let fingerprint_key = keystore::get_key(&fixture.root, "vega-privacy-fingerprint-v1")
                .expect("new privacy fingerprint key");
            assert!(known.contains(&fingerprint_key));
            assert_ne!(
                vega_runtime::redact_sensitive_credential_text("synthetic-owner-value", &known),
                "synthetic-owner-value"
            );
            let child = crate::agent::subagents::ChildSettings::new(
                &run.request,
                &config,
                &run.permission_fence,
            );
            assert!(
                child
                    .owner_credential_reader
                    .as_ref()
                    .expect("inherited reader")()
                .expect("inherited values")
                .iter()
                .any(|value| value == "synthetic-owner-value")
            );
            assert!(child.run_credential_reader.is_some());
            fixture.store.conn().execute(
                "UPDATE threads SET permission_mode='confirm',full_access_grant=NULL,permission_revision=permission_revision+1 WHERE id=?1",
                [&fixture.thread],
            ).expect("simulate next-run permission change without changing accepted authority");
            assert!(shared().is_ok());
            assert!(runtime().is_ok());
            keystore::set_key(&fixture.root, "present", "synthetic-rotated-value")
                .expect("rotate isolated value");
            let changed = shared().expect("re-read owner values");
            assert!(
                changed
                    .iter()
                    .any(|value| value == "synthetic-rotated-value")
            );
            assert!(!changed.iter().any(|value| value == "synthetic-owner-value"));
        }
    }
}

#[test]
fn required_credentials_win_over_optional_missing_refs_and_invalid_grants_do_not_read() {
    for required_ref in ["absent", "different-required"] {
        let fixture = Fixture::new();
        confirmed(&fixture);
        safe_store(&fixture);
        let root = fixture.root.clone();
        let required = Arc::new(move || {
            keystore::get_key(&root, required_ref)
                .map(|value| vec![value])
                .map_err(|_| ())
        });
        let authority = reader(&fixture, &["absent"], required);
        assert!(
            fixture
                .prepare_with_config(
                    None,
                    PersistenceActorConfig::default().with_run_credential_reader(authority.clone())
                )
                .is_err()
        );
        assert!(fixture.rows().is_empty());
        assert!(authority.reader()().is_err());
        assert!(matches!(
            keystore::lookup_existing_key(&fixture.root, "vega-privacy-fingerprint-v1"),
            Ok(None)
        ));
    }
    let fixture = Fixture::new();
    fixture
        .store
        .conn()
        .execute(
            "UPDATE threads SET permission_mode='full_access' WHERE id=?1",
            [&fixture.thread],
        )
        .expect("missing authorization grant");
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let authority = reader(
        &fixture,
        &[],
        Arc::new(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }),
    );
    assert!(matches!(
        fixture.prepare_with_config(
            None,
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone())
        ),
        Err(ConversationError::InvalidTaskIdentity)
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(fixture.rows().is_empty());
    assert!(!fixture.root.join("credentials").exists());
    assert!(authority.reader()().is_err());
}

#[test]
fn storage_failures_are_not_hidden_by_privacy_initialization_or_optional_policy() {
    use std::os::unix::fs::PermissionsExt;

    for failure in [
        "root",
        "directory",
        "document",
        "malformed",
        "unsafe",
        "limit",
        "symlink",
        "hardlink",
    ] {
        let fixture = Fixture::new();
        confirmed(&fixture);
        let root = if failure == "root" {
            fixture.root.join("absent-root")
        } else {
            fixture.root.clone()
        };
        if failure != "root" && failure != "directory" {
            safe_store(&fixture);
            let path = root.join("credentials/credentials.toml");
            match failure {
                "document" => std::fs::remove_file(&path).expect("remove fixture document"),
                "malformed" => std::fs::write(&path, "keys = [").expect("malformed fixture"),
                "unsafe" => std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
                    .expect("unsafe fixture mode"),
                "limit" => {
                    std::fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).expect("oversize fixture")
                }
                "symlink" => {
                    let retained = root.join("retained.toml");
                    std::fs::rename(&path, &retained).expect("retain original");
                    std::os::unix::fs::symlink(&retained, &path).expect("fixture link");
                }
                "hardlink" => {
                    std::fs::hard_link(&path, root.join("alias.toml")).expect("fixture hard link")
                }
                _ => unreachable!("fixture failure kind"),
            }
        }
        let path = root.join("credentials/credentials.toml");
        let before = std::fs::read(&path).ok();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let authority = RunCredentialReader::new(
            fixture.database.clone(),
            fixture.thread.clone(),
            Some(root.clone()),
            vec!["absent".into()],
            Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }),
        );
        assert!(
            fixture
                .prepare_with_config(
                    None,
                    PersistenceActorConfig::default().with_run_credential_reader(authority.clone())
                )
                .is_err(),
            "{failure}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{failure}");
        assert!(fixture.rows().is_empty());
        assert!(authority.reader()().is_err());
        assert_eq!(std::fs::read(&path).ok(), before, "{failure}");
        if failure == "root" {
            assert!(!root.exists());
        }
        if failure == "directory" {
            assert!(!root.join("credentials").exists());
        }
    }
}

#[test]
fn credential_refresh_writes_outside_the_transaction_and_authority_drift_refuses_activation() {
    for mutation in ["privacy_only", "permission", "model", "backend", "status"] {
        let fixture = Fixture::new();
        confirmed(&fixture);
        safe_store(&fixture);
        let database = fixture.database.clone();
        let thread = fixture.thread.clone();
        let writes = Arc::new(AtomicUsize::new(0));
        let observed = writes.clone();
        let authority = reader(
            &fixture,
            &["absent"],
            Arc::new(move || {
                let store = Store::open(&database).map_err(|_| ())?;
                let tx = store.immediate_transaction().map_err(|_| ())?;
                let statement = match mutation {
                    "privacy_only" => "UPDATE threads SET title='independent writer' WHERE id=?1",
                    "permission" => {
                        "UPDATE threads SET permission_mode='confirm',full_access_grant=NULL,permission_revision=permission_revision+1 WHERE id=?1"
                    }
                    "model" => "UPDATE threads SET model='changed' WHERE id=?1",
                    "backend" => "UPDATE threads SET backend='codex' WHERE id=?1",
                    "status" => "UPDATE threads SET status='archived' WHERE id=?1",
                    _ => return Err(()),
                };
                tx.execute(statement, [&thread]).map_err(|_| ())?;
                tx.commit().map_err(|_| ())?;
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }),
        );
        let result = fixture.prepare_with_config(
            None,
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone()),
        );
        assert_eq!(writes.load(Ordering::SeqCst), 1, "{mutation}");
        if mutation == "privacy_only" {
            let run = result.expect("unrelated writer does not block accepted run");
            assert_eq!(fixture.rows().len(), 2);
            assert!(authority.reader()().is_ok());
            assert!(
                keystore::lookup_existing_key(&fixture.root, "vega-privacy-fingerprint-v1")
                    .expect("privacy document")
                    .is_some()
            );
            drop(run);
        } else {
            assert!(
                matches!(result, Err(ConversationError::InvalidTaskIdentity)),
                "{mutation}"
            );
            assert!(fixture.rows().is_empty());
            assert!(authority.reader()().is_err());
        }
    }
}

#[test]
fn interrupted_reset_and_paused_runs_refuse_before_reading_credentials() {
    for blocked in ["interrupted", "pending", "pause"] {
        let fixture = Fixture::new();
        let tail = if blocked == "interrupted" {
            let (tail, _, _) = fixture.interrupt();
            fixture
                .store
                .conn()
                .execute(
                    "DELETE FROM run_permission_fences WHERE message_id=?1",
                    [&tail.message_id],
                )
                .expect("remove saved authority");
            Some(tail)
        } else {
            None
        };
        if blocked == "pending" {
            fixture.begin_reset();
        }
        let owner = (blocked == "pause").then(|| {
            crate::thread_access::acquire(&fixture.store, &fixture.thread)
                .expect("admission acquired before pause")
        });
        let pause = (blocked == "pause")
            .then(|| crate::permission_reset::pause_admission(&fixture.store).expect("pause"));
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let authority = reader(
            &fixture,
            &[],
            Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Vec::new())
            }),
        );
        let before = fixture.rows();
        let mut config =
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone());
        if let Some(owner) = owner {
            config = config.with_thread_admission(owner);
        }
        assert!(matches!(
            fixture.prepare_with_config(tail, config),
            Err(ConversationError::InvalidTaskIdentity)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.rows(), before);
        assert!(!fixture.root.join("credentials").exists());
        assert!(authority.reader()().is_err());
        drop(pause);
    }
}

#[test]
fn delegated_readers_require_the_exact_accepted_parent_and_cannot_rebind_a_new_run() {
    let fixture = Fixture::new();
    confirmed(&fixture);
    safe_store(&fixture);
    let authority = reader(&fixture, &["absent"], empty_required());
    let run = fixture
        .prepare_with_config(
            None,
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone()),
        )
        .expect("accepted parent");
    let child_message = ulid::Ulid::generate().to_string();
    let mut child = run.permission_fence.clone();
    child.thread_id = ulid::Ulid::generate().to_string();
    child.full_access_grant = None;
    child.delegation = Some(DelegatedPermissionSource {
        parent_message_id: run.assistant_message_id.clone(),
        child_message_id: child_message.clone(),
        parent_fence: Box::new(run.permission_fence.clone()),
    });
    assert!(child.is_valid());
    assert!(authority.prepare(&child, &child_message).is_ok());
    assert!(authority.activate(&child, &child_message).is_ok());
    for change in ["parent_message", "parent_revision", "child_message"] {
        let mut changed = child.clone();
        let source = changed.delegation.as_mut().expect("delegated source");
        match change {
            "parent_message" => source.parent_message_id = ulid::Ulid::generate().to_string(),
            "parent_revision" => source.parent_fence.permission_revision += 1,
            "child_message" => source.child_message_id = ulid::Ulid::generate().to_string(),
            _ => unreachable!("mutation"),
        }
        assert!(changed.is_valid());
        assert!(authority.prepare(&changed, &child_message).is_err());
        assert!(authority.activate(&changed, &child_message).is_err());
    }
    let unaccepted = reader(&fixture, &["absent"], empty_required());
    assert!(unaccepted.prepare(&child, &child_message).is_err());
    assert!(unaccepted.activate(&child, &child_message).is_err());
    let fence = run.permission_fence.clone();
    drop(run);
    assert!(
        authority
            .prepare(&fence, &ulid::Ulid::generate().to_string())
            .is_err()
    );
    assert!(
        fixture
            .prepare_with_config(
                None,
                PersistenceActorConfig::default().with_run_credential_reader(authority.clone())
            )
            .is_err()
    );
    assert_eq!(fixture.rows().len(), 2);
    assert!(authority.reader()().is_ok());
}

#[test]
fn empty_binding_sets_do_not_probe_storage_and_live_storage_failure_stays_an_error() {
    let fixture = Fixture::new();
    let authority = RunCredentialReader::new(
        fixture.database.clone(),
        fixture.thread.clone(),
        None,
        Vec::new(),
        empty_required(),
    );
    let run = fixture
        .prepare_with_config(
            None,
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone()),
        )
        .expect("no credential storage needed");
    assert!(authority.reader()().expect("empty values").is_empty());
    assert!(!fixture.root.join("credentials").exists());
    drop(run);
    let fixture = Fixture::new();
    confirmed(&fixture);
    safe_store(&fixture);
    let authority = reader(&fixture, &["absent"], empty_required());
    let run = fixture
        .prepare_with_config(
            None,
            PersistenceActorConfig::default().with_run_credential_reader(authority.clone()),
        )
        .expect("accepted P2");
    std::fs::remove_file(fixture.root.join("credentials/credentials.toml"))
        .expect("remove isolated document during run");
    assert!(authority.reader()().is_err());
    assert!(!fixture.root.join("credentials/credentials.toml").exists());
    assert!(
        run.request
            .tool_config
            .credential_reader()
            .expect("runtime reader")()
        .is_err()
    );
}
