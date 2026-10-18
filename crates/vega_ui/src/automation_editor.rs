use gpui_kit::prelude::*;
use gpui_kit::*;
use vega_conversation::types::{AutomationCommand, AutomationKind, AutomationTask};
use vega_theme::{Layout, Typography, theme};

use crate::interaction::InteractionExt;
use crate::text_input::TextInput;

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
    OpenLatest(AutomationTask),
}

pub struct AutomationEditor {
    base: AutomationTask,
    latest: Option<AutomationTask>,
    input: Entity<TextInput>,
    pending: Option<AutomationEditSubmission>,
    request: u64,
    error: Option<String>,
    elapsed_ms: u64,
    save_focus: FocusHandle,
    revert_focus: FocusHandle,
}

impl EventEmitter<AutomationEditorEvent> for AutomationEditor {}

impl AutomationEditor {
    pub fn new(task: AutomationTask, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new_multiline(cx, "任务内容", 8)
                .with_editor_viewport()
                .with_tab_stop(true);
            input.set_text(&task.instruction, cx);
            input
        });
        cx.observe(&input, |_, _, cx| {
            cx.emit(AutomationEditorEvent::Changed);
            cx.notify();
        })
        .detach();
        Self {
            elapsed_ms: task.elapsed_ms,
            latest: Some(task.clone()),
            base: task,
            input,
            pending: None,
            request: 0,
            error: None,
            save_focus: cx.focus_handle().tab_stop(true),
            revert_focus: cx.focus_handle().tab_stop(true),
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
        self.instruction(cx) != self.base.instruction
    }
    pub fn is_saving(&self) -> bool {
        self.pending.is_some()
    }
    pub fn has_conflict(&self) -> bool {
        self.latest.as_ref().is_some_and(|task| {
            task.identity != self.base.identity || task.revision != self.base.revision
        })
    }
    pub fn can_save(&self, cx: &App) -> bool {
        let text = self.instruction(cx);
        self.latest.is_some()
            && !self.has_conflict()
            && self.pending.is_none()
            && self.is_dirty(cx)
            && !text.trim().is_empty()
            && text.len() <= 16384
            && !self.input.read(cx).is_composing()
    }
    pub fn refresh(
        &mut self,
        tasks: &[AutomationTask],
        elapsed_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
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
        if !self.is_dirty(cx)
            && self.pending.is_none()
            && let Some(task) = &latest
            && task.identity == self.base.identity
        {
            if task.instruction != self.base.instruction {
                self.input
                    .update(cx, |input, cx| input.set_text(&task.instruction, cx));
            }
            self.base = task.clone();
        }
        self.latest = latest;
        cx.notify();
    }
    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.can_save(cx) {
            return;
        }
        self.request = self.request.wrapping_add(1);
        let submission = AutomationEditSubmission {
            key: self.key(),
            revision: self.base.revision,
            instruction: self.instruction(cx).to_string(),
            request: self.request,
        };
        self.pending = Some(submission.clone());
        self.error = None;
        cx.emit(AutomationEditorEvent::Save(submission));
        cx.notify();
    }
    pub fn finish_save(
        &mut self,
        submitted: &AutomationEditSubmission,
        result: Result<Vec<AutomationTask>, String>,
        cx: &mut Context<Self>,
    ) {
        if self.pending.as_ref() != Some(submitted) || self.key() != submitted.key {
            return;
        }
        self.pending = None;
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
                    self.error = Some("目标已清除".into());
                }
            }
            Err(error) => self.error = Some(error),
        }
        cx.emit(AutomationEditorEvent::Changed);
        cx.notify();
    }
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        if let Some(task) = self.latest.clone() {
            if task.identity != self.base.identity {
                cx.emit(AutomationEditorEvent::OpenLatest(task));
                return;
            }
            self.input
                .update(cx, |input, cx| input.set_text(&task.instruction, cx));
            self.base = task;
            self.error = None;
            cx.emit(AutomationEditorEvent::Changed);
            cx.notify();
        }
    }
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
        action: fn(&mut Self, &mut Context<Self>),
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let primary = id == "automation-editor-save";
        div()
            .id(id)
            .debug_selector(move || id.into())
            .role(Role::Button)
            .aria_label(label)
            .track_focus(focus)
            .tab_stop(enabled)
            .h(px(Layout::AUTOMATION_ACTION_SIZE))
            .px_3()
            .flex()
            .items_center()
            .rounded_full()
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .text_color(if primary && enabled {
                colors.bg_base
            } else if enabled {
                colors.text_primary
            } else {
                colors.text_tertiary
            })
            .interaction_surface(
                if primary && enabled {
                    colors.text_primary
                } else {
                    colors.bg_base
                },
                if primary && enabled {
                    colors.text_secondary
                } else {
                    colors.bg_hover
                },
                if primary && enabled {
                    colors.text_secondary
                } else {
                    colors.bg_active
                },
                enabled,
            )
            .focus_visible(move |style| style.border_1().border_color(colors.accent))
            .when(enabled, |button| button.cursor_pointer())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if enabled {
                    cx.stop_propagation();
                    action(this, cx);
                }
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    action(this, cx);
                }
            }))
            .child(label)
            .into_any_element()
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

impl Render for AutomationEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let conflict = self.has_conflict();
        let dirty = self.is_dirty(cx);
        let error = if self.latest.is_none() {
            Some("目标已清除".to_string())
        } else if conflict {
            Some("目标已更新，请载入最新内容后重试".to_string())
        } else if dirty && self.instruction(cx).trim().is_empty() {
            Some("请输入任务内容".to_string())
        } else if self.instruction(cx).len() > 16384 {
            Some("任务内容最长 16 KiB，请缩短后保存".to_string())
        } else {
            self.error.clone()
        };
        let task = self
            .latest
            .as_ref()
            .filter(|task| task.identity == self.base.identity)
            .unwrap_or(&self.base);
        let mut details = vec![
            task.status.label().to_string(),
            format!("执行时间 {}", format_elapsed(self.elapsed_ms)),
        ];
        if task.kind == AutomationKind::Goal {
            details.push(if task.usage_complete {
                format!("已用 {} tokens", task.used_tokens)
            } else {
                format!("用量未知（已记录 {} tokens）", task.used_tokens)
            });
        }
        if let Some(budget) = task.token_budget {
            details.push(format!("预算 {budget} tokens"));
        }
        if task.kind == AutomationKind::Loop {
            details.push(task.interval_seconds.map_or_else(
                || "模型安排下次执行".into(),
                |seconds| format!("间隔 {}", format_elapsed(seconds.saturating_mul(1000))),
            ));
            if let Some(expiry) = task.expires_at_ms {
                details.push(format!("有效期至 {}", format_timestamp(expiry)));
            }
        }
        div()
            .id("automation-editor")
            .debug_selector(|| "automation-editor".into())
            .size_full()
            .flex()
            .flex_col()
            .min_w_0()
            .key_context("AutomationEditor")
            .on_action(cx.listener(|this, _: &SaveAutomation, _, cx| {
                this.save(cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &CloseAutomationEditor, _, cx| {
                cx.emit(AutomationEditorEvent::Close);
                cx.stop_propagation();
            }))
            .text_size(px(Typography::BODY))
            .text_color(colors.text_primary)
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .p_3()
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(colors.text_secondary)
                            .child(if self.pending.is_some() {
                                "保存中…"
                            } else if dirty {
                                "未保存"
                            } else {
                                ""
                            }),
                    )
                    .child(self.button(
                        "automation-editor-revert",
                        if conflict { "载入最新" } else { "还原" },
                        self.pending.is_none()
                            && self.latest.is_some()
                            && (dirty || conflict || self.error.is_some()),
                        &self.revert_focus,
                        cx,
                        Self::revert,
                    ))
                    .child(self.button(
                        "automation-editor-save",
                        "保存",
                        self.can_save(cx),
                        &self.save_focus,
                        cx,
                        Self::save,
                    )),
            )
            .children(error.map(|error| {
                div()
                    .debug_selector(|| "automation-editor-error".into())
                    .flex_shrink_0()
                    .px_3()
                    .pt_2()
                    .text_color(colors.danger)
                    .child(error)
            }))
            .child(
                div()
                    .id("automation-editor-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(colors.text_secondary)
                            .child("任务内容"),
                    )
                    .child(
                        div()
                            .id("automation-editor-input")
                            .debug_selector(|| "automation-editor-input".into())
                            .role(Role::TextInput)
                            .aria_label("任务内容")
                            .w_full()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(Typography::BODY * Typography::BODY_LINE_HEIGHT * 2.))
                            .child(self.input.clone()),
                    )
                    .child(
                        div()
                            .id("automation-editor-details")
                            .min_h_0()
                            .max_h(px(112.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .text_size(px(Typography::METADATA))
                            .text_color(colors.text_secondary)
                            .children(details.into_iter().map(|value| div().child(value))),
                    ),
            )
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

#[cfg(test)]
mod tests;
