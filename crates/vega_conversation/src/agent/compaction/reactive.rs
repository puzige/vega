use super::*;
use vega_store::context_compaction::{ContextCheckpoint, ContextSource};
use vega_store::context_projection::{
    AssistantCursor, ContinuationMode, ProjectionFragment, ProjectionPlan, ProjectionStrategy,
    SourceCursor, SourceRange,
};

struct ResponseGroup {
    fragment: ProjectionFragment,
    history: Vec<ChatMessage>,
    tokens: u64,
}

pub(super) struct ResponsePlanner {
    groups: Vec<ResponseGroup>,
    future: SourceRange,
    prior: Option<ProjectionPlan>,
    previous_id: Option<i64>,
    split: usize,
    head: usize,
    truncations: usize,
    fallback_started: bool,
    pending_user: Option<usize>,
    deeper_limit: Option<usize>,
}

fn invalid() -> ContextCompactionFailure {
    failure(context_error(ContextRuntimeError::InvalidProjection), None)
}

fn cursor_key(cursor: &SourceCursor) -> (u64, bool, u64, u64) {
    match &cursor.assistant {
        Some(response) => (
            cursor.message_seq,
            false,
            response.tool_seq,
            response.text_bytes,
        ),
        None => (cursor.message_seq, true, 0, 0),
    }
}

fn push_range(ranges: &mut Vec<SourceRange>, range: SourceRange) {
    if let Some(previous) = ranges.last_mut()
        && previous.through == range.after
    {
        previous.through = range.through;
    } else {
        ranges.push(range);
    }
}

fn push_fragment(fragments: &mut Vec<ProjectionFragment>, fragment: ProjectionFragment) {
    if let ProjectionFragment::Source { range } = &fragment
        && let Some(ProjectionFragment::Source { range: previous }) = fragments.last_mut()
        && previous.through == range.after
    {
        previous.through = range.through.clone();
    } else {
        fragments.push(fragment);
    }
}

impl ResponsePlanner {
    pub(super) fn new(
        source: &ContextSource,
        checkpoint: Option<&ContextCheckpoint>,
        owner: Option<&str>,
    ) -> Result<Self, ContextCompactionFailure> {
        let prior = projection::previous_plan(source, checkpoint)?;
        let fragments = prior.as_ref().map_or_else(
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
        let mut groups = Vec::new();
        let mut future = None;
        for fragment in fragments {
            let ProjectionFragment::Source { range } = &fragment else {
                let history = projection::fragment_history(source, checkpoint, &fragment)?;
                let tokens = estimate_chat_context("", &history, &[])
                    .map_err(|error| failure(VegaError::Context(error.into()), None))?
                    .input_tokens;
                groups.push(ResponseGroup {
                    fragment,
                    history,
                    tokens,
                });
                continue;
            };
            let mut after = range.after.clone();
            for message in &source.messages {
                let whole = SourceCursor {
                    message_id: message.id.clone(),
                    message_seq: message.seq as u64,
                    assistant: None,
                };
                if range
                    .after
                    .as_ref()
                    .is_some_and(|cursor| cursor_key(cursor) >= cursor_key(&whole))
                    || range
                        .through
                        .as_ref()
                        .is_some_and(|cursor| cursor.message_seq < whole.message_seq)
                {
                    continue;
                }
                let mut boundaries = Vec::new();
                let mut calls = source
                    .tool_calls
                    .iter()
                    .filter(|call| call.message_id == message.id)
                    .collect::<Vec<_>>();
                calls.sort_by_key(|call| call.seq);
                if calls.iter().all(|call| call.text_offset_bytes.is_some()) {
                    for call in &calls {
                        let boundary = SourceCursor {
                            message_id: message.id.clone(),
                            message_seq: message.seq as u64,
                            assistant: Some(AssistantCursor {
                                text_bytes: call.text_offset_bytes.ok_or_else(invalid)? as u64,
                                tool_seq: call.seq as u64,
                                tool_id: call.id.clone(),
                                response_id: call.response_id.clone(),
                            }),
                        };
                        if boundary.validate(source).is_ok() {
                            boundaries.push(boundary);
                        }
                    }
                }
                if matches!(message.status.as_str(), "done" | "failed" | "interrupted") {
                    if range
                        .through
                        .as_ref()
                        .is_none_or(|end| end.assistant.is_none())
                        && boundaries
                            .last()
                            .and_then(|boundary| boundary.assistant.as_ref())
                            .is_some_and(|cursor| {
                                cursor.text_bytes as usize == message.content.len()
                            })
                    {
                        boundaries.pop();
                    }
                    boundaries.push(whole);
                } else if message.status != "streaming"
                    || Some(message.id.as_str()) != owner
                    || calls.last().map_or(!message.content.is_empty(), |call| {
                        call.text_offset_bytes
                            .and_then(|offset| usize::try_from(offset).ok())
                            != Some(message.content.len())
                    })
                {
                    return Err(invalid());
                }
                for boundary in boundaries {
                    if after
                        .as_ref()
                        .is_some_and(|cursor| cursor_key(cursor) >= cursor_key(&boundary))
                        || range
                            .through
                            .as_ref()
                            .is_some_and(|cursor| cursor_key(cursor) < cursor_key(&boundary))
                    {
                        continue;
                    }
                    let selected = SourceRange {
                        after: after.clone(),
                        through: Some(boundary.clone()),
                    };
                    selected.validate_terminal(source).map_err(|_| invalid())?;
                    let history = super::super::pipeline::history_from_context_source(
                        &selected.project(source).map_err(|_| invalid())?,
                        "__summary__",
                        None,
                    )
                    .map_err(|_| invalid())?;
                    let tokens = estimate_chat_context("", &history, &[])
                        .map_err(|error| failure(VegaError::Context(error.into()), None))?
                        .input_tokens;
                    groups.push(ResponseGroup {
                        fragment: ProjectionFragment::Source { range: selected },
                        history,
                        tokens,
                    });
                    after = Some(boundary);
                }
            }
            if let Some(through) = &range.through {
                if after.as_ref() != Some(through) {
                    return Err(invalid());
                }
            } else {
                if future.is_some() {
                    return Err(invalid());
                }
                future = Some(SourceRange {
                    after,
                    through: None,
                });
            }
        }
        let pending_user = groups
            .iter()
            .rposition(|group| {
                matches!(&group.fragment, ProjectionFragment::Source { .. })
                    && group
                        .history
                        .iter()
                        .any(|message| message.role == ChatRole::User)
            })
            .filter(|index| {
                !groups[index + 1..].iter().any(|group| {
                    group
                        .history
                        .iter()
                        .any(|message| message.role == ChatRole::Assistant)
                })
            });
        let split = groups.len();
        Ok(Self {
            groups,
            future: future.ok_or_else(invalid)?,
            prior,
            previous_id: checkpoint.map(|checkpoint| checkpoint.id),
            split,
            head: 0,
            truncations: 0,
            fallback_started: false,
            pending_user,
            deeper_limit: None,
        })
    }

    pub(super) fn start_reactive(
        &mut self,
        trigger: ContextCompactionTrigger,
    ) -> Result<(), ContextCompactionFailure> {
        if self.groups.len() < 2 {
            return Err(failure(
                context_error(ContextRuntimeError::NoCompactablePrefix),
                None,
            ));
        }
        let (gap, deeper) = match trigger {
            ContextCompactionTrigger::InputOverflow { token_gap, deeper } => (token_gap, deeper),
            _ => (None, false),
        };
        if deeper {
            if self
                .prior
                .as_ref()
                .is_none_or(|plan| plan.strategy != ProjectionStrategy::Reactive)
            {
                return Err(invalid());
            }
            let count = self.raw_count(0, self.groups.len());
            if count < 2 {
                return Err(invalid());
            }
            self.deeper_limit = Some(count);
        }
        self.split = self.groups.len() - 1;
        if !deeper && let Some(gap) = gap {
            let mut retained = self.groups[self.split].tokens;
            while self.split > 1 && retained < gap {
                self.split -= 1;
                retained = retained.saturating_add(self.groups[self.split].tokens);
            }
        }
        if let Some(index) = self.pending_user {
            self.split = self.split.min(index);
        }
        if self.split == 0 {
            return Err(failure(
                context_error(ContextRuntimeError::NoCompactablePrefix),
                None,
            ));
        }
        Ok(())
    }

    pub(super) fn align(&mut self, plan: &ProjectionPlan) {
        self.split = self.groups.iter().position(|group| match &group.fragment {
            ProjectionFragment::Source { range } => plan.fragments.iter().any(|fragment| {
                matches!(fragment, ProjectionFragment::Source { range: retained } if retained.after == range.after)
            }),
            ProjectionFragment::Summary => plan.fragments.iter().any(|fragment| {
                matches!(fragment, ProjectionFragment::PreviousSummary { checkpoint_id } if Some(*checkpoint_id) == self.previous_id)
            }),
            fragment @ ProjectionFragment::PreviousSummary { .. } => plan.fragments.iter().any(|retained| retained == fragment),
            _ => false,
        }).unwrap_or(self.groups.len());
    }

    fn raw_count(&self, start: usize, end: usize) -> usize {
        self.groups[start..end]
            .iter()
            .filter(|group| {
                matches!(group.fragment, ProjectionFragment::Source { .. })
                    && !group.history.is_empty()
            })
            .count()
    }

    pub(super) fn retained_groups(&self) -> usize {
        self.raw_count(self.split, self.groups.len())
    }

    pub(super) fn retreat(&mut self, gap: Option<u64>) -> bool {
        if self.fallback_started {
            return false;
        }
        let before = self.split;
        let mut removed = 0u64;
        while self.split > self.head + 1 {
            let next = self.split - 1;
            if self
                .deeper_limit
                .is_some_and(|limit| self.raw_count(next, self.groups.len()) >= limit)
            {
                break;
            }
            self.split = next;
            removed = removed.saturating_add(self.groups[next].tokens);
            if removed >= gap.unwrap_or(1).max(1) {
                break;
            }
        }
        before != self.split
    }

    pub(super) fn fallback(&mut self, gap: Option<u64>) -> bool {
        let end = self.pending_user.unwrap_or(self.groups.len());
        if !self.fallback_started {
            self.fallback_started = true;
            if self.split != end {
                self.split = end;
                return true;
            }
        }
        self.truncate_head(gap)
    }

    pub(super) fn truncate_head(&mut self, gap: Option<u64>) -> bool {
        if self.truncations >= 3 {
            return false;
        }
        let end = self.pending_user.unwrap_or(self.groups.len());
        let remaining = self.raw_count(self.head, end);
        if remaining < 2 {
            return false;
        }
        let count_target = remaining.div_ceil(5).max(1);
        let mut tokens = 0u64;
        let mut count = 0;
        let mut next = self.head;
        while next < end && count < remaining - 1 {
            let group = &self.groups[next];
            next += 1;
            if matches!(group.fragment, ProjectionFragment::Source { .. })
                && !group.history.is_empty()
            {
                count += 1;
                tokens = tokens.saturating_add(group.tokens);
                if gap.map_or(count >= count_target, |gap| tokens >= gap.max(1)) {
                    break;
                }
            }
        }
        if count == 0 {
            return false;
        }
        self.head = next;
        self.split = end;
        self.truncations += 1;
        true
    }

    pub(super) fn plan(
        &self,
        source: &ContextSource,
        request: &ContextCompactionRequest,
    ) -> Result<projection::PlannedProjection, ContextCompactionFailure> {
        if self.head >= self.split
            || self
                .deeper_limit
                .is_some_and(|limit| self.retained_groups() >= limit)
        {
            return Err(invalid());
        }
        let mut covered = self
            .prior
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.covered.clone());
        let mut omitted = self
            .prior
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.omitted.clone());
        let mut fragments = Vec::new();
        let retained_fragment = |fragment: &ProjectionFragment| -> Result<ProjectionFragment, ContextCompactionFailure> {
            Ok(match fragment {
                ProjectionFragment::Summary => ProjectionFragment::PreviousSummary { checkpoint_id: self.previous_id.ok_or_else(invalid)? },
                other => other.clone(),
            })
        };
        for group in &self.groups[..self.head] {
            if let ProjectionFragment::Source { range } = &group.fragment {
                push_range(&mut omitted, range.clone());
            } else {
                push_fragment(&mut fragments, retained_fragment(&group.fragment)?);
            }
        }
        let mut history = Vec::new();
        for group in &self.groups[self.head..self.split] {
            if let ProjectionFragment::Source { range } = &group.fragment {
                push_range(&mut covered, range.clone());
            }
            history.extend_from_slice(&group.history);
        }
        if history.is_empty() {
            return Err(invalid());
        }
        fragments.push(ProjectionFragment::Summary);
        let image_through = self.groups[..self.split]
            .iter()
            .filter_map(|group| match &group.fragment {
                ProjectionFragment::Source { range } => range.through.as_ref().map(|cursor| {
                    if cursor.assistant.is_some() {
                        cursor.message_seq.saturating_sub(1)
                    } else {
                        cursor.message_seq
                    }
                }),
                ProjectionFragment::HistoricalImages { through_seq } => Some(*through_seq),
                _ => None,
            })
            .max();
        let mut image_fragment = fragments
            .iter()
            .any(|fragment| matches!(fragment, ProjectionFragment::HistoricalImages { .. }))
            || self.groups[self.split..]
                .iter()
                .any(|group| matches!(group.fragment, ProjectionFragment::HistoricalImages { .. }));
        if !image_fragment
            && let Some(through_seq) = image_through
            && source.images.iter().any(|image| {
                source.messages.iter().any(|message| {
                    message.id == image.message_id && message.seq as u64 <= through_seq
                })
            })
        {
            fragments.push(ProjectionFragment::HistoricalImages { through_seq });
            image_fragment = true;
        }
        let task = source
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "user" && message.status == "done");
        let retained_task = task.is_some_and(|task| {
            fragments.iter().any(|fragment| matches!(fragment, ProjectionFragment::TaskReference { message_id } if *message_id == task.id))
                || self.groups[self.split..].iter().any(|group| {
                    matches!(&group.fragment, ProjectionFragment::Source { range } if range.project(source).is_ok_and(|selected| selected.messages.iter().any(|message| message.id == task.id)))
                        || matches!(&group.fragment, ProjectionFragment::TaskReference { message_id } if *message_id == task.id)
                })
        });
        if request.source_owner_id.is_some()
            && !retained_task
            && let Some(task) = task
        {
            fragments.push(ProjectionFragment::TaskReference {
                message_id: task.id.clone(),
            });
        }
        for group in &self.groups[self.split..] {
            push_fragment(&mut fragments, retained_fragment(&group.fragment)?);
        }
        push_fragment(
            &mut fragments,
            ProjectionFragment::Source {
                range: self.future.clone(),
            },
        );
        if image_fragment && let Some(through) = image_through {
            for fragment in &mut fragments {
                if let ProjectionFragment::HistoricalImages { through_seq } = fragment {
                    *through_seq = (*through_seq).max(through);
                }
            }
        }
        let plan = ProjectionPlan {
            history_dependencies: None,
            schema: 1,
            strategy: ProjectionStrategy::Reactive,
            continuation: if request.source_owner_id.is_some() {
                ContinuationMode::ContinueExistingRun
            } else {
                ContinuationMode::NoContinuation
            },
            source_owner_id: request.source_owner_id.clone(),
            task_message_id: task.map(|task| task.id.clone()),
            covered,
            omitted,
            fragments,
        };
        plan.validate_install(source).map_err(|_| invalid())?;
        Ok(projection::PlannedProjection {
            plan,
            summary_history: history,
        })
    }
}
