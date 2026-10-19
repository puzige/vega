use super::*;
use crate::interaction::InteractionExt;
use crate::scroll_edge_fade::{ScrollFade, ScrollableFadeExt};
use gpui_kit::Role;
use vega_conversation::types::{
    SubagentOpenRequested, SubagentSnapshot, SubagentState, SubagentStopRequested,
};

pub(crate) struct SubagentGroup {
    source: String,
    agents: Vec<SubagentSnapshot>,
    streams: HashMap<String, Entity<ConversationStream>>,
    stream_observers: HashMap<String, gpui_kit::Subscription>,
    expanded: HashSet<String>,
    focus: HashMap<String, [FocusHandle; 3]>,
}

impl EventEmitter<SubagentOpenRequested> for SubagentGroup {}
impl EventEmitter<SubagentStopRequested> for SubagentGroup {}
impl EventEmitter<SubagentOpenRequested> for ConversationStream {}
impl EventEmitter<SubagentStopRequested> for ConversationStream {}

struct SubagentButton {
    id: String,
    label: String,
    accessible_name: String,
    focus: FocusHandle,
    enabled: bool,
    expanded: Option<bool>,
}

impl SubagentGroup {
    fn update(&mut self, snapshot: SubagentSnapshot, cx: &mut Context<Self>) {
        if snapshot.state.is_terminal() {
            if let Some(stream) = self.streams.remove(&snapshot.agent_id) {
                stream.update(cx, |stream, _| stream.delegated_run_active = false);
            }
            self.stream_observers.remove(&snapshot.agent_id);
        }
        self.focus
            .entry(snapshot.agent_id.clone())
            .or_insert_with(|| std::array::from_fn(|_| cx.focus_handle().tab_stop(true)));
        if let Some(agent) = self
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == snapshot.agent_id)
        {
            *agent = snapshot;
        } else {
            self.agents.push(snapshot);
        }
        cx.notify();
    }
    fn bind(&mut self, id: &str, stream: Entity<ConversationStream>, cx: &mut Context<Self>) {
        if self.streams.get(id) == Some(&stream) {
            return;
        }
        let observer = cx.observe(&stream, |_, _, cx| cx.notify());
        stream.update(cx, |stream, _| stream.delegated_run_active = true);
        self.stream_observers.insert(id.into(), observer);
        self.streams.insert(id.into(), stream);
        cx.notify();
    }
    pub(crate) fn row_count(&self) -> usize {
        self.agents.len().saturating_add(1)
    }
    fn button(
        &self,
        button: SubagentButton,
        cx: &mut Context<Self>,
        activate: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let colors = theme(cx).colors;
        let action = std::rc::Rc::new(activate);
        let keyboard = action.clone();
        let SubagentButton {
            id,
            label,
            accessible_name,
            focus,
            enabled,
            expanded,
        } = button;
        let button = div()
            .id(id.clone())
            .role(Role::Button)
            .aria_label(accessible_name)
            .when_some(expanded, |button, expanded| button.aria_expanded(expanded));
        #[cfg(test)]
        let button = gpui_kit::test::TestSupportExt::test_support(button);
        button
            .track_focus(&focus)
            .tab_stop(enabled)
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .px_2()
            .py_1()
            .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
            .text_size(px(Typography::BODY))
            .text_color(colors.text_secondary)
            .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, enabled)
            .focus_visible(move |style| style.border_color(colors.accent))
            .when(enabled, |button| button.cursor_pointer())
            .on_click(cx.listener(move |this, _, _, cx| {
                if enabled {
                    cx.stop_propagation();
                    action(this, cx);
                }
            }))
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if enabled && matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        keyboard(this, cx);
                    }
                }),
            )
            .flex()
            .items_center()
            .gap_1()
            .when_some(expanded, |button, expanded| {
                button.child(crate::icons::icon(
                    if expanded {
                        crate::icons::Icon::ChevronDown
                    } else {
                        crate::icons::Icon::ChevronRight
                    },
                    colors.text_secondary,
                ))
            })
            .child(label)
            .into_any_element()
    }
}

impl Render for SubagentGroup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let mut group = div().w_full().min_w_0().flex().flex_col().py_1().gap_1();
        for agent in &self.agents {
            let Some(focus) = self.focus.get(&agent.agent_id) else {
                continue;
            };
            let id = agent.agent_id.clone();
            let open_id = id.clone();
            let stop_id = id.clone();
            let run = agent.message_id.clone();
            let detail_id = id.clone();
            let status = match agent.state {
                SubagentState::Running => "Running",
                SubagentState::Cancelling => "Stopping",
                SubagentState::Completed => "Completed",
                SubagentState::Failed => "Failed",
                SubagentState::Interrupted => "Interrupted",
            };
            let mut row = div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(self.button(
                    SubagentButton {
                        id: format!("agent-{id}-result"),
                        label: agent.name.clone(),
                        accessible_name: format!("{} result", agent.name),
                        focus: focus[0].clone(),
                        enabled: !agent.result.is_empty(),
                        expanded: (!agent.result.is_empty()).then_some(self.expanded.contains(&id)),
                    },
                    cx,
                    move |this, cx| {
                        if !this.expanded.remove(&detail_id) {
                            this.expanded.insert(detail_id.clone());
                        }
                        cx.notify();
                    },
                ))
                .child(
                    div()
                        .text_size(px(Typography::METADATA))
                        .text_color(if agent.state == SubagentState::Failed {
                            colors.danger
                        } else {
                            colors.text_secondary
                        })
                        .child(status),
                )
                .child(self.button(
                    SubagentButton {
                        id: format!("agent-{id}-open"),
                        label: "Open".into(),
                        accessible_name: format!("Open {}", agent.name),
                        focus: focus[1].clone(),
                        enabled: true,
                        expanded: None,
                    },
                    cx,
                    move |this, cx| {
                        cx.emit(SubagentOpenRequested {
                            source_thread_id: this.source.clone(),
                            thread_id: open_id.clone(),
                        })
                    },
                ));
            if !agent.state.is_terminal() {
                row = row.child(self.button(
                    SubagentButton {
                        id: format!("agent-{id}-stop"),
                        label: "Stop".into(),
                        accessible_name: format!("Stop {}", agent.name),
                        focus: focus[2].clone(),
                        enabled: agent.state == SubagentState::Running,
                        expanded: None,
                    },
                    cx,
                    move |this, cx| {
                        cx.emit(SubagentStopRequested {
                            source_thread_id: this.source.clone(),
                            thread_id: stop_id.clone(),
                            message_id: run.clone(),
                        })
                    },
                ));
            }
            group = group.child(row);
            if self.expanded.contains(&id) && !agent.result.is_empty() {
                group = group.child(
                    div()
                        .id(format!("agent-{id}-result-body"))
                        .max_h(px(Layout::DISCLOSURE_CONTENT_MAX_HEIGHT))
                        .overflow_y_scroll()
                        .px_2()
                        .text_size(px(Typography::BODY))
                        .text_color(if agent.state == SubagentState::Failed {
                            colors.danger
                        } else {
                            colors.text_primary
                        })
                        .child(agent.result.clone())
                        .when(agent.truncated, |view| {
                            view.child(
                                div()
                                    .text_color(colors.text_secondary)
                                    .child("Result truncated. Open conversation for full details."),
                            )
                        })
                        .scroll_edge_fade(colors.bg_base, ScrollFade::compact()),
                );
            }
            if let Some(stream) = self.streams.get(&id) {
                let permission = stream.read(cx).active_permission.clone();
                let question = stream.read(cx).active_user_question.clone();
                if let Some(card) = permission {
                    group =
                        group.child(render_entry(&StreamEntry::Permission { card }, window, cx));
                }
                group = group.children(question);
            }
        }
        group
    }
}

impl ConversationStream {
    pub fn apply_subagent_usage(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let usage = match event {
            ConversationEvent::UsageUpdated {
                usage,
                cost,
                pricing,
                ..
            } => Some(ConversationEvent::ContextCompactionUsageUpdated {
                usage: *usage,
                cost: Some(*cost),
                pricing: pricing.clone(),
            }),
            ConversationEvent::ContextCompactionUsageUpdated { .. }
            | ConversationEvent::ContextCompactionStatus { .. } => Some(event.clone()),
            _ => None,
        };
        if usage.as_ref().is_some_and(|event| self.meter.apply(event)) {
            cx.notify();
        }
    }

    pub fn finish_subagent_usage(&mut self, complete: bool, cx: &mut Context<Self>) {
        if !complete {
            self.meter.restore_unknown_context_usage();
            cx.notify();
        }
    }

    pub fn mark_subagent_cancelling(&mut self, id: &str, run: &str, cx: &mut Context<Self>) {
        for group in self.subagent_groups.values() {
            group.update(cx, |group, cx| {
                let snapshot = group
                    .agents
                    .iter()
                    .find(|agent| {
                        agent.agent_id == id
                            && agent.message_id == run
                            && agent.state == SubagentState::Running
                    })
                    .cloned();
                if let Some(mut snapshot) = snapshot {
                    snapshot.state = SubagentState::Cancelling;
                    group.update(snapshot, cx);
                }
            });
        }
    }

    pub fn apply_subagent(&mut self, snapshot: SubagentSnapshot, cx: &mut Context<Self>) {
        if snapshot.parent_thread_id != self.thread.id {
            return;
        }
        let owner = snapshot.parent_message_id.clone();
        let group = if let Some(group) = self.subagent_groups.get(&owner) {
            group.clone()
        } else {
            let source = self.thread.id.clone();
            let group = cx.new(|_| SubagentGroup {
                source,
                agents: Vec::new(),
                streams: HashMap::new(),
                stream_observers: HashMap::new(),
                expanded: HashSet::new(),
                focus: HashMap::new(),
            });
            cx.observe(&group,|this,group,cx| {
                let index = this.entry_index_where(|entry| matches!(entry, StreamEntry::Subagents {group:owned} if owned == &group));
                this.invalidate_item(index); cx.notify();
            }).detach();
            cx.subscribe(&group, |_, _, event: &SubagentOpenRequested, cx| {
                cx.emit(event.clone())
            })
            .detach();
            cx.subscribe(&group, |_, _, event: &SubagentStopRequested, cx| {
                cx.emit(event.clone())
            })
            .detach();
            self.ensure_entry_identities();
            let index = self
                .entry_identities
                .iter()
                .rposition(|identity| identity.message_id.as_deref() == Some(&owner))
                .map_or(self.entries.len(), |index| index + 1);
            self.entries.insert(
                index,
                StreamEntry::Subagents {
                    group: group.clone(),
                },
            );
            let identity = self.durable_entry_identity(&owner, "subagents", None);
            self.entry_identities.insert(index, identity);
            self.grow_entry_indices(index, 1);
            self.list.splice(index..index, 1);
            self.subagent_groups.insert(owner.clone(), group.clone());
            group
        };
        group.update(cx, |group, cx| group.update(snapshot, cx));
        cx.notify();
    }
    pub fn bind_subagent_stream(
        &mut self,
        parent_message: &str,
        id: &str,
        stream: Entity<ConversationStream>,
        cx: &mut Context<Self>,
    ) {
        if let Some(group) = self.subagent_groups.get(parent_message) {
            group.update(cx, |group, cx| group.bind(id, stream, cx));
        }
    }
    pub(crate) fn render_subagent_parent(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let parent = self.thread.parent_thread_id.clone()?;
        let colors = theme(cx).colors;
        let activate = std::rc::Rc::new(move |this: &mut Self, cx: &mut Context<Self>| {
            cx.emit(SubagentOpenRequested {
                source_thread_id: this.thread.id.clone(),
                thread_id: parent.clone(),
            })
        });
        let keyboard = activate.clone();
        Some(
            div()
                .id("subagent-parent")
                .role(Role::Button)
                .aria_label("Back to parent")
                .track_focus(&self.subagent_parent_focus)
                .tab_stop(true)
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .px_2()
                .py_1()
                .rounded(px(Layout::SETTINGS_CONTROL_RADIUS))
                .text_size(px(Typography::BODY))
                .text_color(colors.text_secondary)
                .interaction_surface(colors.bg_base, colors.bg_hover, colors.bg_active, true)
                .focus_visible(move |style| style.border_color(colors.accent))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    activate(this, cx);
                }))
                .on_key_down(
                    cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            cx.stop_propagation();
                            keyboard(this, cx);
                        }
                    }),
                )
                .child("Back to parent")
                .into_any_element(),
        )
    }
}
