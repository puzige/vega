use super::*;

fn assistant_text(stream: &ConversationStream, _cx: &App) -> String {
    stream
        .entries
        .iter()
        .filter_map(|entry| match entry {
            StreamEntry::Assistant { model, .. } => Some(
                model
                    .committed_lines
                    .iter()
                    .chain(model.pending_lines.iter())
                    .flat_map(|line| line.spans.iter())
                    .map(|span| span.text.as_str())
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}
