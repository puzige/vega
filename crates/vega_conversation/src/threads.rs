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
use vega_store::messages as store_messages;
use vega_store::projects as store_projects;
use vega_store::threads as store;

use crate::types::{
    ConversationError, CurrentProject, PermissionMode, Thread, ThreadMode, ThreadStatus,
    ThreadUpdate,
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

/// Returns durable user submissions for Composer Up-history in sequence
/// order. The synthetic approval instruction is a controller capability, not
/// text the user typed, so it is deliberately excluded.
pub fn composer_history(store: &Store, thread_id: &str) -> Result<Vec<String>, ConversationError> {
    const HISTORY_WINDOW: usize = 200;
    let rows =
        store_messages::recent(store.conn(), thread_id, HISTORY_WINDOW).map_err(store_error)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.role == "user" && row.kind == "text" && row.status == "done")
        .filter(|row| row.content != crate::plans::APPROVAL_INSTRUCTION)
        .map(|row| row.content)
        .collect())
}

/// The project new threads attach to by default: the most recently opened
/// project, if any (T12: the sidebar seeds its selected-project cache from
/// this and rewrites it on row click).
pub fn current_project(store: &Store) -> Result<Option<CurrentProject>, ConversationError> {
    let row = store_projects::latest_project(store.conn()).map_err(store_error)?;
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
    let exists = store_projects::project_exists(store.conn(), project_id).map_err(store_error)?;
    if !exists {
        return Err(ConversationError::NoProject);
    }
    let permission_mode = if permission_mode.is_empty() {
        // DDL 默认（config 缺失模板同值：confirm）。
        PermissionMode::Confirm
    } else {
        PermissionMode::parse(permission_mode).ok_or_else(|| {
            ConversationError::CorruptRow(format!("permission_mode: {permission_mode}"))
        })?
    };
    let now = now_ms();
    let thread = Thread {
        id: new_thread_id(),
        project_id: project_id.to_string(),
        title: String::new(),
        mode: ThreadMode::Execute,
        permission_mode,
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
            permission_mode: thread.permission_mode.as_str(),
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

/// Lists a project's threads, most recently updated first, pinned group
/// first. T13 (A1-05) adds the lifecycle filter: `Some(status)` restricts
/// the list (the sidebar main list reads `Active`, the 「已归档」 section
/// reads `Archived`); `None` keeps both.
pub fn list_threads(
    store: &Store,
    project_id: &str,
    status: Option<ThreadStatus>,
) -> Result<Vec<Thread>, ConversationError> {
    let rows = store::list_by_project(store.conn(), project_id, status.map(ThreadStatus::as_str))
        .map_err(store_error)?;
    rows.iter().map(thread_from_row).collect()
}

/// Loads one thread by id; reports [`ConversationError::NotFound`] when the
/// row is missing.
fn get_thread(store: &Store, thread_id: &str) -> Result<Thread, ConversationError> {
    let row = store::find(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or_else(|| ConversationError::NotFound(thread_id.to_string()))?;
    thread_from_row(&row)
}

/// Renames a thread (A1-05) and bumps `updated_at` (rename is thread
/// activity per the DDL semantics), returning the refreshed thread.
pub fn rename_thread(
    store: &Store,
    thread_id: &str,
    title: &str,
) -> Result<Thread, ConversationError> {
    let updated = store::rename(store.conn(), thread_id, title, now_ms()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    get_thread(store, thread_id)
}

/// Switches a thread's lifecycle status (`active` ↔ `archived`, A1-05).
/// `updated_at` is not bumped, so archiving does not reorder the list.
pub fn set_thread_status(
    store: &Store,
    thread_id: &str,
    status: ThreadStatus,
) -> Result<(), ConversationError> {
    let updated =
        store::set_status(store.conn(), thread_id, status.as_str()).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
}

/// Sets a thread's pinned flag (置顶切换, A1-05). `updated_at` is not bumped.
pub fn set_thread_pinned(
    store: &Store,
    thread_id: &str,
    pinned: bool,
) -> Result<(), ConversationError> {
    let updated = store::set_pinned(store.conn(), thread_id, pinned).map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
}

/// Persists a typed Ask/Plan/Execute selection. Execute is rejected while a
/// pending Plan exists; approval owns that transition.
pub fn set_thread_mode(
    store: &Store,
    thread_id: &str,
    mode: ThreadMode,
) -> Result<Thread, ConversationError> {
    let updated =
        store::set_mode(store.conn(), thread_id, mode.as_str(), now_ms()).map_err(store_error)?;
    if updated == 0 {
        let current = get_thread(store, thread_id)?;
        if mode == ThreadMode::Execute {
            let plans =
                store_messages::plans_for_thread(store.conn(), thread_id).map_err(store_error)?;
            if plans
                .iter()
                .any(|plan| plan.plan_status.as_deref() == Some("pending"))
            {
                return Err(ConversationError::PendingPlan);
            }
            if current.mode != ThreadMode::Execute {
                return Err(ConversationError::PendingPlan);
            }
        }
    }
    get_thread(store, thread_id)
}

/// Persists the typed permission mode independently from run mode.
pub fn set_thread_permission_mode(
    store: &Store,
    thread_id: &str,
    mode: PermissionMode,
) -> Result<Thread, ConversationError> {
    let updated = store::set_permission_mode(store.conn(), thread_id, mode.as_str(), now_ms())
        .map_err(store_error)?;
    if updated == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    get_thread(store, thread_id)
}

/// Deletes a thread (A1-05). The store layer removes the thread together
/// with its `messages`/`tool_calls` rows in one transaction (no orphan rows;
/// `token_usage` is kept for cost auditing).
pub fn delete_thread(store: &Store, thread_id: &str) -> Result<(), ConversationError> {
    let deleted = store::delete_thread(store.conn(), thread_id).map_err(store_error)?;
    if deleted == 0 {
        return Err(ConversationError::NotFound(thread_id.to_string()));
    }
    Ok(())
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
    let permission_mode = PermissionMode::parse(&row.permission_mode).ok_or_else(|| {
        ConversationError::CorruptRow(format!("permission_mode: {}", row.permission_mode))
    })?;
    Ok(Thread {
        id: row.id.clone(),
        project_id: row.project_id.clone(),
        title: row.title.clone(),
        mode,
        permission_mode,
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
        composer_history, create_thread, current_project, delete_thread, list_threads,
        new_thread_id, open_thread, rename_thread, set_thread_mode, set_thread_permission_mode,
        set_thread_pinned, set_thread_status, update_thread,
    };
    use crate::types::{ConversationError, PermissionMode, ThreadMode, ThreadStatus, ThreadUpdate};
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
        assert_eq!(thread.permission_mode, PermissionMode::Auto);
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
        assert_eq!(thread.permission_mode, PermissionMode::Confirm);
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
        assert_eq!(thread.permission_mode, PermissionMode::Confirm);
    }

    #[test]
    fn create_and_load_unknown_permission_modes_fail_closed() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let create_error = create_thread(&store, "p1", "m", "yolo").unwrap_err();
        assert!(matches!(create_error, ConversationError::CorruptRow(_)));

        let thread = create_thread(&store, "p1", "m", "confirm").unwrap();
        store
            .conn()
            .execute(
                "UPDATE threads SET permission_mode = 'yolo' WHERE id = ?1",
                [&thread.id],
            )
            .unwrap();
        let load_error = list_threads(&store, "p1", None).unwrap_err();
        assert!(matches!(load_error, ConversationError::CorruptRow(_)));
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

        let threads = list_threads(&store, "p1", None).unwrap();
        let updated: Vec<i64> = threads.iter().map(|thread| thread.updated_at).collect();
        assert_eq!(updated, vec![300, 200, 100]);
    }

    #[test]
    fn list_threads_filters_by_status() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let first = create_thread(&store, "p1", "", "confirm").unwrap();
        let second = create_thread(&store, "p1", "", "confirm").unwrap();
        set_thread_status(&store, &second.id, ThreadStatus::Archived).unwrap();

        let active = list_threads(&store, "p1", Some(ThreadStatus::Active)).unwrap();
        assert_eq!(
            active.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec![first.id.as_str()]
        );
        assert!(active.iter().all(|t| t.status == ThreadStatus::Active));

        let archived = list_threads(&store, "p1", Some(ThreadStatus::Archived)).unwrap();
        assert_eq!(
            archived.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec![second.id.as_str()]
        );
        assert!(archived.iter().all(|t| t.status == ThreadStatus::Archived));

        // None 不过滤：主列表与归档区条数之和。
        assert_eq!(list_threads(&store, "p1", None).unwrap().len(), 2);
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
        let error = list_threads(&store, "p1", None).unwrap_err();
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
    fn rename_thread_updates_title_and_bumps_updated_at() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();
        // 把 updated_at 拨回过去，验证重命名确实推进时间戳（DDL 语义）。
        store
            .conn()
            .execute("UPDATE threads SET updated_at = 1000", [])
            .unwrap();

        let renamed = rename_thread(&store, &thread.id, "重命名").unwrap();
        assert_eq!(renamed.title, "重命名");
        assert!(renamed.updated_at > 1000);
        // 落库值一致。
        assert_eq!(open_thread(&store, &thread.id).unwrap().title, "重命名");
    }

    #[test]
    fn rename_thread_missing_reports_not_found() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let error = rename_thread(&store, "missing", "x").unwrap_err();
        assert!(matches!(error, ConversationError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn set_thread_status_toggles_between_active_and_archived() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();
        store
            .conn()
            .execute("UPDATE threads SET updated_at = 1000", [])
            .unwrap();

        set_thread_status(&store, &thread.id, ThreadStatus::Archived).unwrap();
        // 读取走裸 SQL，避免 open_thread 的触碰副作用干扰断言。
        let (status, updated_at): (String, i64) = store
            .conn()
            .query_row(
                "SELECT status, updated_at FROM threads WHERE id = ?1",
                [&thread.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "archived");
        // 归档不是会话活动：updated_at 不推进。
        assert_eq!(updated_at, 1000);

        set_thread_status(&store, &thread.id, ThreadStatus::Active).unwrap();
        let status: String = store
            .conn()
            .query_row(
                "SELECT status FROM threads WHERE id = ?1",
                [&thread.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
    }

    #[test]
    fn set_thread_status_missing_reports_not_found() {
        let (store, _dir) = open_store();
        let error = set_thread_status(&store, "missing", ThreadStatus::Archived).unwrap_err();
        assert!(matches!(error, ConversationError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn set_thread_pinned_toggles_the_flag() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();

        set_thread_pinned(&store, &thread.id, true).unwrap();
        assert!(open_thread(&store, &thread.id).unwrap().pinned);
        set_thread_pinned(&store, &thread.id, false).unwrap();
        assert!(!open_thread(&store, &thread.id).unwrap().pinned);
    }

    #[test]
    fn set_thread_pinned_missing_reports_not_found() {
        let (store, _dir) = open_store();
        let error = set_thread_pinned(&store, "missing", true).unwrap_err();
        assert!(matches!(error, ConversationError::NotFound(id) if id == "missing"));
    }

    #[test]
    fn delete_thread_removes_the_thread_but_keeps_token_usage() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "", "confirm").unwrap();
        // 裸 SQL 灌入 message / tool_call / token_usage 行（架构师裁决③：
        // 验证删除的事务原子性——messages/tool_calls 无孤儿行）。
        store
            .conn()
            .execute(
                "INSERT INTO messages (id, thread_id, seq, role, content, created_at) \
                 VALUES ('m1', ?1, 1, 'user', 'hello', 1)",
                [&thread.id],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO tool_calls (id, thread_id, message_id, seq, tool, input_json, status, created_at) \
                 VALUES ('tc1', ?1, 'm1', 1, 'bash', '{}', 'success', 2)",
                [&thread.id],
            )
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO token_usage (thread_id, message_id, model, input_tokens, output_tokens, cost_microcents, created_at) \
                 VALUES (?1, 'm1', 'test-model', 10, 20, 3, 3)",
                [&thread.id],
            )
            .unwrap();

        delete_thread(&store, &thread.id).unwrap();
        let count = |table: &str| -> i64 {
            store
                .conn()
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE thread_id = ?1"),
                    [&thread.id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        // thread 行已删除（读取走裸 SQL，避免 open_thread 的触碰副作用）。
        let remaining: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM threads WHERE id = ?1",
                [&thread.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(count("messages"), 0);
        assert_eq!(count("tool_calls"), 0);
        // token_usage 保留作成本审计（卡面要求）。
        assert_eq!(count("token_usage"), 1);
    }

    #[test]
    fn delete_thread_missing_reports_not_found() {
        let (store, _dir) = open_store();
        let error = delete_thread(&store, "missing").unwrap_err();
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

    #[test]
    fn typed_modes_and_permissions_survive_restart() {
        let (store, dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "mock", "confirm").unwrap();
        assert_eq!(
            set_thread_mode(&store, &thread.id, ThreadMode::Ask)
                .unwrap()
                .mode,
            ThreadMode::Ask
        );
        assert_eq!(
            set_thread_permission_mode(&store, &thread.id, PermissionMode::ReadOnly)
                .unwrap()
                .permission_mode,
            PermissionMode::ReadOnly
        );
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        let loaded = open_thread(&reopened, &thread.id).unwrap();
        assert_eq!(loaded.mode, ThreadMode::Ask);
        assert_eq!(loaded.permission_mode, PermissionMode::ReadOnly);
    }

    #[test]
    fn composer_history_is_durable_thread_scoped_and_excludes_approval_capability() {
        let (store, dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "mock", "confirm").unwrap();
        let other = create_thread(&store, "p1", "mock", "confirm").unwrap();
        for (id, owner, seq, content) in [
            ("older", thread.id.as_str(), 1, "older\nmessage"),
            ("other", other.id.as_str(), 1, "foreign"),
            (
                "approval",
                thread.id.as_str(),
                2,
                crate::plans::APPROVAL_INSTRUCTION,
            ),
            ("newer", thread.id.as_str(), 3, "newer message"),
        ] {
            vega_store::messages::insert(
                store.conn(),
                &vega_store::messages::MessageRow {
                    id: id.into(),
                    thread_id: owner.into(),
                    seq,
                    role: "user".into(),
                    kind: "text".into(),
                    content: content.into(),
                    status: "done".into(),
                    created_at: seq,
                    plan_status: None,
                    plan_review_note: None,
                    plan_reviewed_at: None,
                },
            )
            .unwrap();
        }
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        assert_eq!(
            composer_history(&reopened, &thread.id).unwrap(),
            vec!["older\nmessage".to_string(), "newer message".to_string()]
        );
        assert_eq!(
            composer_history(&reopened, &other.id).unwrap(),
            vec!["foreign".to_string()]
        );
    }

    #[test]
    fn execute_mode_rejects_pending_or_corrupt_plan_state() {
        let (store, _dir) = open_store();
        insert_project(&store, "p1", "alpha");
        let thread = create_thread(&store, "p1", "mock", "confirm").unwrap();
        set_thread_mode(&store, &thread.id, ThreadMode::Plan).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status) \
                 VALUES ('plan',?1,1,'assistant','plan','steps','done',0,'pending')",
                [&thread.id],
            )
            .unwrap();
        assert!(matches!(
            set_thread_mode(&store, &thread.id, ThreadMode::Execute),
            Err(ConversationError::PendingPlan)
        ));
        store
            .conn()
            .execute(
                "UPDATE threads SET mode='execute' WHERE id=?1",
                [&thread.id],
            )
            .unwrap();
        assert!(matches!(
            set_thread_mode(&store, &thread.id, ThreadMode::Execute),
            Err(ConversationError::PendingPlan)
        ));
        store
            .conn()
            .execute(
                "UPDATE messages SET plan_review_note='corrupt' WHERE id='plan'",
                [],
            )
            .unwrap();
        assert!(set_thread_mode(&store, &thread.id, ThreadMode::Execute).is_err());
    }
}
