//! Bounded persisted task search for the conversation palette boundary.
use rusqlite::{Connection, params};
/// Store-only joined search row, mapped to shared types by conversation.
pub struct TaskRow {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub project_name: String,
    pub project_path: String,
}
/// Search all registered projects, preferring the current project then recency.
pub fn search(
    conn: &Connection,
    current: Option<&str>,
    query: &str,
) -> rusqlite::Result<Vec<TaskRow>> {
    let mut statement=conn.prepare("SELECT t.id,t.project_id,substr(t.title,1,256),substr(p.name,1,128),p.path FROM threads t JOIN projects p ON p.id=t.project_id WHERE t.status='active' AND instr(lower(t.title),lower(?1))>0 ORDER BY (t.project_id IS ?2) DESC,t.updated_at DESC,t.id LIMIT 128")?;
    statement
        .query_map(params![query, current], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                title: row.get(2)?,
                project_name: row.get(3)?,
                project_path: row.get(4)?,
            })
        })?
        .collect()
}
