use super::*;
use vega_store::context_compaction::ContextSource;
use vega_store::context_projection::{
    AssistantCursor, ContinuationMode, ProjectionFragment, ProjectionPlan, ProjectionStrategy,
    SourceCursor, SourceRange,
};

pub(super) struct PlannedProjection {
    pub plan: ProjectionPlan,
    pub summary_history: Vec<ChatMessage>,
}

pub(super) fn partial_plan(
    source: &ContextSource,
    checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
    selection: &crate::types::PartialCompactionSelection,
) -> Result<PlannedProjection, ContextCompactionFailure> {
    let invalid = || failure(context_error(ContextRuntimeError::InvalidProjection), None);
    if selection.source_fingerprint != source.fingerprint {
        return Err(failure(
            context_error(ContextRuntimeError::SourceChanged),
            None,
        ));
    }
    let normalized;
    let prior = if let Some(previous) = checkpoint {
        if let Some(plan) = &previous.projection_plan {
            Some(plan)
        } else {
            normalized = prefix_plan(
                source,
                previous.covered_through_seq,
                previous.covered_assistant_id.as_deref().map(|id| {
                    (
                        id,
                        previous.covered_assistant_text_bytes,
                        previous.covered_assistant_tool_seq,
                    )
                }),
                None,
                true,
            )?;
            Some(&normalized)
        }
    } else {
        None
    };
    let mut normalized_checkpoint = checkpoint.cloned();
    if let Some(previous) = &mut normalized_checkpoint {
        previous.projection_plan = prior.cloned();
    }
    let active =
        super::super::pipeline::source_after_checkpoint(source, normalized_checkpoint.as_ref())
            .map_err(|_| invalid())?;
    if !active
        .messages
        .iter()
        .any(|message| message.id == selection.message_id && message.status == "done")
    {
        return Err(invalid());
    }
    let up_to = selection.direction == crate::types::PartialCompactionDirection::UpTo;
    let original_fragments = prior.map_or_else(
        || {
            vec![ProjectionFragment::Source {
                range: SourceRange {
                    after: None,
                    through: None,
                },
            }]
        },
        |plan| plan.fragments.clone(),
    );
    let mut divided = Vec::new();
    let mut reached = false;
    for fragment in original_fragments {
        if let ProjectionFragment::Source { range } = &fragment {
            let projected = range.project(source).map_err(|_| invalid())?;
            if let Some(index) = projected
                .messages
                .iter()
                .position(|message| message.id == selection.message_id)
            {
                let boundary = if index == 0 {
                    range.after.clone()
                } else {
                    let message = &projected.messages[index - 1];
                    Some(SourceCursor {
                        message_id: message.id.clone(),
                        message_seq: message.seq as u64,
                        assistant: None,
                    })
                };
                if boundary != range.after {
                    divided.push((
                        ProjectionFragment::Source {
                            range: SourceRange {
                                after: range.after.clone(),
                                through: boundary.clone(),
                            },
                        },
                        true,
                    ));
                }
                divided.push((
                    ProjectionFragment::Source {
                        range: SourceRange {
                            after: boundary,
                            through: range.through.clone(),
                        },
                    },
                    false,
                ));
                reached = true;
                continue;
            }
        }
        divided.push((fragment, !reached));
    }
    if !reached {
        return Err(invalid());
    }
    let mut summary_history = Vec::new();
    let mut fragments = Vec::new();
    let mut prior_covered = prior.map_or_else(Vec::new, |plan| plan.covered.clone());
    let omitted = prior.map_or_else(Vec::new, |plan| plan.omitted.clone());
    for (fragment, before) in divided {
        if before != up_to {
            fragments.push(match fragment {
                ProjectionFragment::Summary => ProjectionFragment::PreviousSummary {
                    checkpoint_id: checkpoint.ok_or_else(invalid)?.id,
                },
                other => other,
            });
            continue;
        }
        match fragment {
            ProjectionFragment::Source { mut range } => {
                if range.through.is_none() {
                    let tail = source.messages.last().ok_or_else(invalid)?;
                    range.through = Some(SourceCursor {
                        message_id: tail.id.clone(),
                        message_seq: tail.seq as u64,
                        assistant: None,
                    });
                }
                if range.after == range.through {
                    continue;
                }
                range.validate_terminal(source).map_err(|_| invalid())?;
                summary_history.extend(
                    super::super::pipeline::history_from_context_source(
                        &range.project(source).map_err(|_| invalid())?,
                        "__summary__",
                        None,
                    )
                    .map_err(|_| invalid())?,
                );
                prior_covered.push(range);
            }
            ProjectionFragment::Summary => {
                summary_history.push(super::super::pipeline::historical_checkpoint_summary(
                    &checkpoint.ok_or_else(invalid)?.summary,
                    checkpoint.ok_or_else(invalid)?.id,
                ))
            }
            ProjectionFragment::PreviousSummary { checkpoint_id } => {
                let text = checkpoint
                    .and_then(|previous| previous.referenced_summaries.get(&checkpoint_id))
                    .ok_or_else(invalid)?;
                summary_history.push(super::super::pipeline::historical_checkpoint_summary(
                    text,
                    checkpoint_id,
                ));
            }
            ProjectionFragment::TaskReference { message_id } => summary_history.push(
                super::super::pipeline::historical_task_reference(source, &message_id)
                    .map_err(|_| invalid())?,
            ),
            ProjectionFragment::HistoricalImages { through_seq } => summary_history.extend(
                super::super::pipeline::historical_image_projection(source, through_seq)
                    .map_err(|_| invalid())?,
            ),
        }
    }
    if summary_history.is_empty() {
        return Err(invalid());
    }
    if up_to {
        fragments.insert(0, ProjectionFragment::Summary);
    } else {
        fragments.push(ProjectionFragment::Summary);
        let tail = source.messages.last().ok_or_else(invalid)?;
        fragments.push(ProjectionFragment::Source {
            range: SourceRange {
                after: Some(SourceCursor {
                    message_id: tail.id.clone(),
                    message_seq: tail.seq as u64,
                    assistant: None,
                }),
                through: None,
            },
        });
    }
    let plan = ProjectionPlan {
        history_dependencies: None,
        schema: 1,
        strategy: if up_to {
            ProjectionStrategy::PartialUpTo
        } else {
            ProjectionStrategy::PartialFrom
        },
        continuation: ContinuationMode::NoContinuation,
        source_owner_id: None,
        task_message_id: source
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user" && message.status == "done")
            .map(|message| message.id.clone()),
        covered: prior_covered,
        omitted,
        fragments,
    };
    plan.validate_install(source).map_err(|_| invalid())?;
    Ok(PlannedProjection {
        plan,
        summary_history,
    })
}

pub(super) fn previous_plan(
    source: &ContextSource,
    checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
) -> Result<Option<ProjectionPlan>, ContextCompactionFailure> {
    checkpoint
        .map(|previous| {
            if let Some(plan) = &previous.projection_plan {
                Ok(plan.clone())
            } else {
                prefix_plan(
                    source,
                    previous.covered_through_seq,
                    previous.covered_assistant_id.as_deref().map(|id| {
                        (
                            id,
                            previous.covered_assistant_text_bytes,
                            previous.covered_assistant_tool_seq,
                        )
                    }),
                    None,
                    true,
                )
            }
        })
        .transpose()
}

pub(super) fn fragment_history(
    source: &ContextSource,
    checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
    fragment: &ProjectionFragment,
) -> Result<Vec<ChatMessage>, ContextCompactionFailure> {
    let invalid = || failure(context_error(ContextRuntimeError::InvalidProjection), None);
    Ok(match fragment {
        ProjectionFragment::Source { range } => {
            super::super::pipeline::history_from_context_source(
                &range.project(source).map_err(|_| invalid())?,
                "__summary__",
                None,
            )
            .map_err(|_| invalid())?
        }
        ProjectionFragment::Summary => vec![super::super::pipeline::historical_checkpoint_summary(
            &checkpoint.ok_or_else(invalid)?.summary,
            checkpoint.ok_or_else(invalid)?.id,
        )],
        ProjectionFragment::PreviousSummary { checkpoint_id } => {
            vec![super::super::pipeline::historical_checkpoint_summary(
                checkpoint
                    .and_then(|checkpoint| checkpoint.referenced_summaries.get(checkpoint_id))
                    .ok_or_else(invalid)?,
                *checkpoint_id,
            )]
        }
        ProjectionFragment::TaskReference { message_id } => vec![
            super::super::pipeline::historical_task_reference(source, message_id)
                .map_err(|_| invalid())?,
        ],
        ProjectionFragment::HistoricalImages { through_seq } => {
            super::super::pipeline::historical_image_projection(source, *through_seq)
                .map_err(|_| invalid())?
        }
    })
}

pub(super) fn full_plan(
    source: &ContextSource,
    checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
    owner: Option<&str>,
) -> Result<PlannedProjection, ContextCompactionFailure> {
    let invalid = || failure(context_error(ContextRuntimeError::InvalidProjection), None);
    let prior = previous_plan(source, checkpoint)?;
    let tail = source.messages.last().ok_or_else(invalid)?;
    let mut boundary = SourceCursor {
        message_id: tail.id.clone(),
        message_seq: tail.seq as u64,
        assistant: None,
    };
    if tail.status == "streaming" {
        if Some(tail.id.as_str()) != owner {
            return Err(invalid());
        }
        if let Some(call) = source
            .tool_calls
            .iter()
            .filter(|call| call.message_id == tail.id)
            .max_by_key(|call| call.seq)
        {
            let text_bytes = call.text_offset_bytes.ok_or_else(invalid)?;
            if usize::try_from(text_bytes).ok() != Some(tail.content.len()) {
                return Err(invalid());
            }
            boundary.assistant = Some(AssistantCursor {
                text_bytes: text_bytes as u64,
                tool_seq: call.seq as u64,
                tool_id: call.id.clone(),
                response_id: call.response_id.clone(),
            });
        } else if tail.content.is_empty() {
            let previous = source.messages.iter().rev().nth(1).ok_or_else(invalid)?;
            boundary = SourceCursor {
                message_id: previous.id.clone(),
                message_seq: previous.seq as u64,
                assistant: None,
            };
        } else {
            return Err(invalid());
        }
    }
    boundary.validate(source).map_err(|_| invalid())?;
    let original = prior.as_ref().map_or_else(
        || {
            vec![ProjectionFragment::Source {
                range: SourceRange {
                    after: None,
                    through: None,
                },
            }]
        },
        |plan| plan.fragments.clone(),
    );
    let mut covered = prior
        .as_ref()
        .map_or_else(Vec::new, |plan| plan.covered.clone());
    let omitted = prior
        .as_ref()
        .map_or_else(Vec::new, |plan| plan.omitted.clone());
    let mut summary_history = Vec::new();
    for fragment in original {
        match fragment {
            ProjectionFragment::Source { mut range } => {
                if range.through.is_none() {
                    range.through = Some(boundary.clone());
                }
                if range.after == range.through {
                    continue;
                }
                range.validate_terminal(source).map_err(|_| invalid())?;
                summary_history.extend(
                    super::super::pipeline::history_from_context_source(
                        &range.project(source).map_err(|_| invalid())?,
                        "__summary__",
                        None,
                    )
                    .map_err(|_| invalid())?,
                );
                covered.push(range);
            }
            ProjectionFragment::Summary => {
                summary_history.push(super::super::pipeline::historical_checkpoint_summary(
                    &checkpoint.ok_or_else(invalid)?.summary,
                    checkpoint.ok_or_else(invalid)?.id,
                ))
            }
            ProjectionFragment::PreviousSummary { checkpoint_id } => {
                let text = checkpoint
                    .and_then(|previous| previous.referenced_summaries.get(&checkpoint_id))
                    .ok_or_else(invalid)?;
                summary_history.push(super::super::pipeline::historical_checkpoint_summary(
                    text,
                    checkpoint_id,
                ));
            }
            ProjectionFragment::TaskReference { message_id } => summary_history.push(
                super::super::pipeline::historical_task_reference(source, &message_id)
                    .map_err(|_| invalid())?,
            ),
            ProjectionFragment::HistoricalImages { through_seq } => summary_history.extend(
                super::super::pipeline::historical_image_projection(source, through_seq)
                    .map_err(|_| invalid())?,
            ),
        }
    }
    if summary_history.is_empty() {
        return Err(failure(
            context_error(ContextRuntimeError::NoCompactablePrefix),
            None,
        ));
    }
    let task = source
        .messages
        .iter()
        .rev()
        .find(|message| message.role == "user" && message.status == "done");
    let through_seq = if boundary.assistant.is_some() {
        boundary.message_seq.saturating_sub(1)
    } else {
        boundary.message_seq
    };
    let mut fragments = vec![
        ProjectionFragment::Summary,
        ProjectionFragment::HistoricalImages { through_seq },
    ];
    if owner.is_some()
        && let Some(task) = task
    {
        fragments.push(ProjectionFragment::TaskReference {
            message_id: task.id.clone(),
        });
    }
    fragments.push(ProjectionFragment::Source {
        range: SourceRange {
            after: Some(boundary),
            through: None,
        },
    });
    let plan = ProjectionPlan {
        history_dependencies: None,
        schema: 1,
        strategy: ProjectionStrategy::Full,
        continuation: if owner.is_some() {
            ContinuationMode::ContinueExistingRun
        } else {
            ContinuationMode::NoContinuation
        },
        source_owner_id: owner.map(str::to_owned),
        task_message_id: task.map(|task| task.id.clone()),
        covered,
        omitted,
        fragments,
    };
    plan.validate_install(source).map_err(|_| invalid())?;
    Ok(PlannedProjection {
        plan,
        summary_history,
    })
}

pub(super) fn ordered_prefix_plan(
    source: &ContextSource,
    checkpoint: Option<&vega_store::context_compaction::ContextCheckpoint>,
    mut plan: ProjectionPlan,
) -> Result<PlannedProjection, ContextCompactionFailure> {
    let invalid = || failure(context_error(ContextRuntimeError::InvalidProjection), None);
    let boundary = plan
        .covered
        .first()
        .and_then(|range| range.through.clone())
        .ok_or_else(invalid)?;
    let prior = previous_plan(source, checkpoint)?;
    let original = prior.as_ref().map_or_else(
        || {
            vec![ProjectionFragment::Source {
                range: SourceRange {
                    after: None,
                    through: None,
                },
            }]
        },
        |previous| previous.fragments.clone(),
    );
    plan.covered = prior
        .as_ref()
        .map_or_else(Vec::new, |previous| previous.covered.clone());
    plan.omitted = prior
        .as_ref()
        .map_or_else(Vec::new, |previous| previous.omitted.clone());
    let key = |cursor: &SourceCursor| match &cursor.assistant {
        Some(response) => (
            cursor.message_seq,
            false,
            response.tool_seq,
            response.text_bytes,
        ),
        None => (cursor.message_seq, true, 0, 0),
    };
    let mut reached = false;
    let mut retained = Vec::new();
    let mut history = Vec::new();
    for fragment in original {
        if let ProjectionFragment::Source { range } = &fragment {
            if range
                .after
                .as_ref()
                .is_some_and(|after| key(after) >= key(&boundary))
            {
                reached = true;
            }
            if !reached {
                let mut prefix = range.clone();
                if range
                    .through
                    .as_ref()
                    .is_none_or(|through| key(through) > key(&boundary))
                {
                    prefix.through = Some(boundary.clone());
                    retained.push(ProjectionFragment::Source {
                        range: SourceRange {
                            after: Some(boundary.clone()),
                            through: range.through.clone(),
                        },
                    });
                    reached = true;
                } else if range.through.as_ref() == Some(&boundary) {
                    reached = true;
                }
                prefix.validate_terminal(source).map_err(|_| invalid())?;
                history.extend(
                    super::super::pipeline::history_from_context_source(
                        &prefix.project(source).map_err(|_| invalid())?,
                        "__summary__",
                        None,
                    )
                    .map_err(|_| invalid())?,
                );
                plan.covered.push(prefix);
                continue;
            }
        }
        if reached {
            retained.push(match fragment {
                ProjectionFragment::Summary => ProjectionFragment::PreviousSummary {
                    checkpoint_id: checkpoint.ok_or_else(invalid)?.id,
                },
                other => other,
            });
            continue;
        }
        match fragment {
            ProjectionFragment::Summary => {
                history.push(super::super::pipeline::historical_checkpoint_summary(
                    &checkpoint.ok_or_else(invalid)?.summary,
                    checkpoint.ok_or_else(invalid)?.id,
                ))
            }
            ProjectionFragment::PreviousSummary { checkpoint_id } => {
                history.push(super::super::pipeline::historical_checkpoint_summary(
                    checkpoint
                        .and_then(|previous| previous.referenced_summaries.get(&checkpoint_id))
                        .ok_or_else(invalid)?,
                    checkpoint_id,
                ))
            }
            ProjectionFragment::TaskReference { message_id } => history.push(
                super::super::pipeline::historical_task_reference(source, &message_id)
                    .map_err(|_| invalid())?,
            ),
            ProjectionFragment::HistoricalImages { through_seq } => history.extend(
                super::super::pipeline::historical_image_projection(source, through_seq)
                    .map_err(|_| invalid())?,
            ),
            ProjectionFragment::Source { .. } => return Err(invalid()),
        }
    }
    if history.is_empty() || !reached {
        return Err(failure(
            context_error(ContextRuntimeError::NoCompactablePrefix),
            None,
        ));
    }
    plan.fragments
        .retain(|fragment| !matches!(fragment, ProjectionFragment::Source { .. }));
    plan.fragments.extend(retained);
    plan.validate_install(source).map_err(|_| invalid())?;
    Ok(PlannedProjection {
        plan,
        summary_history: history,
    })
}

pub(super) fn prefix_plan(
    source: &ContextSource,
    covered_seq: u64,
    partial: Option<(&str, u64, u64)>,
    owner: Option<&str>,
    manual: bool,
) -> Result<ProjectionPlan, ContextCompactionFailure> {
    let invalid = || failure(context_error(ContextRuntimeError::InvalidProjection), None);
    let boundary = if let Some((id, text_bytes, tool_seq)) = partial {
        let message = source
            .messages
            .iter()
            .find(|message| message.id == id)
            .ok_or_else(invalid)?;
        let call = source
            .tool_calls
            .iter()
            .find(|call| call.message_id == id && call.seq as u64 == tool_seq)
            .ok_or_else(invalid)?;
        SourceCursor {
            message_id: id.into(),
            message_seq: message.seq as u64,
            assistant: Some(AssistantCursor {
                text_bytes,
                tool_seq,
                tool_id: call.id.clone(),
                response_id: call.response_id.clone(),
            }),
        }
    } else {
        let message = source
            .messages
            .iter()
            .rev()
            .find(|message| message.seq as u64 <= covered_seq)
            .ok_or_else(invalid)?;
        SourceCursor {
            message_id: message.id.clone(),
            message_seq: message.seq as u64,
            assistant: None,
        }
    };
    let task = if let Some((id, _, _)) = partial {
        let owner = source
            .messages
            .iter()
            .find(|message| message.id == id)
            .ok_or_else(invalid)?;
        source.messages.iter().rev().find(|message| {
            message.role == "user" && message.status == "done" && message.seq < owner.seq
        })
    } else {
        source
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user" && message.status == "done")
    };
    let mut fragments = vec![
        ProjectionFragment::Summary,
        ProjectionFragment::HistoricalImages {
            through_seq: covered_seq,
        },
    ];
    if partial.is_some()
        && let Some(task) = task
        && task.seq as u64 <= covered_seq
    {
        fragments.push(ProjectionFragment::TaskReference {
            message_id: task.id.clone(),
        });
    }
    fragments.push(ProjectionFragment::Source {
        range: SourceRange {
            after: Some(boundary.clone()),
            through: None,
        },
    });
    let plan = ProjectionPlan {
        history_dependencies: None,
        schema: 1,
        strategy: if manual {
            ProjectionStrategy::Manual
        } else {
            ProjectionStrategy::Automatic
        },
        continuation: if owner.is_some() && !manual {
            ContinuationMode::ContinueExistingRun
        } else {
            ContinuationMode::NoContinuation
        },
        source_owner_id: owner.map(str::to_owned),
        task_message_id: task.map(|task| task.id.clone()),
        covered: vec![SourceRange {
            after: None,
            through: Some(boundary),
        }],
        omitted: Vec::new(),
        fragments,
    };
    plan.validate_install(source).map_err(|_| invalid())?;
    Ok(plan)
}
