//! Thread orchestration (A1-02): the entry point the UI uses to create,
//! list, update, and open threads.
//!
//! This layer owns everything that must not leak into the SQL layer
//! ([`vega_store::threads`]): ulid generation for thread ids (T11 ruling),
//! the enum ↔ DDL-string bridge for `mode`/`status`, the config-defaults
//! fallbacks (`model` may be empty until S4; an empty `permission_mode`
//! falls back to the DDL default `confirm`), and the two-row touch on
//! open. Storage failures surface as [`ConversationError`] values.

use vega_store::Store;
use vega_store::threads as store;

use crate::types::{
    ConversationError, CurrentProject, Thread, ThreadMode, ThreadStatus, ThreadUpdate,
};

/// Generates a fresh thread id (ulid).
///
/// T11 ruling: ids are minted in this crate, so the store layer only ever
/// sees opaque strings.
pub fn new_thread_id() -> String {
    // ulid 3.x：构造函数为 generate()（自带当前时间戳 + 随机数）。
    ulid::Ulid::generate().to_string()
}

/// Unix-milliseconds timestamp for `created_at`/`updated_at`.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// Wraps any store-layer failure display into [`ConversationError::Store`].
///
/// Generic over the error type so this crate stays free of a direct
/// `rusqlite` dependency (the SQL types stop at `vega_store`).
fn store_error<E: std::fmt::Display>(error: E) -> ConversationError {
    ConversationError::Store(error.to_string())
}

/// The project new threads attach to while T10's project picker is not
/// wired in: the most recently opened project, if any.
pub fn current_project(store: &Store) -> Result<Option<CurrentProject>, ConversationError> {
    let row = store::latest_project(store.conn()).map_err(store_error)?;
    Ok(row.map(|row| CurrentProject {
        id: row.id,
        name: row.name,
    }))
}

/// Creates a thread in `project_id` from the given config defaults and
/// returns it (not yet opened).
///
/// - `model` is stored as-is; an empty string is allowed until S4 wires a
///   provider (architect's ruling on the T11 card).
/// - an empty `permission_mode` falls back to the DDL default `confirm`;
///   combined with the config template generated on first load, this also
///   covers the missing-config path.
/// - `mode` is always `execute` (DDL default) for this card.
///
/// The id is a fresh ulid minted here; `title` starts empty (T13 owns
/// renaming) and `status`/`pinned`/`unread` start at their DDL defaults.
pub fn create_thread(
    store: &Store,
    project_id: &str,
    model: &str,
    permission_mode: &str,
) -> Result<Thread, ConversationError> {
    // 显式守卫：无项目时给出类型化错误，而不是裸外键失败。
    let exists = store::project_exists(store.conn(), project_id).map_err(store_error)?;
    if !exists {
        return Err(ConversationError::NoProject);
    }
    let permission_mode = if permission_mode.is_empty() {
        // DDL 默认（config 缺失模板同值：confirm）。
        "confirm"
    } else {
        permission_mode
    };
    let now = now_ms();
    let thread = Thread {
        id: new_thread_id(),
        project_id: project_id.to_string(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode: permission_mode.to_string(),
        model: model.to_string(),
        status: ThreadStatus::Active,
        pinned: false,
        unread: false,
        created_at: now,
        updated_at: now,
    };
    store::create(
        store.conn(),
        store::NewThread {
            id: &thread.id,
            project_id: &thread.project_id,
            title: &thread.title,
            mode: thread.mode.as_str(),
            permission_mode: &thread.permission_mode,
            model: &thread.model,
            status: thread.status.as_str(),
            pinned: thread.pinned,
            unread: thread.unread,
            created_at: thread.created_at,
            updated_at: thread.updated_at,
        },
    )
    .map_err(store_error)?;
    Ok(thread)
}

/// Lists a project's threads, most recently updated first.
pub fn list_threads(store: &Store, project_id: &str) -> Result<Vec<Thread>, ConversationError> {
    let rows = store::list_by_project(store.conn(), project_id).map_err(store_error)?;
    rows.iter().map(thread_from_row).collect()
}

/// Applies a partial update (title/status/pinned/unread) to one thread.
///
/// An all-`None` update is a no-op; updating a thread that does not exist
/// reports [`ConversationError::NotFound`].
pub fn update_thread(
    store: &Store,
    thread_id: &str,
    update: &ThreadUpdate,
) -> Result<(), ConversationError> {
    if update.is_empty() {
        return Ok(());
    }
    let updated = store::update(
        store.conn(),
        thread_id,
        update.title.as_deref(),
        update.status.map(ThreadStatus::as_str),
        update.pinned,
        update.unread,
    )
    .map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
}

/// Opens a thread: bumps `threads.updated_at` and the owning project's
/// `last_opened_at` (single transaction in the store layer) and returns the
/// refreshed thread.
pub fn open_thread(store: &Store, thread_id: &str) -> Result<Thread, ConversationError> {
    let row = store::open_thread(store.conn(), thread_id, now_ms())
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    thread_from_row(&row)
}

/// Converts a raw store row into the shared [`Thread`], validating the
/// `mode`/`status` vocabulary.
fn thread_from_row(row: &store::ThreadRow) -> Result<Thread, ConversationError> {
    let mode = ThreadMode::parse(&row.mode)
        .ok_or_else(|| ConversationError::CorruptRow(format!("mode: {}", row.mode)))?;
    let status = ThreadStatus::parse(&row.status)
        .ok_or_else(|| ConversationError::CorruptRow(format!("status: {}", row.status)))?;
    Ok(Thread {
        id: row.id.clone(),
        project_id: row.project_id.clone(),
        title: row.title.clone(),
        mode,
        permission_mode: row.permission_mode.clone(),
        model: row.model.clone(),
        status,
        pinned: row.pinned,
        unread: row.unread,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        create_thread, current_project, list_threads, new_thread_id, open_thread, update_thread,
    };
    use crate::types::{ConversationError, ThreadMode, ThreadStatus, ThreadUpdate};
    use vega_store::Store;
    use vega_store::config::AppConfig;

    /// Creates a migrated store backed by a fresh temporary directory.
    fn open_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        (store, dir)
    }

    /// T11 裁决：测试所需 project 行用裸 SQL 插入（补齐 DDL 必填字段），
    /// 不依赖 T10 的函数。
    fn insert_project(store: &Store, id: &str, name: &str) {
        let path = format!("/tmp/{id}");
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, git_default_branch, created_at, last_opened_at) \
                 VALUES (?1, ?2, ?3, NULL, 0, 0)",
                [id, path.as_str(), name],
            )
            .unwrap();
    }

    #[test]
    fn new_thread_ids_are_ulids() {
        let first = new_thread_id();
        let second = new_thread_id();
        // 可解析为 ulid 且互不相同。
        assert!(ulid::Ulid::from_string(&first).is_ok());
        assert!(ulid::Ulid::from_string(&second).is_ok());
        assert_ne!(first, second);
    }

    #[test]
    fn create_thread_uses_ddl_defaults_and_the_given_config_values() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");

        let thread = create_thread(&store, "p1", "deepseek-chat", "auto").unwrap();
        assert!(ulid::Ulid::from_string(&thread.id).is_ok());
        assert_eq!(thread.project_id, "p1");
        assert_eq!(thread.title, "");
        assert_eq!(thread.mode, ThreadMode::Execute);
        assert_eq!(thread.permission_mode, "auto");
        assert_eq!(thread.model, "deepseek-chat");
        assert_eq!(thread.status, ThreadStatus::Active);
        assert!(!thread.pinned);
        assert!(!thread.unread);
        assert!(thread.created_at > 0);
        assert_eq!(thread.created_at, thread.updated_at);
    }

    #[test]
    fn create_thread_empty_permission_mode_falls_back_to_confirm() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "m", "").unwrap();
        assert_eq!(thread.permission_mode, "confirm");
    }

    #[test]
    fn create_thread_with_missing_config_defaults() {
        // config 缺失时 load() 自动生成模板，其值即 AppConfig::default()
        // （等价性由 vega_store::config::missing_file_creates_default_template
        // 保证）。此测试证明该模板路径的默认值可以直接建线程。
        let defaults = AppConfig::default();
        assert!(defaults.defaults.model.is_empty());
        assert_eq!(defaults.defaults.permission_mode, "confirm");

        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(
            &store,
            "p1",
            &defaults.defaults.model,
            &defaults.defaults.permission_mode,
        )
        .unwrap();
        assert_eq!(thread.model, "");
        assert_eq!(thread.permission_mode, "confirm");
    }

    #[test]
    fn create_thread_without_project_reports_no_project() {
        let (store, _dir) = open_store();
        let error = create_thread(&store, "missing", "", "confirm").unwrap_err();
        assert!(matches!(error, ConversationError::NoProject));
    }

    #[test]
    fn list_threads_orders_by_updated_at_desc() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        create_thread(&store, "p1", "", "confirm").unwrap();
        create_thread(&store, "p1", "", "confirm").unwrap();
        create_thread(&store, "p1", "", "confirm").unwrap();
        // 用裸 SQL 拉开 updated_at，避免同毫秒创建导致顺序不稳定。
        store
            .conn()
            .execute("UPDATE threads SET updated_at = 300 WHERE rowid = 1", [])
            .unwrap();
        store
            .conn()
            .execute("UPDATE threads SET updated_at = 100 WHERE rowid = 2", [])
            .unwrap();
        store
            .conn()
            .execute("UPDATE threads SET updated_at = 200 WHERE rowid = 3", [])
            .unwrap();

        let threads = list_threads(&store, "p1").unwrap();
        let updated: Vec<i64> = threads.iter().map(|thread| thread.updated_at).collect();
        assert_eq!(updated, vec![300, 200, 100]);
    }

    #[test]
    fn list_threads_reports_corrupt_rows() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        create_thread(&store, "p1", "", "confirm").unwrap();
        store
            .conn()
            .execute("UPDATE threads SET mode = 'yolo'", [])
            .unwrap();
        let error = list_threads(&store, "p1").unwrap_err();
        assert!(matches!(error, ConversationError::CorruptRow(_)));
    }

    #[test]
    fn open_thread_touches_thread_and_project() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();
        // 把 project 时间戳拨回过去，验证打开动作确实推进它。
        store
            .conn()
            .execute("UPDATE projects SET last_opened_at = 1000", [])
            .unwrap();

        let opened = open_thread(&store, &thread.id).unwrap();
        assert!(opened.updated_at > 1000);
        let last_opened_at: i64 = store
            .conn()
            .query_row(
                "SELECT last_opened_at FROM projects WHERE id = 'p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(last_opened_at > 1000);
    }

    #[test]
    fn open_thread_missing_reports_not_found() {
        let (store, _dir) = open_store();
        let error = open_thread(&store, "missing").unwrap_err();
        assert!(matches!(error, ConversationError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn update_thread_applies_the_field_set() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();

        update_thread(
            &store,
            &thread.id,
            &ThreadUpdate {
                title: Some("重命名".to_string()),
                status: Some(ThreadStatus::Archived),
                pinned: Some(true),
                unread: None,
            },
        )
        .unwrap();
        let reloaded = open_thread(&store, &thread.id).unwrap();
        assert_eq!(reloaded.title, "重命名");
        assert_eq!(reloaded.status, ThreadStatus::Archived);
        assert!(reloaded.pinned);
        assert!(!reloaded.unread);
    }

    #[test]
    fn update_thread_empty_update_is_a_noop() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();
        update_thread(&store, &thread.id, &ThreadUpdate::default()).unwrap();
        let reloaded = open_thread(&store, &thread.id).unwrap();
        assert_eq!(reloaded.title, "");
    }

    #[test]
    fn update_thread_missing_reports_not_found() {
        let (store, _dir) = open_store();
        let error = update_thread(
            &store,
            "missing",
            &ThreadUpdate {
                title: Some("x".to_string()),
                ..ThreadUpdate::default()
            },
        )
        .unwrap_err();
        assert!(matches!(error, ConversationError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn current_project_prefers_the_most_recently_opened() {
        let (store, _dir) = open_store();
        assert!(current_project(&store).unwrap().is_none());
        insert_project(&store, "p1", "alpha");
        insert_project(&store, "p2", "beta");
        store
            .conn()
            .execute(
                "UPDATE projects SET last_opened_at = 500 WHERE id = 'p2'",
                [],
            )
            .unwrap();
        let project = current_project(&store).unwrap().unwrap();
        assert_eq!(project.id, "p2");
        assert_eq!(project.name, "beta");
    }
}
