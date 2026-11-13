use super::take_preparation_owner;
use crate::ordinary_submission::OrdinaryPreparationOwner;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
use vega_conversation::types::{OrdinarySubmission, RawSubmission};

#[test]
fn old_preparation_cannot_remove_new_owner() {
    let store = vega_store::Store::open(":memory:").unwrap();
    let make = |thread: &str| {
        let admission =
            vega_store::thread_admission::try_acquire(store.database_identity(), thread)
                .unwrap()
                .unwrap();
        OrdinarySubmission::begin(
            store.database_identity(),
            thread.into(),
            RawSubmission::new("test".into(), vec![], vec![], 1, "test".into(), None, None),
            admission,
        )
        .unwrap()
    };
    let (_first, first_worker) = make("first");
    let (_second, second_worker) = make("second");
    let first = first_worker.attempt_id();
    let second = second_worker.attempt_id();
    let mut owners = HashMap::new();
    owners.insert(
        "thread".to_owned(),
        OrdinaryPreparationOwner {
            attempt: second,
            cancel: CancellationToken::new(),
        },
    );
    assert!(take_preparation_owner(&mut owners, "thread", first).is_none());
    assert_eq!(owners.get("thread").unwrap().attempt, second);
    assert!(take_preparation_owner(&mut owners, "thread", second).is_some());
    assert!(owners.is_empty());
}
