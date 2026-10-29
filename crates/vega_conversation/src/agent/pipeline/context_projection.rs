use super::*;
use std::collections::BTreeMap;
use vega_store::context_projection::{ProjectionFragment, ProjectionPlan, SourceRange};

pub(crate) struct LiveSourceRange<'a> {
    pub range: &'a SourceRange,
    pub messages: &'a [vega_runtime::ChatMessage],
}

pub(crate) fn interpret_projection(
    source: &vega_store::context_compaction::ContextSource,
    plan: &ProjectionPlan,
    summary: &str,
    previous: &BTreeMap<i64, String>,
    current_assistant_id: &str,
) -> Result<Vec<vega_runtime::ChatMessage>, ConversationError> {
    interpret_projection_with_live(source, plan, summary, previous, current_assistant_id, &[])
}

pub(crate) fn interpret_projection_with_live(
    source: &vega_store::context_compaction::ContextSource,
    plan: &ProjectionPlan,
    summary: &str,
    previous: &BTreeMap<i64, String>,
    current_assistant_id: &str,
    live: &[LiveSourceRange<'_>],
) -> Result<Vec<vega_runtime::ChatMessage>, ConversationError> {
    plan.validate(source)
        .map_err(|_| ConversationError::CorruptRow("invalid context projection plan".into()))?;
    for overlay in live {
        if plan.fragments.iter().filter(|fragment| {
            matches!(fragment, ProjectionFragment::Source { range } if range == overlay.range)
        }).count() != 1 || live.iter().filter(|other| other.range == overlay.range).count() != 1 {
            return Err(ConversationError::CorruptRow("invalid live context range".into()));
        }
    }
    let mut messages = Vec::new();
    for fragment in &plan.fragments {
        match fragment {
            ProjectionFragment::Source { range } => {
                let durable = source_range_history(source, plan, range, current_assistant_id)?;
                if let Some(overlay) = live.iter().find(|overlay| overlay.range == range) {
                    let expected = comparable_assistant_segments(&durable)?;
                    let actual = comparable_assistant_segments(overlay.messages)?;
                    if !super::super::compaction::durable_messages_equal(
                        &expected.iter().collect::<Vec<_>>(),
                        &actual.iter().collect::<Vec<_>>(),
                    ) {
                        return Err(ConversationError::CorruptRow(
                            "live context source changed".into(),
                        ));
                    }
                    messages.extend_from_slice(overlay.messages);
                } else {
                    messages.extend(durable);
                }
            }
            ProjectionFragment::Summary => {
                if !plan.omitted.is_empty() {
                    let ranges = serde_json::to_string(&plan.omitted).map_err(|_| {
                        ConversationError::CorruptRow("invalid omitted context ranges".into())
                    })?;
                    messages.push(vega_runtime::ChatMessage::new(
                        vega_runtime::ChatRole::User,
                        format!(
                            "[Historical context omitted — these source ranges are NOT covered by the following summary. Their original records remain in the conversation audit; do not infer their contents or repeat their side effects.]\n{ranges}"
                        ),
                    ));
                }
                let mut summary_message = historical_summary_message(summary);
                summary_message
                    .attach_context_file_references(context_file_references(source, plan)?);
                messages.push(summary_message);
            }
            ProjectionFragment::PreviousSummary { checkpoint_id } => {
                let previous = previous.get(checkpoint_id).ok_or_else(|| {
                    ConversationError::CorruptRow("historical summary reference missing".into())
                })?;
                messages.push(historical_summary_message(previous));
            }
            ProjectionFragment::TaskReference { message_id } => {
                messages.push(historical_task_reference(source, message_id)?);
            }
            ProjectionFragment::HistoricalImages { through_seq } => {
                messages.extend(historical_image_projection(source, *through_seq)?);
            }
        }
    }
    Ok(messages)
}

fn context_file_references(
    source: &vega_store::context_compaction::ContextSource,
    plan: &ProjectionPlan,
) -> Result<Vec<(String, String)>, ConversationError> {
    let mut retained = std::collections::HashSet::new();
    for fragment in &plan.fragments {
        if let ProjectionFragment::Source { range } = fragment {
            let selected = range.project(source).map_err(|_| {
                ConversationError::CorruptRow("invalid context file source range".into())
            })?;
            retained.extend(selected.tool_calls.into_iter().map(|call| call.id));
        }
    }
    let mut calls = source
        .tool_calls
        .iter()
        .filter(|call| {
            matches!(call.tool.as_str(), "read" | "Read")
                && call.status == "success"
                && !retained.contains(&call.id)
        })
        .collect::<Vec<_>>();
    calls.sort_by_key(|call| std::cmp::Reverse(call.seq));
    let mut paths = std::collections::HashSet::new();
    let mut files = Vec::new();
    for call in calls {
        let Ok(input) = serde_json::from_str::<serde_json::Value>(&call.input_json) else {
            continue;
        };
        let Some(path) = input
            .get("path")
            .or_else(|| input.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty() && path.len() <= 4096)
        else {
            continue;
        };
        if paths.insert(path.to_owned()) {
            files.push((call.id.clone(), path.to_owned()));
        }
        if files.len() == 5 {
            break;
        }
    }
    Ok(files)
}

fn source_range_history(
    source: &vega_store::context_compaction::ContextSource,
    plan: &ProjectionPlan,
    range: &SourceRange,
    current_assistant_id: &str,
) -> Result<Vec<vega_runtime::ChatMessage>, ConversationError> {
    let mut selected = range
        .project(source)
        .map_err(|_| ConversationError::CorruptRow("invalid context source range".into()))?;
    if let Some(owner) = plan.source_owner_id.as_deref()
        && owner != current_assistant_id
        && let Some(message) = selected
            .messages
            .iter_mut()
            .find(|message| message.id == owner && message.status == "streaming")
    {
        let calls = selected
            .tool_calls
            .iter()
            .filter(|call| call.message_id == owner)
            .collect::<Vec<_>>();
        if !calls.is_empty()
            && calls.iter().all(|call| {
                matches!(
                    call.status.as_str(),
                    "success" | "failed" | "cancelled" | "rejected"
                ) && call.output_text.is_some()
            })
        {
            message.status = "done".into();
        }
    }
    history_from_context_source(&selected, current_assistant_id, None)
}

pub(crate) fn match_live_ranges<'a>(
    source: &vega_store::context_compaction::ContextSource,
    plan: &'a ProjectionPlan,
    messages: &'a [vega_runtime::ChatMessage],
) -> Result<Vec<LiveSourceRange<'a>>, ConversationError> {
    let mut live = Vec::new();
    let mut cursor = 0;
    for fragment in &plan.fragments {
        let ProjectionFragment::Source { range } = fragment else {
            continue;
        };
        let expected = comparable_assistant_segments(&source_range_history(
            source,
            plan,
            range,
            "__live_projection__",
        )?)?;
        if expected.is_empty() {
            continue;
        }
        let expected_refs = expected.iter().collect::<Vec<_>>();
        let mut matched = None;
        for start in cursor..messages.len() {
            if messages[start].role != expected[0].role {
                continue;
            }
            for end in start + 1..=messages.len() {
                let actual = comparable_assistant_segments(&messages[start..end])?;
                if actual.len() > expected.len() {
                    break;
                }
                if super::super::compaction::durable_messages_equal(
                    &expected_refs,
                    &actual.iter().collect::<Vec<_>>(),
                ) {
                    if matched.is_some() {
                        return Err(ConversationError::CorruptRow(
                            "ambiguous live context range".into(),
                        ));
                    }
                    matched = Some(start..end);
                    break;
                }
            }
        }
        let matched = matched
            .ok_or_else(|| ConversationError::CorruptRow("live context range missing".into()))?;
        cursor = matched.end;
        live.push(LiveSourceRange {
            range,
            messages: &messages[matched],
        });
    }
    Ok(live)
}

fn comparable_assistant_segments(
    messages: &[vega_runtime::ChatMessage],
) -> Result<Vec<vega_runtime::ChatMessage>, ConversationError> {
    let mut comparable = Vec::new();
    let mut text = String::new();
    for message in messages {
        if message.role == vega_runtime::ChatRole::Assistant {
            if !message.images.is_empty() || message.tool_call_id.is_some() {
                return Err(ConversationError::CorruptRow(
                    "invalid live assistant source".into(),
                ));
            }
            text.push_str(&message.content);
            if !message.tool_calls.is_empty() {
                let mut assistant = message.clone();
                assistant.content = std::mem::take(&mut text);
                comparable.push(assistant);
            }
        } else {
            if !text.is_empty() {
                comparable.push(vega_runtime::ChatMessage::new(
                    vega_runtime::ChatRole::Assistant,
                    std::mem::take(&mut text),
                ));
            }
            comparable.push(message.clone());
        }
    }
    if !text.is_empty() {
        comparable.push(vega_runtime::ChatMessage::new(
            vega_runtime::ChatRole::Assistant,
            text,
        ));
    }
    Ok(comparable)
}

pub(super) fn retained_source(
    source: &vega_store::context_compaction::ContextSource,
    plan: &ProjectionPlan,
) -> Result<vega_store::context_compaction::ContextSource, ConversationError> {
    plan.validate(source)
        .map_err(|_| ConversationError::CorruptRow("invalid context projection plan".into()))?;
    let mut retained = source.clone();
    retained.messages.clear();
    retained.tool_calls.clear();
    retained.images.clear();
    for fragment in &plan.fragments {
        if let ProjectionFragment::Source { range } = fragment {
            let selected = range.project(source).map_err(|_| {
                ConversationError::CorruptRow("invalid retained context range".into())
            })?;
            if selected.messages.iter().any(|message| {
                retained
                    .messages
                    .iter()
                    .any(|existing| existing.id == message.id)
            }) {
                return Err(ConversationError::CorruptRow(
                    "overlapping retained message ranges".into(),
                ));
            }
            retained.messages.extend(selected.messages);
            retained.tool_calls.extend(selected.tool_calls);
            retained.images.extend(selected.images);
        }
    }
    Ok(retained)
}
