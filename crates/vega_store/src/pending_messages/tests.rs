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
