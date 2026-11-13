use super::draft::{Draft, Value};
use super::measurement::{MeasurementLease, Viewport};
use super::{COMMIT_ROW_HEIGHT, CommitOperationId, CommitPanelFocus, CommitPanelStage};
use crate::nsh::{OrderedCommands, ProjectionId, color};
use nsh_core::prelude::*;
use nsh_core::{Checked, element::input};
use std::collections::HashSet;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;
use vega_conversation::types::{IndexSnapshotId, PreparedCommit, WorkspaceFileId};
use vega_theme::{ThemeColors, Typography};

#[derive(Clone)]
pub(super) struct Row {
    pub id: WorkspaceFileId,
    pub label: String,
    pub forced: bool,
    pub selected: bool,
    pub optional: Option<usize>,
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub page: ProjectionId,
    pub thread: String,
    pub project: String,
    pub stage: CommitPanelStage,
    pub pending: Option<CommitOperationId>,
    pub checklist: Option<IndexSnapshotId>,
    pub generation: Option<u64>,
    pub prepared: Option<PreparedCommit>,
    pub selected: HashSet<WorkspaceFileId>,
    pub disabled: bool,
    pub focus: CommitPanelFocus,
    pub focus_revision: u64,
    pub rows: Arc<[Row]>,
    pub draft: Draft,
    pub draft_revision: u64,
    pub draft_text: String,
    pub draft_overflow: bool,
    pub failure: bool,
    pub colors: ThemeColors,
}

impl Snapshot {
    pub fn same_authority(&self, other: &Self) -> bool {
        self.draft_revision == other.draft_revision && self.same_focus_authority(other)
    }

    pub(super) fn same_focus_authority(&self, other: &Self) -> bool {
        self.page.matches(&other.page)
            && self.thread == other.thread
            && self.project == other.project
            && self.stage == other.stage
            && self.pending == other.pending
            && self.checklist == other.checklist
            && self.generation == other.generation
            && self.prepared == other.prepared
            && self.selected == other.selected
            && self.disabled == other.disabled
            && self.focus == other.focus
            && self.focus_revision == other.focus_revision
            && self.failure == other.failure
            && self.draft_overflow == other.draft_overflow
    }

    pub fn with_current_draft<R>(
        &self,
        displayed: &Self,
        fresh: &Self,
        apply: impl FnOnce(&mut Value) -> R,
    ) -> Option<R> {
        let mut draft = self.draft.0.lock().ok()?;
        if !self.same_authority(displayed)
            || !self.same_authority(fresh)
            || !draft.owns(&self.page)
            || draft.composing
            || draft.overflow
            || self.draft_revision != draft.revision
            || self.draft_text != draft.text
        {
            return None;
        }
        Some(apply(&mut draft))
    }

    pub(super) fn panel_height(&self) -> f32 {
        // The checklist keeps its own scroll position after six visible rows.
        // Leave room for the multiline draft and the prepared-commit actions.
        let controls = if self.prepared.is_some() { 324. } else { 140. };
        controls + COMMIT_ROW_HEIGHT * self.rows.len().min(6) as f32
    }

    pub fn actionable(&self) -> bool {
        !self.disabled && !self.failure && self.pending.is_none()
    }

    pub fn editable(&self) -> bool {
        !self.disabled
            && !self.failure
            && matches!(
                self.stage,
                CommitPanelStage::CommitReady | CommitPanelStage::Drafting
            )
    }

    pub fn can_confirm(&self) -> bool {
        self.actionable()
            && match self.stage {
                CommitPanelStage::Checklist => {
                    !self.selected.is_empty() || self.rows.iter().any(|row| row.forced)
                }
                CommitPanelStage::CommitReady => true,
                _ => false,
            }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Action {
    Cancel,
    Toggle(WorkspaceFileId),
    Generate,
    Confirm,
    Recover,
}

pub(super) enum Request {
    Displayed(Snapshot),
    Edit(ProjectionId),
    Focused(Snapshot),
    TraverseDraft(Snapshot, bool),
    Measured(Snapshot, Option<[f32; 4]>, MeasurementLease),
    Action(Snapshot, Action, Option<Weak<()>>),
}

pub(super) fn button(
    viewport: Viewport,
    observed: Snapshot,
    commands: OrderedCommands<Request>,
    label: &'static str,
    action: Action,
    enabled: bool,
    focused: bool,
) -> AnyElement {
    let colors = observed.colors;
    let target = element_ref();
    let lease = signal_with(Arc::new(()), SignalEquals::Never);
    if focused {
        viewport.follow(
            target,
            None,
            observed.clone(),
            commands.clone(),
            Arc::downgrade(&lease.get_untracked()),
        );
    }
    view()
        .element_ref(&target)
        .on_layout(move |_: &Bounds<Points>| {
            if focused {
                viewport.invalidate();
            }
        })
        .role(AccessibilityRole::Button)
        .label(label)
        .accessibility_state(AccessibilityState {
            disabled: !enabled,
            busy: observed.pending.is_some(),
            ..Default::default()
        })
        .disabled(!enabled)
        .border(px(1.))
        .border_color(color(if focused {
            colors.accent
        } else {
            colors.border_subtle
        }))
        .bg(color(if focused {
            colors.bg_hover
        } else {
            colors.bg_elevated
        }))
        .rounded(px(6.))
        .px_3()
        .py_2()
        .text_color(color(if enabled {
            colors.text_primary
        } else {
            colors.text_tertiary
        }))
        .child(text(label))
        .on_press(move || {
            if !enabled {
                return;
            }
            commands.send(Request::Action(observed.clone(), action, None));
        })
        .into_element()
}

pub(super) fn recovery(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let viewport = Viewport::new(state);
    let commands = OrderedCommands::new(sender);
    let displayed = commands.clone();
    effect(move || displayed.send(Request::Displayed(state.get())));
    let content = dynamic(move || {
        let observed = state.get();
        view()
            .flex_col()
            .gap_3()
            .p_4()
            .w_full()
            .bg(color(observed.colors.bg_base))
            .child(text(vega_i18n::t("ui.commit_changes")))
            .child(
                text(vega_i18n::t_args(
                    "ui.commit_unavailable_code",
                    &[("code", "UI")],
                ))
                .text_color(color(observed.colors.danger)),
            )
            .child(button(
                viewport,
                observed.clone(),
                commands.clone(),
                vega_i18n::t("settings.action.reload"),
                Action::Recover,
                true,
                observed.focus == CommitPanelFocus::Confirm,
            ))
            .child(button(
                viewport,
                observed.clone(),
                commands.clone(),
                vega_i18n::t("ui.cancel"),
                Action::Cancel,
                true,
                observed.focus == CommitPanelFocus::Cancel,
            ))
            .into_element()
    });
    scroll()
        .size_full()
        .element_ref(&viewport.root)
        .on_layout(move |bounds: &Bounds<Points>| viewport.resized(bounds, false))
        .on_scroll(move |e: &ScrollEvent| viewport.scrolled(e.offset.y.0, false))
        .child(content)
        .into_element()
}

pub(super) fn panel(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let viewport = Viewport::new(state);
    let commands = OrderedCommands::new(sender);
    let displayed = commands.clone();
    effect(move || displayed.send(Request::Displayed(state.get())));
    let message = signal(String::new());
    let composing = signal(false);
    let local_revision = signal(None::<u64>);
    let input_ref = element_ref();
    let list_ref = element_ref();
    viewport.follow_input(input_ref);
    let focused_revision = signal(None::<u64>);
    effect(move || {
        let observed = state.get();
        if let Ok(draft) = observed.draft.0.lock()
            && !draft.composing
            && !composing.get()
            && draft.revision == observed.draft_revision
            && draft
                .page
                .as_ref()
                .is_some_and(|page| page.matches(&observed.page))
        {
            message.set(observed.draft_text.clone());
            local_revision.set(Some(observed.draft_revision));
        }
        if focused_revision.get() != Some(observed.focus_revision) {
            if observed.focus == CommitPanelFocus::Draft && input_ref.node().is_some() {
                input_ref.focus();
                focused_revision.set(Some(observed.focus_revision));
            } else if observed.focus != CommitPanelFocus::Draft {
                focused_revision.set(Some(observed.focus_revision));
            }
            if let CommitPanelFocus::Optional(index) = observed.focus {
                let staged = observed.rows.iter().take_while(|row| row.forced).count();
                list_ref.scroll_to_offset(px((staged + index) as f32 * COMMIT_ROW_HEIGHT), false);
            }
        }
    });
    let rows = commands.clone();
    let footer = commands.clone();
    let changes = commands.clone();
    let compositions = commands.clone();
    let focused = commands.clone();
    let input_keys = commands.clone();
    let content = view()
        .w_full()
        .h(move || px(state.get().panel_height()))
        .flex_col()
        .gap_3()
        .p_4()
        .bg(move || color(state.get().colors.bg_base))
        .text_color(move || color(state.get().colors.text_primary))
        .text_size(px(Typography::BODY))
        .child(dynamic(move || {
            let observed = state.get();
            let send = commands.clone();
            if observed.focus == CommitPanelFocus::Draft && observed.editable() {
                for (key, backwards) in [("tab", false), ("shift+tab", true)] {
                    let displayed = observed.clone();
                    let sender = commands.clone();
                    on_key_with(
                        key,
                        KeyOptions {
                            when: nsh_core::keymap::When::Always,
                            repeat: false,
                            pass_through: false,
                        },
                        move |_| sender.send(Request::TraverseDraft(displayed.clone(), backwards)),
                    );
                }
            }
            if observed.failure || send.failed.get() {
                return view()
                    .flex_col()
                    .child(button(
                        viewport,
                        observed.clone(),
                        send.clone(),
                        vega_i18n::t("settings.action.reload"),
                        Action::Recover,
                        true,
                        false,
                    ))
                    .child(button(
                        viewport,
                        observed.clone(),
                        send,
                        vega_i18n::t("ui.cancel"),
                        Action::Cancel,
                        true,
                        observed.focus == CommitPanelFocus::Cancel,
                    ))
                    .into_element();
            }
            let status = match observed.stage {
                CommitPanelStage::Loading
                | CommitPanelStage::Preparing
                | CommitPanelStage::Drafting
                | CommitPanelStage::Committing => Some(vega_i18n::t("ui.loading")),
                _ => None,
            };
            let mut header = view().flex_col().gap_2().child(
                view()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        text(vega_i18n::t("ui.commit_changes"))
                            .text_size(px(Typography::HEADING_BLOCK)),
                    )
                    .child(button(
                        viewport,
                        observed.clone(),
                        send,
                        vega_i18n::t("ui.cancel"),
                        Action::Cancel,
                        true,
                        observed.focus == CommitPanelFocus::Cancel,
                    )),
            );
            if let Some(status) = status {
                header = header.child(
                    text(status)
                        .role(AccessibilityRole::Progressbar)
                        .accessibility_state(AccessibilityState {
                            busy: true,
                            ..Default::default()
                        }),
                );
            }
            if let CommitPanelStage::Failed(code) = observed.stage {
                header = header.child(
                    text(vega_i18n::t_args(
                        "ui.commit_unavailable_code",
                        &[("code", super::panel::commit_error_label(code))],
                    ))
                    .text_color(color(observed.colors.danger)),
                );
            }
            header.into_element()
        }))
        .child(
            virtual_list(
                move || state.get().rows.len(),
                move |position| {
                    let send = rows.clone();
                    dynamic(move || {
                        let observed = state.get();
                        let Some(row) = observed.rows.get(position.index()).cloned() else {
                            return view().into_element();
                        };
                        let lease = signal_with(Arc::new(()), SignalEquals::Never);
                        let weak = Arc::downgrade(&lease.get_untracked());
                        let enabled = observed.actionable()
                            && observed.stage == CommitPanelStage::Checklist
                            && super::commit_row_is_focusable(row.forced);
                        let focused = row.optional.is_some_and(|index| {
                            observed.focus == CommitPanelFocus::Optional(index)
                        });
                        let send = send.clone();
                        let staged = observed.rows.iter().take_while(|row| row.forced).count();
                        let target = element_ref();
                        if focused {
                            viewport.follow(
                                target,
                                Some(list_ref),
                                observed.clone(),
                                send.clone(),
                                weak.clone(),
                            );
                        }
                        view()
                            .element_ref(&target)
                            .on_layout(move |_: &Bounds<Points>| {
                                if focused {
                                    viewport.invalidate();
                                }
                            })
                            .native_id(&super::commit_row_key(
                                observed.generation.unwrap_or_default(),
                                position.index(),
                                staged,
                            ))
                            .role(AccessibilityRole::Checkbox)
                            .label(format!(
                                "{} {}",
                                super::commit_row_status(row.forced, row.selected),
                                row.label
                            ))
                            .accessibility_state(AccessibilityState {
                                checked: Some(if row.forced || row.selected {
                                    Checked::True
                                } else {
                                    Checked::False
                                }),
                                disabled: !enabled,
                                ..Default::default()
                            })
                            .disabled(!enabled)
                            .h(px(COMMIT_ROW_HEIGHT))
                            .px_2()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .border(px(if focused { 1. } else { 0. }))
                            .border_color(color(observed.colors.accent))
                            .text_size(px(Typography::CODE))
                            .text_color(color(if row.forced {
                                observed.colors.text_tertiary
                            } else {
                                observed.colors.text_primary
                            }))
                            .child(text(if row.forced || row.selected {
                                "●"
                            } else {
                                "○"
                            }))
                            .child(
                                text(super::commit_row_status(row.forced, row.selected))
                                    .text_color(color(observed.colors.text_tertiary)),
                            )
                            .child(text(row.label).number_of_lines(1).flex_1().min_w(px(0.)))
                            .on_press(move || {
                                if enabled && weak.upgrade().is_some() {
                                    send.send(Request::Action(
                                        observed.clone(),
                                        Action::Toggle(row.id),
                                        Some(weak.clone()),
                                    ));
                                }
                            })
                            .into_element()
                    })
                },
            )
            .element_ref(&list_ref)
            .on_layout(move |bounds: &Bounds<Points>| viewport.resized(bounds, true))
            .on_scroll(move |e: &ScrollEvent| viewport.scrolled(e.offset.y.0, true))
            .estimated_item_size(px(COMMIT_ROW_HEIGHT))
            .flex_1()
            .min_h(px(0.))
            .w_full(),
        )
        .child(when(
            move || {
                matches!(
                    state.get().stage,
                    CommitPanelStage::CommitReady
                        | CommitPanelStage::Drafting
                        | CommitPanelStage::Committing
                )
            },
            move || {
                let changes = changes.clone();
                let compositions = compositions.clone();
                let focused = focused.clone();
                let input_keys = input_keys.clone();
                view()
                    .flex_col()
                    .gap_2()
                    .child(
                        input()
                            .element_ref(&input_ref)
                            .label(vega_i18n::t(
                                "ui.commit_message_enter_newline_cmd_enter_commit",
                            ))
                            .placeholder(vega_i18n::t("ui.commit_message_placeholder"))
                            .multiline(true)
                            .submit_behavior(SubmitBehavior::Newline)
                            .value(move || message.get())
                            .readonly(move || !state.get().editable())
                            .h(px(112.))
                            .w_full()
                            .border(px(1.))
                            .border_color(move || {
                                color(if state.get().focus == CommitPanelFocus::Draft {
                                    state.get().colors.accent
                                } else {
                                    state.get().colors.border_subtle
                                })
                            })
                            .rounded(px(6.))
                            .p_2()
                            .on_layout(move |_: &Bounds<Points>| {
                                if state.get_untracked().focus == CommitPanelFocus::Draft {
                                    input_ref.focus();
                                }
                            })
                            .on_focus(move || focused.send(Request::Focused(state.get_untracked())))
                            .on_key_press(move |key: &str| {
                                if key == "Escape" {
                                    input_keys.send(Request::Action(
                                        state.get_untracked(),
                                        Action::Cancel,
                                        None,
                                    ));
                                }
                            })
                            .on_change(move |value: &str| {
                                let observed = state.get_untracked();
                                if let Ok(mut draft) = observed.draft.0.lock()
                                    && draft.owns(&observed.page)
                                    && draft.editable
                                    && observed.editable()
                                    && local_revision.get_untracked() == Some(draft.revision)
                                {
                                    draft.replace(value);
                                    local_revision.set(Some(draft.revision));
                                    message.set(value.to_owned());
                                    changes.send(Request::Edit(observed.page.clone()));
                                }
                            })
                            .on_composition_change(move |event: &Composition| {
                                let observed = state.get_untracked();
                                composing.set(event.composing);
                                if let Ok(mut draft) = observed.draft.0.lock()
                                    && draft.owns(&observed.page)
                                    && draft.editable
                                {
                                    let locally_current =
                                        local_revision.get_untracked() == Some(draft.revision);
                                    if event.composing && !draft.composing {
                                        match draft.revision.checked_add(1) {
                                            Some(revision) => draft.revision = revision,
                                            None => draft.overflow = true,
                                        }
                                    }
                                    if locally_current {
                                        local_revision.set(Some(draft.revision));
                                    }
                                    draft.composing = event.composing;
                                    compositions.send(Request::Edit(observed.page.clone()));
                                }
                            }),
                    )
                    .into_element()
            },
        ))
        .child(dynamic(move || {
            let observed = state.get();
            let send = footer.clone();
            let mut actions = view().flex_col().gap_2();
            if let Some(prepared) = &observed.prepared {
                actions = actions.child(text(vega_i18n::t_args(
                    "ui.commit_staged_files",
                    &[("count", &prepared.staged_file_count.to_string())],
                )));
                if prepared.summary_truncated {
                    actions = actions.child(text("…"));
                }
                actions = actions.child(button(
                    viewport,
                    observed.clone(),
                    send.clone(),
                    vega_i18n::t("ui.generate_message"),
                    Action::Generate,
                    observed.actionable() && observed.stage == CommitPanelStage::CommitReady,
                    observed.focus == CommitPanelFocus::Generate,
                ));
            }
            actions
                .child(button(
                    viewport,
                    observed.clone(),
                    send,
                    if observed.prepared.is_some() {
                        vega_i18n::t("ui.commit")
                    } else {
                        vega_i18n::t("ui.prepare")
                    },
                    Action::Confirm,
                    observed.can_confirm(),
                    observed.focus == CommitPanelFocus::Confirm,
                ))
                .into_element()
        }));
    scroll()
        .size_full()
        .element_ref(&viewport.root)
        .on_layout(move |bounds: &Bounds<Points>| viewport.resized(bounds, false))
        .on_scroll(move |e: &ScrollEvent| viewport.scrolled(e.offset.y.0, false))
        .child(content)
        .into_element()
}
