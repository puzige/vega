use super::{Outcome, persist_permission_selection, reload_permission_selection};
use vega_conversation::types::{FullAccessScope, PermissionMode, ThreadPermissionError};
use vega_store::{Store, threads};

#[test]
fn one_selection_commits_full_access_for_draft_existing_and_legacy_conversations() {
    for kind in ["draft", "existing", "legacy"] {
        let root = tempfile::tempdir().expect("isolated fixture");
        let path = root.path().join("vega.db");
        let store = Store::open(&path).expect("fixture store");
        store.migrate().expect("schema");
        let source = if kind == "draft" {
            vega_conversation::threads::draft_thread(None, "fixture-model", "confirm")
        } else {
            let thread = vega_conversation::threads::create_standalone_thread(
                &store,
                "fixture-model",
                "confirm",
            )
            .expect("existing conversation");
            if kind == "legacy" {
                store
                    .conn()
                    .execute(
                        "UPDATE threads SET permission_mode='full_access' WHERE id=?1",
                        [thread.id.as_str()],
                    )
                    .expect("legacy mode without grant");
            }
            vega_conversation::threads::get_thread(&store, &thread.id).expect("source")
        };
        assert!(source.full_access_grant.is_none());
        let draft = (kind == "draft").then(|| source.clone());
        let result =
            persist_permission_selection(path, source.clone(), draft, PermissionMode::FullAccess)
                .expect("one menu selection persists without another action");
        let Outcome::Saved(saved) = result else {
            panic!("full access must be durable after selection");
        };
        let durable =
            vega_conversation::threads::get_thread(&store, &source.id).expect("durable result");
        assert_eq!(saved, durable);
        assert_eq!(durable.id, source.id);
        assert_eq!(durable.model, source.model);
        assert_eq!(durable.project_id, source.project_id);
        assert_eq!(durable.permission_mode, PermissionMode::FullAccess);
        assert_eq!(durable.permission_revision, source.permission_revision + 1);
        assert!(durable.default_permission.is_none());
        let state = vega_store::permission_contracts::load_state(store.conn()).expect("authority");
        assert!(
            durable
                .full_access_grant
                .expect("explicit selection grant")
                .verifies(
                    &state.authority_epoch,
                    &FullAccessScope::Thread {
                        thread_id: source.id
                    },
                )
        );
    }
}

#[test]
fn a_mismatched_draft_selection_never_creates_or_grants_a_conversation() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let path = root.path().join("vega.db");
    let store = Store::open(&path).expect("fixture store");
    store.migrate().expect("schema");
    let source = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
    for draft in [
        None,
        Some(vega_conversation::types::Thread {
            model: "changed-model".to_owned(),
            ..source.clone()
        }),
    ] {
        let result = persist_permission_selection(
            path.clone(),
            source.clone(),
            draft,
            PermissionMode::FullAccess,
        );
        assert!(matches!(result, Err(ThreadPermissionError::Conflict)));
        assert!(
            threads::find(store.conn(), &source.id)
                .expect("no partial row")
                .is_none()
        );
    }
}

#[test]
fn a_stale_selection_does_not_overwrite_newer_permissions() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let path = root.path().join("vega.db");
    let store = Store::open(&path).expect("fixture store");
    store.migrate().expect("schema");
    let source = vega_conversation::threads::create_standalone_thread(&store, "fixture", "confirm")
        .expect("source");
    store.conn().execute(
        "UPDATE threads SET permission_mode='readonly', permission_revision=permission_revision+1 WHERE id=?1",
        [source.id.as_str()],
    ).expect("newer selection");
    let expected =
        vega_conversation::threads::get_thread(&store, &source.id).expect("newer source");
    let result = persist_permission_selection(path, source, None, PermissionMode::FullAccess);
    assert!(matches!(result, Err(ThreadPermissionError::Conflict)));
    let actual = vega_conversation::threads::get_thread(&store, &expected.id).expect("unchanged");
    assert_eq!(actual, expected);
    assert!(actual.full_access_grant.is_none());
}

#[test]
fn restricted_draft_selections_remain_unsaved_and_have_no_full_access_grant() {
    let root = tempfile::tempdir().expect("isolated fixture");
    let path = root.path().join("vega.db");
    let store = Store::open(&path).expect("fixture store");
    store.migrate().expect("schema");
    for mode in [
        PermissionMode::ReadOnly,
        PermissionMode::Confirm,
        PermissionMode::Auto,
    ] {
        let source = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
        let result =
            persist_permission_selection(path.clone(), source.clone(), Some(source.clone()), mode)
                .expect("restricted draft selection");
        let Outcome::Draft(saved) = result else {
            panic!("restricted drafts must remain drafts");
        };
        assert_eq!(saved.permission_mode, mode);
        assert_eq!(saved.model, source.model);
        assert!(saved.full_access_grant.is_none());
        assert!(
            threads::find(store.conn(), &source.id)
                .expect("no row")
                .is_none()
        );
    }
}

#[test]
fn reloading_a_selection_recovers_committed_drafts_without_reauthorizing() {
    for committed in [false, true] {
        let root = tempfile::tempdir().expect("isolated fixture");
        let path = root.path().join("vega.db");
        let store = Store::open(&path).expect("fixture store");
        store.migrate().expect("schema");
        let source = vega_conversation::threads::draft_thread(None, "fixture", "confirm");
        let saved = if committed {
            let result = persist_permission_selection(
                path.clone(),
                source.clone(),
                Some(source.clone()),
                PermissionMode::FullAccess,
            )
            .expect("committed selection before unknown acknowledgement");
            let Outcome::Saved(saved) = result else {
                panic!("durable selection");
            };
            Some(saved)
        } else {
            None
        };
        let recovered = reload_permission_selection(path, source.clone(), Some(source.clone()))
            .expect("reload the original intent");
        match (saved, recovered) {
            (Some(expected), Outcome::Saved(actual)) => {
                assert_eq!(actual, expected);
                assert_eq!(
                    vega_conversation::threads::get_thread(&store, &source.id)
                        .expect("no second grant"),
                    expected
                );
            }
            (None, Outcome::Draft(actual)) => {
                assert_eq!(actual, source);
                assert!(
                    threads::find(store.conn(), &source.id)
                        .expect("reload creates no row")
                        .is_none()
                );
            }
            _ => panic!("reload must reflect actual durable state"),
        }
    }
}
