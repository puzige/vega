use super::*;

fn timeline(stream: &ConversationStream, cx: &App) -> Vec<String> {
    let tool_id = |card: &Entity<ToolCard>| {
        stream
            .tool_cards
            .iter()
            .find_map(|(id, owned)| (owned == card).then_some(id.clone()))
            .unwrap_or_else(|| "unknown-tool".into())
    };
    stream
        .entries
        .iter()
        .flat_map(|entry| match entry {
            StreamEntry::RunActivity { .. } => Vec::new(),
            StreamEntry::RunActivitySegment { group, segment } => group
                .read(cx)
                .segment_children(*segment)
                .into_iter()
                .map(|child| match child {
                    RunActivityChild::Thinking(_) => "thinking".to_string(),
                    RunActivityChild::Tool(card) => tool_id(&card),
                    RunActivityChild::ToolGroup(group) => group
                        .read(cx)
                        .children()
                        .iter()
                        .map(&tool_id)
                        .collect::<Vec<_>>()
                        .join("+"),
                    RunActivityChild::Artifact(_) => "artifact".to_string(),
                })
                .collect::<Vec<_>>(),
            StreamEntry::UserImages { .. } => vec!["user-images".into()],
            StreamEntry::Assistant { model, failure, .. } => {
                let text: String = model
                    .committed_lines
                    .iter()
                    .chain(model.pending_lines.iter())
                    .flat_map(|line| line.spans.iter())
                    .map(|span| span.text.as_str())
                    .collect();
                if failure.is_some() {
                    vec![format!("failed:{text}")]
                } else {
                    vec![text]
                }
            }
            StreamEntry::Tool { card } => vec![tool_id(card)],
            StreamEntry::ToolGroup { group } => vec![
                group
                    .read(cx)
                    .children()
                    .iter()
                    .map(&tool_id)
                    .collect::<Vec<_>>()
                    .join("+"),
            ],
            StreamEntry::User { .. } => vec!["user".into()],
            StreamEntry::Summary { .. } => vec!["summary".into()],
            StreamEntry::Plan { .. } => vec!["plan".into()],
            StreamEntry::Artifact { .. } => vec!["artifact".into()],
            StreamEntry::Permission { .. } => vec!["permission".into()],
            StreamEntry::ContextCompaction { .. } => vec!["compaction".into()],
            StreamEntry::SkillActivation { .. } => vec!["skill-activation".into()],
            StreamEntry::Screenshot { .. } => vec!["screenshot".into()],
        })
        .collect()
}
