use super::*;
use crate::interaction::InteractionExt;
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
        div()
            .id(id.clone())
            .debug_selector(move || id.clone())
            .role(Role::Button)
            .aria_label(accessible_name)
            .when_some(expanded, |button, expanded| button.aria_expanded(expanded))
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
        let mut group = div()
            .debug_selector(|| "subagent-group".into())
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .py_1()
            .gap_1();
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
                .debug_selector(|| "subagent-row".into())
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
                        .debug_selector(|| "subagent-result-body".into())
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
                        }),
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
                .debug_selector(|| "subagent-parent".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use tokio_util::sync::CancellationToken;
    use vega_conversation::agent::{PermissionHook, PermissionQueue, UserQuestionQueue};
    use vega_conversation::types::{Approval, PermissionDecision, PermissionRequest, ToolCall};
    use vega_runtime::{
        RuntimeUserQuestionHook, RuntimeUserQuestionRequest, RuntimeUserQuestionResponse,
        UserQuestionInput,
    };

    fn thread(id: &str) -> Thread {
        Thread {
            id: id.into(),
            parent_thread_id: (id != "parent").then(|| "parent".into()),
            backend: TaskBackend::Native,
            project_id: "project".into(),
            title: id.into(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: "mock".into(),
            status: vega_conversation::types::ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn snapshot(id: &str) -> SubagentSnapshot {
        SubagentSnapshot {
            agent_id: id.into(),
            parent_thread_id: "parent".into(),
            parent_message_id: "parent-run".into(),
            message_id: format!("{id}-run"),
            name: format!("Research {id}"),
            state: SubagentState::Running,
            result: String::new(),
            truncated: false,
        }
    }

    fn fixture(
        cx: &mut TestAppContext,
    ) -> (
        WindowHandle<ConversationStream>,
        Entity<ConversationStream>,
        [Entity<ConversationStream>; 2],
        [PermissionQueue; 2],
    ) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SettingsOpen(false));
            crate::init(cx);
        });
        let parent = cx.new(|cx| ConversationStream::new(thread("parent"), cx));
        let queues = [PermissionQueue::new(), PermissionQueue::new()];
        let children = std::array::from_fn(|index| {
            cx.new(|cx| {
                ConversationStream::new_with_request_queues(
                    thread(if index == 0 { "a" } else { "b" }),
                    queues[index].clone(),
                    UserQuestionQueue::new(),
                    cx,
                )
            })
        });
        parent.update(cx, |parent, cx| {
            parent.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "parent-run".into(),
                    seq: 1,
                },
                cx,
            );
            for (id, child) in ["a", "b"].into_iter().zip(&children) {
                parent.apply_subagent(snapshot(id), cx);
                parent.bind_subagent_stream("parent-run", id, child.clone(), cx);
            }
        });
        let view = parent.clone();
        let window = cx
            .update(|cx| cx.open_window(Default::default(), move |_, _| view))
            .unwrap();
        cx.run_until_parked();
        (window, parent, children, queues)
    }

    fn propose(child: &Entity<ConversationStream>, call: ToolCall, cx: &mut TestAppContext) {
        child.update(cx, |child, cx| {
            let question = call.tool == "ask_user_question";
            child.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "child-message".into(),
                    seq: 1,
                },
                cx,
            );
            child.apply_event(
                ConversationEvent::ToolCallProposed { call: call.clone() },
                cx,
            );
            if question {
                child.apply_event(
                    ConversationEvent::ToolCallApproved {
                        call_id: call.id.clone(),
                        approval: Approval::Once,
                    },
                    cx,
                );
                child.apply_event(ConversationEvent::ToolCallRunning { call_id: call.id }, cx);
            }
        });
    }

    #[gpui_kit::test]
    async fn m11_two_child_permissions_share_the_original_responder_and_stop_is_scoped(
        cx: &mut TestAppContext,
    ) {
        let (window, parent, children, queues) = fixture(cx);
        let mut waits = Vec::new();
        for (index, id) in ["a", "b"].into_iter().enumerate() {
            let call_id = format!("permission-{id}");
            propose(
                &children[index],
                ToolCall {
                    id: call_id.clone(),
                    tool: "bash".into(),
                    input_json: r#"{"cmd":"printf synthetic"}"#.into(),
                },
                cx,
            );
            waits.push(queues[index].request(
                PermissionRequest {
                    call_id,
                    tool: "bash".into(),
                    display_target: "printf synthetic".into(),
                    danger_rule_id: None,
                    danger_reason: None,
                    external: None,
                    acp_options: None,
                },
                CancellationToken::new(),
            ));
        }
        cx.run_until_parked();
        assert!(
            children
                .iter()
                .all(|child| child.read_with(cx, |child, _| child.has_active_permission_card()))
        );
        assert_eq!(
            parent.read_with(cx, |parent, cx| parent.subagent_groups["parent-run"]
                .read(cx)
                .streams
                .len()),
            2
        );
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("conversation-permission-card")
                .is_some()
        );
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        assert!(queues[0].has_pending());
        assert!(!queues[1].has_pending());
        assert_eq!(
            waits.pop().unwrap().await.unwrap(),
            PermissionDecision::Once
        );
        children[0].update(cx, |child, cx| child.timeout_permission(cx));
        assert_eq!(
            waits.pop().unwrap().await.unwrap(),
            PermissionDecision::Timeout
        );
        assert!(!queues[0].has_pending());
    }

    #[gpui_kit::test]
    async fn m11_m14_two_child_questions_route_once_and_terminal_group_unbinds_later_requests(
        cx: &mut TestAppContext,
    ) {
        let (window, parent, children, _) = fixture(cx);
        let queues = children
            .each_ref()
            .map(|child| child.read_with(cx, |child, _| child.user_question_queue()));
        let mut waits = Vec::new();
        for (index, id) in ["a", "b"].into_iter().enumerate() {
            let request = RuntimeUserQuestionRequest {call_id:format!("question-{id}"),input:UserQuestionInput::from_json(r#"{"questions":[{"id":"choice","header":"Choice","question":"Choose a result","options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]}]}"#).unwrap()};
            propose(
                &children[index],
                ToolCall {
                    id: request.call_id.clone(),
                    tool: "ask_user_question".into(),
                    input_json: request.input.to_json().unwrap(),
                },
                cx,
            );
            waits.push(queues[index].request(request, CancellationToken::new()));
        }
        cx.run_until_parked();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        let option = visual.debug_bounds("question-option-0").unwrap();
        visual.simulate_click(option.center(), Modifiers::default());
        visual.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        assert!(queues[0].has_pending());
        assert!(!queues[1].has_pending());
        assert!(matches!(
            waits.pop().unwrap().await,
            RuntimeUserQuestionResponse::Answered(_)
        ));
        children[0].update(cx, |child, cx| child.cancel_user_questions(cx));
        assert!(matches!(
            waits.pop().unwrap().await,
            RuntimeUserQuestionResponse::Cancelled
        ));
        parent.update(cx, |parent, cx| {
            for id in ["a", "b"] {
                let mut agent = snapshot(id);
                agent.state = SubagentState::Completed;
                agent.result = "Original result".into();
                parent.apply_subagent(agent, cx);
            }
        });
        assert!(parent.read_with(cx, |parent, cx| {
            parent.subagent_groups["parent-run"]
                .read(cx)
                .streams
                .is_empty()
        }));
        let request = RuntimeUserQuestionRequest {call_id:"later-user-question".into(),input:UserQuestionInput::from_json(r#"{"questions":[{"id":"later","header":"Later","question":"Continue?","options":[{"label":"A","description":"Yes"},{"label":"B","description":"No"}]}]}"#).unwrap()};
        propose(
            &children[0],
            ToolCall {
                id: request.call_id.clone(),
                tool: "ask_user_question".into(),
                input_json: request.input.to_json().unwrap(),
            },
            cx,
        );
        let later = queues[0].request(request, CancellationToken::new());
        cx.run_until_parked();
        assert!(children[0].read_with(cx, |child, _| child.has_pending_user_question()));
        assert!(
            VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("question-option-0")
                .is_none()
        );
        children[0].update(cx, |child, cx| child.cancel_user_questions(cx));
        drop(later);
    }

    #[gpui_kit::test]
    async fn m18_group_result_keyboard_controls_and_layout_follow_real_height(
        cx: &mut TestAppContext,
    ) {
        let (window, parent, _, _) = fixture(cx);
        parent.update(cx, |parent, cx| {
            let mut agent = snapshot("a");
            agent.state = SubagentState::Failed;
            agent.result = "Synthetic failure\nActionable details\nMore details".repeat(3);
            parent.apply_subagent(agent, cx);
        });
        let opened = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let stopped = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        parent.update(cx, |_, cx| {
            let opened = opened.clone();
            cx.subscribe(&parent, move |_, _, event: &SubagentOpenRequested, _| {
                opened.lock().unwrap().push(event.thread_id.clone())
            })
            .detach();
            let stopped = stopped.clone();
            cx.subscribe(&parent, move |_, _, event: &SubagentStopRequested, _| {
                stopped.lock().unwrap().push(event.thread_id.clone())
            })
            .detach();
        });
        for palette in [vega_theme::Theme::light(), vega_theme::Theme::dark()] {
            cx.update(|cx| cx.set_global(palette));
            for width in [960., 480.] {
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                visual.simulate_resize(gpui_kit::size(px(width), px(900.)));
                visual.run_until_parked();
                let before = visual.debug_bounds("subagent-group").unwrap();
                parent.update(&mut visual, |parent, cx| {
                    let group = parent.subagent_groups["parent-run"].clone();
                    group.update(cx, |group, cx| {
                        group.expanded.remove("a");
                        cx.notify();
                    });
                });
                visual.run_until_parked();
                let collapsed = visual.debug_bounds("subagent-group").unwrap();
                let focus = parent.read_with(&visual, |parent, cx| {
                    parent.subagent_groups["parent-run"].read(cx).focus["a"][0].clone()
                });
                visual.update(|window, cx| focus.focus(window, cx));
                visual.simulate_keystrokes("enter");
                visual.run_until_parked();
                let expanded = visual.debug_bounds("subagent-group").unwrap();
                let result = visual.debug_bounds("subagent-result-body").unwrap();
                let peer = visual.debug_bounds("agent-b-open").unwrap();
                assert!(expanded.size.height > collapsed.size.height);
                assert!(peer.top() >= result.bottom());
                assert!(expanded.size.width <= px(width));
                assert!(before.size.width > px(0.));
                let open = visual.debug_bounds("agent-b-open").unwrap();
                visual.simulate_click(open.center(), Modifiers::default());
                visual.run_until_parked();
                let stop = visual.debug_bounds("agent-b-stop").unwrap();
                visual.simulate_click(stop.center(), Modifiers::default());
                visual.run_until_parked();
            }
        }
        assert_eq!(opened.lock().unwrap().as_slice(), ["b", "b", "b", "b"]);
        assert_eq!(stopped.lock().unwrap().as_slice(), ["b", "b", "b", "b"]);
    }

    #[gpui_kit::test]
    async fn m11_settings_fails_child_permission_and_question_closed(cx: &mut TestAppContext) {
        let (_, _, children, queues) = fixture(cx);
        propose(
            &children[0],
            ToolCall {
                id: "settings-permission".into(),
                tool: "bash".into(),
                input_json: r#"{"cmd":"printf synthetic"}"#.into(),
            },
            cx,
        );
        let permission = queues[0].request(
            PermissionRequest {
                call_id: "settings-permission".into(),
                tool: "bash".into(),
                display_target: "printf synthetic".into(),
                danger_rule_id: None,
                danger_reason: None,
                external: None,
                acp_options: None,
            },
            CancellationToken::new(),
        );
        let request = RuntimeUserQuestionRequest {call_id:"settings-question".into(),input:UserQuestionInput::from_json(r#"{"questions":[{"id":"choice","header":"Choice","question":"Choose a result","options":[{"label":"A","description":"First"},{"label":"B","description":"Second"}]}]}"#).unwrap()};
        propose(
            &children[1],
            ToolCall {
                id: request.call_id.clone(),
                tool: "ask_user_question".into(),
                input_json: request.input.to_json().unwrap(),
            },
            cx,
        );
        let queue = children[1].read_with(cx, |child, _| child.user_question_queue());
        let question = queue.request(request, CancellationToken::new());
        cx.run_until_parked();
        assert!(queues[0].has_pending());
        assert!(queue.has_pending());
        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert_eq!(permission.await.unwrap(), PermissionDecision::Timeout);
        assert!(matches!(
            question.await,
            RuntimeUserQuestionResponse::Cancelled
        ));
        assert!(!queues[0].has_pending());
        assert!(!queue.has_pending());
    }

    #[gpui_kit::test]
    async fn m17_child_live_usage_changes_parent_total_without_changing_parent_context_or_round_estimate(
        cx: &mut TestAppContext,
    ) {
        let (_, parent, _, _) = fixture(cx);
        let source = parent.update(cx, |parent, cx| {
            assert!(parent.apply_context_projection(
                "parent",
                "mock",
                None,
                ContextUsageProjection::new(Some(100_000), Some(50_000)),
                true,
                cx
            ));
            parent.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "parent-run".into(),
                    delta: "Parent output being estimated".into(),
                },
                cx,
            );
            parent.context_usage_source()
        });
        let before = parent.read_with(cx, |parent, _| parent.meter_snapshot());
        parent.update(cx, |parent, cx| {
            parent.apply_subagent_usage(
                &ConversationEvent::UsageUpdated {
                    message_id: "child-run".into(),
                    usage: vega_conversation::types::TokenUsage {
                        input: 700,
                        output: 4,
                        cache_read: Some(0),
                        cache_write: Some(0),
                    },
                    cost: vega_conversation::types::Microcents(123),
                    pricing: Some(vega_conversation::types::UsagePricing {
                        version: "fixture".into(),
                        profile: "fixture".into(),
                        call_started_at: 1,
                    }),
                },
                cx,
            )
        });
        let after = parent.read_with(cx, |parent, _| parent.meter_snapshot());
        assert_eq!(after.tokens, before.tokens + 704);
        assert_eq!(after.provisional, before.provisional);
        assert_eq!(
            parent.read_with(cx, |parent, _| parent.context_usage_source()),
            source
        );
        parent.update(cx, |parent, cx| parent.finish_subagent_usage(false, cx));
        assert_eq!(
            parent
                .read_with(cx, |parent, _| parent.meter_snapshot())
                .cost,
            None
        );
    }
}
