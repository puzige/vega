use super::*;
use crate::nsh::ProjectionId;
use gpui_kit::WeakEntity;
use vega_conversation::types::AutomationControl;

mod leaf;
pub(in crate::conversation_stream) use leaf::Control;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::conversation_stream) enum Intent {
    Read,
    Edit,
    Clear,
    Control,
    Authorize,
    Permission(Action),
    Details,
}

#[derive(Clone)]
pub(in crate::conversation_stream) struct Binding {
    owner: WeakEntity<ConversationStream>,
    root: super::super::selection_menu::ReadingOwner,
    thread: Thread,
    projection: AutomationProjection,
    phase: Phase,
    operation: u64,
    task_id: Option<String>,
    task: Option<AutomationTask>,
    intent: Intent,
    focus: Option<FocusHandle>,
    enabled: bool,
    epoch: ProjectionId,
    gate: [bool; 7],
}

impl Binding {
    pub(super) fn capture(
        stream: &ConversationStream,
        task: Option<&AutomationTask>,
        intent: Intent,
        focus: Option<FocusHandle>,
        cx: &Context<ConversationStream>,
    ) -> Self {
        Self {
            owner: cx.weak_entity(),
            root: super::super::selection_menu::ReadingOwner::message(
                stream,
                cx.weak_entity(),
                None,
            ),
            thread: stream.thread.clone(),
            projection: stream.automation.projection.clone(),
            phase: stream.automation_permission.phase.clone(),
            operation: stream.automation_permission.operation,
            task_id: stream.automation_permission.task_id.clone(),
            task: task.cloned(),
            intent,
            focus,
            enabled: Self::allowed(stream, intent, task),
            epoch: stream.reading_navigation_epoch.clone(),
            gate: Self::gate(stream),
        }
    }

    fn gate(stream: &ConversationStream) -> [bool; 7] {
        [
            stream.actions.running,
            stream.composer_submit_pending,
            stream.trusted_action_busy,
            stream.model_selection_pending.is_some(),
            stream.context_operation_busy(),
            stream.automation.projection.busy,
            stream.permission_change_pending(),
        ]
    }

    fn focus_current(&self, stream: &ConversationStream) -> bool {
        let expected = match self.intent {
            Intent::Edit | Intent::Clear | Intent::Control | Intent::Authorize => self
                .task
                .as_ref()
                .and_then(|task| stream.automation.focus.get(&task.identity))
                .map(|focus| {
                    &focus[match self.intent {
                        Intent::Edit => 0,
                        Intent::Control => 1,
                        Intent::Clear => 2,
                        _ => 3,
                    }]
                }),
            Intent::Details => Some(&stream.automation_permission.details),
            Intent::Permission(Action::Cancel) => Some(&stream.automation_permission.cancel),
            Intent::Permission(Action::Confirm) => Some(&stream.automation_permission.confirm),
            Intent::Permission(Action::Reload) => Some(&stream.automation_permission.reload),
            Intent::Read => None,
        };
        self.focus.as_ref() == expected
    }

    fn allowed(stream: &ConversationStream, intent: Intent, task: Option<&AutomationTask>) -> bool {
        if stream.reading_preview
            || stream.permissions_revoked
            || stream.thread.backend != TaskBackend::Native
        {
            return false;
        }
        match intent {
            Intent::Read | Intent::Details => true,
            Intent::Permission(Action::Cancel) => matches!(
                stream.automation_permission.phase,
                Phase::Preparing(_) | Phase::Confirming(_)
            ),
            Intent::Permission(Action::Confirm) => {
                matches!(&stream.automation_permission.phase, Phase::Confirming(change) if stream.automation.projection.tasks.iter().any(|task| task == change.task()))
                    && stream.automation_permission.source.as_ref() == Some(&stream.thread)
                    && stream.thread.permission_mode == PermissionMode::FullAccess
                    && stream.thread.mode == ThreadMode::Execute
                    && !stream.automation.projection.busy
            }
            Intent::Permission(Action::Reload) => {
                matches!(stream.automation_permission.phase, Phase::Failed(_))
            }
            Intent::Edit => task.is_some() && !stream.permission_change_pending(),
            Intent::Clear | Intent::Control => {
                task.is_some_and(|task| intent == Intent::Clear || task.can_control())
                    && !stream.automation.projection.busy
                    && !stream.permission_change_pending()
            }
            Intent::Authorize => {
                task.is_some_and(|task| !task.status.replaceable())
                    && stream.thread.permission_mode == PermissionMode::FullAccess
                    && stream.thread.mode == ThreadMode::Execute
                    && !stream.permission_change_pending()
                    && !stream.actions.running
                    && !stream.composer_submit_pending
                    && !stream.trusted_action_busy
                    && stream.model_selection_pending.is_none()
                    && !stream.context_operation_busy()
                    && !stream.automation.projection.busy
            }
        }
    }

    fn same(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.root.same_binding(&other.root)
            && self.thread == other.thread
            && self.projection == other.projection
            && self.phase == other.phase
            && self.operation == other.operation
            && self.task_id == other.task_id
            && self.task == other.task
            && self.intent == other.intent
            && self.focus == other.focus
            && self.enabled == other.enabled
            && self.gate == other.gate
            && self.epoch.matches(&other.epoch)
    }

    fn source_current(&self, stream: &ConversationStream) -> bool {
        self.thread == stream.thread
            && self.gate == Self::gate(stream)
            && self.focus_current(stream)
            && self.projection == stream.automation.projection
            && self.phase == stream.automation_permission.phase
            && self.operation == stream.automation_permission.operation
            && self.task_id == stream.automation_permission.task_id
            && self.epoch.matches(&stream.reading_navigation_epoch)
            && self.task.as_ref().is_none_or(|task| {
                stream
                    .automation
                    .projection
                    .tasks
                    .iter()
                    .any(|current| current == task)
            })
            && self.enabled == Self::allowed(stream, self.intent, self.task.as_ref())
    }

    fn current(&self, window: &Window, cx: &App) -> bool {
        self.root.root_current(window, cx)
            && self.owner.upgrade().is_some_and(|owner| {
                let stream = owner.read(cx);
                !stream.reading_preview
                    && !stream.permissions_revoked
                    && self.source_current(stream)
            })
    }

    fn composing(&self, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_none_or(|owner| owner.read(cx).input.read(cx).is_composing())
    }

    fn dispatch(&self, action: Navigation, window: &mut Window, cx: &mut App) {
        if !self.current(window, cx) || !self.enabled {
            return;
        }
        let _ = self.owner.update(cx, |stream, cx| {
            if !self.source_current(stream)
                || !Self::allowed(stream, self.intent, self.task.as_ref())
            {
                return;
            }
            match action {
                Navigation::Escape => stream.cancel_automation_permission(cx),
                Navigation::Neighbor { backwards } => {
                    if stream.automation_permission.busy() {
                        let focuses: Vec<_> = match stream.automation_permission.phase {
                            Phase::Preparing(_) => {
                                vec![stream.automation_permission.cancel.clone()]
                            }
                            Phase::Confirming(_) => vec![
                                stream.automation_permission.cancel.clone(),
                                stream.automation_permission.confirm.clone(),
                                stream.automation_permission.details.clone(),
                            ],
                            Phase::Failed(_) => vec![stream.automation_permission.reload.clone()],
                            _ => Vec::new(),
                        };
                        if !focuses.is_empty() {
                            let current = focuses
                                .iter()
                                .position(|focus| focus.is_focused(window))
                                .unwrap_or(0);
                            let next = (current + if backwards { focuses.len() - 1 } else { 1 })
                                % focuses.len();
                            window.focus(&focuses[next], cx);
                        }
                    } else if backwards {
                        stream.previous_composer_control(&PreviousComposerControl, window, cx);
                    } else {
                        stream.next_composer_control(&NextComposerControl, window, cx);
                    }
                }
                Navigation::Activate => match self.intent {
                    Intent::Permission(action) => stream.automation_permission_action(action, cx),
                    Intent::Authorize => {
                        if let Some(task) = &self.task {
                            stream.request_automation_permission(
                                task.clone(),
                                task.status != AutomationStatus::Active,
                                None,
                                cx,
                            );
                        }
                    }
                    Intent::Edit => {
                        if let Some(task) = &self.task {
                            cx.emit(super::super::automation::AutomationEditorRequested {
                                thread_id: stream.thread.id.clone(),
                                identity: task.identity.clone(),
                            });
                        }
                    }
                    Intent::Clear | Intent::Control => {
                        if let Some(task) = &self.task {
                            let control = if self.intent == Intent::Clear {
                                AutomationControl::Clear
                            } else if task.status == AutomationStatus::Active {
                                AutomationControl::Pause
                            } else {
                                AutomationControl::Resume
                            };
                            cx.emit(super::super::automation::AutomationRequested {
                                thread_id: stream.thread.id.clone(),
                                command: vega_conversation::types::AutomationCommand::Control(
                                    task.kind, control,
                                ),
                            });
                        }
                    }
                    _ => {}
                },
            }
        });
    }
}

#[derive(Clone, Copy)]
enum Navigation {
    Activate,
    Neighbor { backwards: bool },
    Escape,
}

pub(in crate::conversation_stream) fn control(
    stream: &ConversationStream,
    id: String,
    text: String,
    intent: Intent,
    task: Option<&AutomationTask>,
    focus: Option<FocusHandle>,
    cx: &Context<ConversationStream>,
) -> Control {
    let colors = theme(cx).colors;
    Control {
        id: id.into(),
        label: text.clone(),
        text: if matches!(intent, Intent::Clear | Intent::Control) {
            String::new()
        } else {
            text
        },
        glyph: crate::icons::nsh_source(match intent {
            Intent::Edit
                if task.is_some_and(|task| {
                    task.kind == vega_conversation::types::AutomationKind::Goal
                }) =>
            {
                crate::icons::Icon::Goal
            }
            Intent::Edit => crate::icons::Icon::Refresh,
            Intent::Clear => crate::icons::Icon::Trash,
            Intent::Control if task.is_some_and(|task| task.status == AutomationStatus::Active) => {
                crate::icons::Icon::Pause
            }
            Intent::Control => crate::icons::Icon::Play,
            _ => crate::icons::Icon::Maximize,
        })
        .filter(|_| matches!(intent, Intent::Edit | Intent::Clear | Intent::Control)),
        truncate: intent == Intent::Edit,
        button: !matches!(intent, Intent::Read | Intent::Details),
        details: intent == Intent::Details,
        colors,
        foreground: colors.text_secondary,
        binding: Binding::capture(stream, task, intent, focus, cx),
    }
}
