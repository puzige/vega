use super::*;

fn terminal(run: &ConversationRun, id: &str) -> crate::types::ToolResult {
    run.events
        .iter()
        .find_map(|event| match event {
            ConversationEvent::ToolCallFinished { call_id, result } if call_id == id => {
                Some(result.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing terminal for {id}: {:?}", run.events))
}
