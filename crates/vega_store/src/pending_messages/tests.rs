use super::*;
use crate::Store;

fn store() -> Store {
    let store = Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    store.conn().execute("INSERT INTO threads (id, model, created_at, updated_at) VALUES ('a', 'test', 1, 1), ('b', 'test', 1, 1)", []).unwrap();
    store
}

fn row(id: &str, generation: &str) -> PendingMessageRow {
    PendingMessageRow {
        id: id.into(),
        thread_id: "a".into(),
        ordinal: 0,
        behavior: "steer".into(),
        generation: generation.into(),
        payload: "original".into(),
        created_at: 1,
    }
}

#[test]
fn issue299_fifo_generation_and_transactional_delivery() {
    let store = store();
    insert(store.conn(), &row("b", "run-a")).unwrap();
    insert(store.conn(), &row("c", "run-b")).unwrap();
    assert_eq!(
        list(store.conn(), "a")
            .unwrap()
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert!(list(store.conn(), "b").unwrap().is_empty());
    assert!(!mark_delivered(store.conn(), "b", "b").unwrap());
    {
        let tx = store.immediate_transaction().unwrap();
        assert!(mark_delivered(&tx, "a", "b").unwrap());
    }
    assert_eq!(list(store.conn(), "a").unwrap().len(), 2);
    assert!(mark_delivered(store.conn(), "a", "b").unwrap());
    assert!(!mark_delivered(store.conn(), "a", "b").unwrap());
    assert_eq!(list(store.conn(), "a").unwrap()[0].id, "c");
    assert!(insert(store.conn(), &row("b", "run-a")).is_err());
}

#[test]
fn issue299_remove_is_bound_to_thread_and_stable_id() {
    let store = store();
    insert(store.conn(), &row("b", "run-a")).unwrap();
    insert(store.conn(), &row("c", "run-a")).unwrap();
    assert!(!remove(store.conn(), "b", "b").unwrap());
    assert!(remove(store.conn(), "a", "b").unwrap());
    assert_eq!(list(store.conn(), "a").unwrap()[0].id, "c");
}
