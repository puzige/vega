use std::error::Error;
use std::fs;

use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use vega_conversation::agent::run_thread_task;
use vega_runtime::{ChatRole, MockProvider, ProviderEvent, ScriptStep, StopReason};
use vega_store::{Store, projects, threads};

const THREAD_ID: &str = "todo-e2e-thread";
const MODEL: &str = "mock-todo-agent";

const SEEDED_TODOS: [(&str, &str); 4] = [
    ("src/lib.rs", "TODO(RUST): persist the cache index"),
    ("scripts/check.py", "TODO(PY): validate command output"),
    ("web/app.ts", "TODO(TS): render the empty state"),
    ("docs/notes.md", "TODO(DOC): add the recovery example"),
];

fn usage(input: u64, output: u64) -> ProviderEvent {
    ProviderEvent::Usage {
        input,
        output,
        cache_read: 0,
        cache_write: 0,
    }
}

fn final_inventory() -> String {
    SEEDED_TODOS
        .iter()
        .map(|(path, marker)| format!("- {path}: {marker}"))
        .fold("TODO inventory:\n".to_string(), |mut inventory, line| {
            inventory.push_str(&line);
            inventory.push('\n');
            inventory
        })
}

fn mock_todo_agent(final_text: &str) -> MockProvider {
    let read_calls = SEEDED_TODOS
        .iter()
        .enumerate()
        .map(|(index, (path, _))| ProviderEvent::ToolUse {
            id: format!("read-{index}"),
            name: "read".to_string(),
            input_json: format!(r#"{{"path":"{path}"}}"#),
        })
        .collect::<Vec<_>>();

    MockProvider::new_rounds(vec![
        vec![ScriptStep::events(vec![
            ProviderEvent::ThinkingDelta("Search the whole repository first.".to_string()),
            ProviderEvent::ToolUse {
                id: "grep-todos".to_string(),
                name: "grep".to_string(),
                input_json: r#"{"pattern":"TODO","path":"."}"#.to_string(),
            },
            usage(40, 8),
            ProviderEvent::Done {
                stop_reason: StopReason::ToolUse,
            },
        ])],
        vec![ScriptStep::events(
            std::iter::once(ProviderEvent::ThinkingDelta(
                "Read every matched file before summarizing.".to_string(),
            ))
            .chain(read_calls)
            .chain([
                usage(75, 12),
                ProviderEvent::Done {
                    stop_reason: StopReason::ToolUse,
                },
            ])
            .collect::<Vec<_>>(),
        )],
        vec![ScriptStep::events(vec![
            ProviderEvent::TextDelta(final_text.to_string()),
            usage(120, 24),
            ProviderEvent::Done {
                stop_reason: StopReason::End,
            },
        ])],
    ])
}

#[tokio::test]
async fn finds_every_seeded_todo_with_real_tools_and_persists_the_run() -> Result<(), Box<dyn Error>>
{
    let fixture = tempdir()?;
    let repo = fixture.path().join("repo");
    let state = fixture.path().join("state");
    fs::create_dir_all(repo.join("src"))?;
    fs::create_dir_all(repo.join("scripts"))?;
    fs::create_dir_all(repo.join("web"))?;
    fs::create_dir_all(repo.join("docs"))?;
    fs::create_dir_all(&state)?;
    fs::write(
        repo.join("src/lib.rs"),
        "pub fn cached() {}\n// TODO(RUST): persist the cache index\n",
    )?;
    fs::write(
        repo.join("scripts/check.py"),
        "def check():\n    pass\n# TODO(PY): validate command output\n",
    )?;
    fs::write(
        repo.join("web/app.ts"),
        "export const ready = true;\n// TODO(TS): render the empty state\n",
    )?;
    fs::write(
        repo.join("docs/notes.md"),
        "# Notes\n\n<!-- TODO(DOC): add the recovery example -->\n",
    )?;

    let store = Store::open(state.join("vega.db"))?;
    store.migrate()?;
    let project = projects::create(
        store.conn(),
        &repo.to_string_lossy(),
        "todo-fixture",
        Some("master"),
    )?;
    threads::create(
        store.conn(),
        threads::NewThread {
            id: THREAD_ID,
            project_id: &project.id,
            title: "Find TODOs",
            mode: "execute",
            permission_mode: "confirm",
            model: MODEL,
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        },
    )?;

    let expected = final_inventory();
    let provider = mock_todo_agent(&expected);
    let tools = vega_tools::Tools::new(&repo)?;
    let run = run_thread_task(
        &store,
        &provider,
        &tools,
        THREAD_ID,
        "Find every TODO in this repository and report its file and text.",
        "Inspect the repository with tools. Read every match before answering.",
        CancellationToken::new(),
    )
    .await?;

    assert_eq!(run.content, expected);
    assert!(!run.interrupted);
    assert!(!run.failed);
    for (path, marker) in SEEDED_TODOS {
        assert!(run.content.contains(path), "missing final path: {path}");
        assert!(
            run.content.contains(marker),
            "missing final TODO marker: {marker}"
        );
    }

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "grep, read, and final answer rounds");
    for request in &requests {
        assert_eq!(request.model, MODEL);
        assert_eq!(
            request
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["read", "glob", "grep"]
        );
    }
    assert_eq!(
        requests[0]
            .messages
            .iter()
            .map(|message| message.role)
            .collect::<Vec<_>>(),
        [ChatRole::System, ChatRole::User]
    );

    let grep_observation = requests[1]
        .messages
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("grep-todos"))
        .ok_or_else(|| std::io::Error::other("grep result missing from observe round"))?;
    for (path, marker) in SEEDED_TODOS {
        assert!(grep_observation.content.contains(path));
        assert!(grep_observation.content.contains(marker));
    }

    let read_request = requests[2]
        .messages
        .iter()
        .find(|message| message.tool_calls.len() == SEEDED_TODOS.len())
        .ok_or_else(|| std::io::Error::other("read calls missing from final request"))?;
    assert!(
        read_request
            .tool_calls
            .iter()
            .all(|call| call.name == "read")
    );
    for (index, (path, marker)) in SEEDED_TODOS.iter().enumerate() {
        let call_id = format!("read-{index}");
        assert!(
            read_request
                .tool_calls
                .iter()
                .any(|call| { call.id == call_id && call.input_json.contains(path) }),
            "missing read request for {path}"
        );
        let observation = requests[2]
            .messages
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(call_id.as_str()))
            .ok_or_else(|| std::io::Error::other(format!("read result missing for {path}")))?;
        assert!(observation.content.contains(marker));
    }

    let mut message_statement = store.conn().prepare(
        "SELECT role, kind, content, status, seq FROM messages \
         WHERE thread_id = ?1 ORDER BY seq",
    )?;
    let messages = message_statement
        .query_map([THREAD_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, "user");
    assert_eq!(messages[0].1, "text");
    assert_eq!(messages[0].3, "done");
    assert_eq!(messages[0].4, 1);
    assert_eq!(messages[1].0, "assistant");
    assert_eq!(messages[1].1, "text");
    assert_eq!(messages[1].2, expected);
    assert_eq!(messages[1].3, "done");
    assert_eq!(messages[1].4, 2);

    let mut tool_statement = store.conn().prepare(
        "SELECT id, message_id, seq, tool, status, approval, output_text, \
                finished_at IS NOT NULL \
         FROM tool_calls WHERE thread_id = ?1 ORDER BY seq",
    )?;
    let persisted_tools = tool_statement
        .query_map([THREAD_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, bool>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(persisted_tools.len(), 1 + SEEDED_TODOS.len());
    for (index, tool) in persisted_tools.iter().enumerate() {
        assert_eq!(tool.1, run.assistant_message_id);
        assert_eq!(tool.2, (index + 1) as i64);
        assert_eq!(tool.4, "success");
        assert_eq!(tool.5, "once");
        assert!(!tool.6.is_empty());
        assert!(tool.7);
    }
    assert_eq!(persisted_tools[0].0, "grep-todos");
    assert_eq!(persisted_tools[0].3, "grep");
    for (index, (_, marker)) in SEEDED_TODOS.iter().enumerate() {
        let persisted = &persisted_tools[index + 1];
        assert_eq!(persisted.0, format!("read-{index}"));
        assert_eq!(persisted.3, "read");
        assert!(persisted.6.contains(marker));
    }

    let mut usage_statement = store.conn().prepare(
        "SELECT message_id, model, input_tokens, output_tokens, cost_microcents \
         FROM token_usage WHERE thread_id = ?1 ORDER BY id",
    )?;
    let usage_rows = usage_statement
        .query_map([THREAD_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(usage_rows.len(), requests.len());
    assert_eq!(
        usage_rows
            .iter()
            .map(|row| (row.2, row.3))
            .collect::<Vec<_>>(),
        [(40, 8), (75, 12), (120, 24)]
    );
    assert!(
        usage_rows
            .iter()
            .all(|row| { row.0 == run.assistant_message_id && row.1 == MODEL && row.4 == 0 })
    );

    let mut table_statement = store.conn().prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let tables = table_statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        tables,
        [
            "messages",
            "permissions",
            "projects",
            "threads",
            "token_usage",
            "tool_calls",
        ]
    );

    Ok(())
}
