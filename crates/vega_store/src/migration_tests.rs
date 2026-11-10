use super::*;
use rusqlite::OptionalExtension;

fn schema(store: &Store) -> Vec<(String, String, String)> {
    let mut query = store
        .conn()
        .prepare("SELECT type,name,coalesce(sql,'') FROM sqlite_schema ORDER BY type,name")
        .unwrap();
    query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn assert_writer_schema(store: &Store) {
    let version: u32 = store
        .conn()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(usize::try_from(version).unwrap(), MIGRATIONS.len());
    let columns: Vec<String> = store
        .conn()
        .prepare("SELECT name FROM pragma_table_info('shell_operations')")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        columns.iter().filter(|name| *name == "concurrent").count(),
        1
    );
    for migration in &MIGRATIONS[63..] {
        for section in migration.split("CREATE TABLE ").skip(1) {
            let name = section.split_whitespace().next().unwrap();
            assert!(
                schema(store)
                    .iter()
                    .any(|(kind, actual, _)| kind == "table" && actual == name)
            );
        }
        for section in migration.split("CREATE TRIGGER ").skip(1) {
            let name = section.split_whitespace().next().unwrap();
            assert!(
                schema(store)
                    .iter()
                    .any(|(kind, actual, _)| kind == "trigger" && actual == name)
            );
        }
    }
}

#[test]
fn published_shell_schema_upgrades_to_same_writer_schema_as_empty_database() {
    let published = Store::open(":memory:").unwrap();
    for (index, migration) in MIGRATIONS.iter().take(63).enumerate() {
        let target = u32::try_from(index + 1).unwrap();
        if target == 5 {
            published
                .conn()
                .pragma_update(None, "foreign_keys", false)
                .unwrap();
        }
        let tx = published.conn().unchecked_transaction().unwrap();
        tx.execute_batch(migration).unwrap();
        if target == 43 {
            permission_contracts::initialize(&tx).unwrap();
        }
        tx.pragma_update(None, "user_version", target).unwrap();
        tx.commit().unwrap();
        if target == 5 {
            published
                .conn()
                .pragma_update(None, "foreign_keys", true)
                .unwrap();
        }
    }
    assert!(
        published
            .conn()
            .prepare("SELECT concurrent FROM shell_operations")
            .is_ok()
    );
    assert!(
        published
            .conn()
            .prepare("SELECT * FROM persistence_command_receipts")
            .is_err()
    );
    published.migrate().unwrap();
    assert_writer_schema(&published);
    let empty = Store::open(":memory:").unwrap();
    empty.migrate().unwrap();
    assert_writer_schema(&empty);
    assert_eq!(schema(&published), schema(&empty));
    let before = schema(&published);
    published.migrate().unwrap();
    empty.migrate().unwrap();
    assert_eq!(schema(&published), before);
    assert_eq!(schema(&empty), before);
    assert!(
        published
            .conn()
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap()
            .is_none()
    );
}
