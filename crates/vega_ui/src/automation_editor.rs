mod nsh_editor;

use crate::text_input::TextInput;
use gpui_kit::prelude::*;
use gpui_kit::*;
use vega_conversation::types::{AutomationCommand, AutomationKind, AutomationTask};

actions!(
    vega_automation_editor,
    [SaveAutomation, CloseAutomationEditor]
);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AutomationEditorKey {
    pub thread_id: String,
    pub kind: AutomationKind,
    pub identity: String,
}

impl AutomationEditorKey {
    pub fn for_task(task: &AutomationTask) -> Self {
        Self {
            thread_id: task.thread_id.clone(),
            kind: task.kind,
            identity: task.identity.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationEditSubmission {
    pub key: AutomationEditorKey,
    pub revision: u64,
    pub instruction: String,
    pub request: u64,
}

impl AutomationEditSubmission {
    pub fn command(&self) -> AutomationCommand {
        AutomationCommand::Edit {
            kind: self.key.kind,
            identity: self.key.identity.clone(),
            revision: self.revision,
            instruction: self.instruction.clone(),
        }
    }
}

#[derive(Clone)]
pub enum AutomationEditorEvent {
    Save(AutomationEditSubmission),
    Changed,
    Close,
    OpenLatest(Box<AutomationTask>),
}

pub struct AutomationEditor {
    base: AutomationTask,
    latest: Option<AutomationTask>,
    input: Entity<TextInput>,
    pending: Option<AutomationEditSubmission>,
    request: u64,
    error: Option<crate::localized_copy::LocalizedCopy>,
    elapsed_ms: u64,
    draft: nsh_editor::Draft,
    carrier_text: String,
    presentation: Option<nsh_editor::Presentation>,
}

impl EventEmitter<AutomationEditorEvent> for AutomationEditor {}

impl AutomationEditor {
    pub fn new(task: AutomationTask, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new_multiline(cx, "", 8)
                .with_placeholder_key("ui.task_content")
                .with_editor_viewport()
                .with_tab_stop(true);
            input.set_text(&task.instruction, cx);
            input
        });
        cx.observe(&input, |this, _, cx| {
            this.observe_carrier(cx);
            this.publish_nsh(cx);
            cx.emit(AutomationEditorEvent::Changed);
            cx.notify();
        })
        .detach();
        Self {
            draft: nsh_editor::Draft::new(task.instruction.clone()),
            carrier_text: task.instruction.clone(),
            presentation: None,
            elapsed_ms: task.elapsed_ms,
            latest: Some(task.clone()),
            base: task,
            input,
            pending: None,
            request: 0,
            error: None,
        }
    }
    pub fn key(&self) -> AutomationEditorKey {
        AutomationEditorKey::for_task(&self.base)
    }
    pub fn instruction<'a>(&self, cx: &'a App) -> &'a str {
        self.input.read(cx).text()
    }
    pub fn input(&self) -> Entity<TextInput> {
        self.input.clone()
    }
    pub fn is_dirty(&self, cx: &App) -> bool {
        if self.input.read(cx).text() != self.carrier_text {
            self.instruction(cx) != self.base.instruction
        } else {
            self.draft.text() != self.base.instruction
        }
    }
    pub fn has_conflict(&self) -> bool {
        self.latest.as_ref().is_some_and(|task| {
            task.identity != self.base.identity || task.revision != self.base.revision
        })
    }
    pub fn can_save(&self, cx: &App) -> bool {
        let valid = if self.input.read(cx).text() != self.carrier_text {
            let text = self.instruction(cx);
            text != self.base.instruction && !text.trim().is_empty() && text.len() <= 16384
        } else {
            self.draft.can_save(&self.base.instruction)
        };
        self.latest.is_some()
            && !self.has_conflict()
            && self.pending.is_none()
            && valid
            && !self.input.read(cx).is_composing()
    }
    pub fn refresh(
        &mut self,
        tasks: &[AutomationTask],
        elapsed_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.observe_carrier(cx);
        let latest = tasks
            .iter()
            .find(|task| task.thread_id == self.base.thread_id && task.kind == self.base.kind)
            .cloned();
        let elapsed_ms = elapsed_ms.unwrap_or_else(|| {
            latest
                .as_ref()
                .filter(|task| task.identity == self.base.identity)
                .map_or(self.elapsed_ms, |task| task.elapsed_ms)
        });
        if latest == self.latest && elapsed_ms == self.elapsed_ms {
            return;
        }
        self.elapsed_ms = elapsed_ms;
        if self.pending.is_none()
            && let Some(task) = &latest
            && task.identity == self.base.identity
            && self
                .draft
                .rebase_if_clean(&self.base.instruction, &task.instruction)
        {
            self.base = task.clone();
            self.sync_carrier(cx);
        }
        self.latest = latest;
        self.publish_nsh(cx);
        cx.notify();
    }
    pub fn save(&mut self, cx: &mut Context<Self>) {
        self.observe_carrier(cx);
        self.save_observed(&self.draft.read(), cx);
    }
    fn save_observed(&mut self, observed: &nsh_editor::Values, cx: &mut Context<Self>) -> bool {
        if !self.can_save(cx) {
            return false;
        }
        let Some(instruction) = self.draft.claim(observed, &self.base.instruction) else {
            return false;
        };
        self.sync_carrier(cx);
        self.request = self.request.wrapping_add(1);
        let submission = AutomationEditSubmission {
            key: self.key(),
            revision: self.base.revision,
            instruction,
            request: self.request,
        };
        self.pending = Some(submission.clone());
        self.error = None;
        self.publish_nsh(cx);
        cx.emit(AutomationEditorEvent::Save(submission));
        cx.notify();
        true
    }
    pub fn finish_save(
        &mut self,
        submitted: &AutomationEditSubmission,
        result: Result<Vec<AutomationTask>, String>,
        cx: &mut Context<Self>,
    ) {
        self.finish_save_localized(submitted, result.map_err(Into::into), cx);
    }
    pub fn finish_save_localized(
        &mut self,
        submitted: &AutomationEditSubmission,
        result: Result<Vec<AutomationTask>, crate::localized_copy::LocalizedCopy>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.as_ref() != Some(submitted) || self.key() != submitted.key {
            return;
        }
        self.pending = None;
        self.draft.release();
        match result {
            Ok(tasks) => {
                if let Some(task) = tasks
                    .iter()
                    .find(|task| AutomationEditorKey::for_task(task) == submitted.key)
                {
                    self.base = task.clone();
                    self.elapsed_ms = self.elapsed_ms.max(task.elapsed_ms);
                    self.latest = Some(task.clone());
                    self.error = None;
                } else {
                    self.latest = None;
                    self.error = Some(crate::localized_copy::LocalizedCopy::new("ui.goal_cleared"));
                }
            }
            Err(error) => self.error = Some(error),
        }
        self.sync_carrier(cx);
        self.publish_nsh(cx);
        cx.emit(AutomationEditorEvent::Changed);
        cx.notify();
    }
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        if let Some(task) = self.latest.clone() {
            if task.identity != self.base.identity {
                cx.emit(AutomationEditorEvent::OpenLatest(Box::new(task)));
                return;
            }
            self.draft.reset(task.instruction.clone());
            self.sync_carrier(cx);
            self.base = task;
            self.error = None;
            self.publish_nsh(cx);
            cx.emit(AutomationEditorEvent::Changed);
            cx.notify();
        }
    }
}

impl Focusable for AutomationEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }
}

pub fn format_elapsed(elapsed_ms: u64) -> String {
    let seconds = elapsed_ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}

impl AutomationEditor {
    fn details(&self) -> Vec<String> {
        let task = self
            .latest
            .as_ref()
            .filter(|task| task.identity == self.base.identity)
            .unwrap_or(&self.base);
        let mut details = vec![
            task.status.label().to_string(),
            vega_i18n::t_args(
                "ui.run_time_duration",
                &[("duration", &format_elapsed(self.elapsed_ms))],
            ),
        ];
        if task.kind == AutomationKind::Goal {
            details.push(if task.usage_complete {
                vega_i18n::t_args(
                    "ui.used_tokens_tokens",
                    &[("tokens", &task.used_tokens.to_string())],
                )
            } else {
                vega_i18n::t_args(
                    "ui.usage_unknown_tokens_tokens_recorded",
                    &[("tokens", &task.used_tokens.to_string())],
                )
            });
        }
        if let Some(budget) = task.token_budget {
            details.push(vega_i18n::t_args(
                "ui.budget_budget_tokens",
                &[("budget", &budget.to_string())],
            ));
        }
        if task.kind == AutomationKind::Loop {
            details.push(task.interval_seconds.map_or_else(
                || vega_i18n::t("ui.the_model_schedules_the_next_run").into(),
                |seconds| {
                    vega_i18n::t_args(
                        "ui.interval_duration",
                        &[("duration", &format_elapsed(seconds.saturating_mul(1000)))],
                    )
                },
            ));
            if let Some(expiry) = task.expires_at_ms {
                details.push(vega_i18n::t_args(
                    "ui.expires_expiry",
                    &[("expiry", &format_timestamp(expiry))],
                ));
            }
        }
        details
    }
}

impl Render for AutomationEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let surface = self.render_nsh(window, cx);
        div()
            .size_full()
            .key_context("AutomationEditor")
            .on_action(cx.listener(|this, _: &SaveAutomation, _, cx| {
                this.save(cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &CloseAutomationEditor, _, cx| {
                cx.emit(AutomationEditorEvent::Close);
                cx.stop_propagation();
            }))
            .child(surface)
    }
}

fn format_timestamp(milliseconds: i64) -> String {
    let (year, month, day) = crate::sidebar::civil_from_days(milliseconds.div_euclid(86_400_000));
    let seconds = milliseconds.div_euclid(1000).rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        seconds / 3600,
        seconds % 3600 / 60
    )
}
