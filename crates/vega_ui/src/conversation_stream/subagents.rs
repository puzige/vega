use super::*;
use crate::interaction::InteractionExt;
use gpui_kit::Role;
use vega_conversation::types::{
    SubagentOpenRequested, SubagentSnapshot, SubagentState, SubagentStopRequested,
};

pub(crate) struct SubagentGroup {
    agents: Vec<SubagentSnapshot>,
    streams: HashMap<String, Entity<ConversationStream>>,
    stream_observers: HashMap<String, gpui_kit::Subscription>,
}

impl EventEmitter<SubagentOpenRequested> for ConversationStream {}
impl EventEmitter<SubagentStopRequested> for ConversationStream {}

impl SubagentGroup {
    fn update(&mut self, snapshot: SubagentSnapshot, cx: &mut Context<Self>) {
        if let Some(current) = self
            .agents
            .iter()
            .find(|agent| agent.agent_id == snapshot.agent_id)
            && current.message_id != snapshot.message_id
            && snapshot.state.is_terminal()
        {
            return;
        }
        if snapshot.state.is_terminal() {
            if let Some(stream) = self.streams.remove(&snapshot.agent_id) {
                stream.update(cx, |stream, _| stream.delegated_run_active = false);
            }
            self.stream_observers.remove(&snapshot.agent_id);
        }
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
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        self.streams
            .values()
            .map(|stream| {
                let stream = stream.read(cx);
                stream
                    .active_permission
                    .as_ref()
                    .map_or(0, |card| card.read(cx).row_count())
                    + usize::from(stream.active_user_question.is_some())
            })
            .sum()
    }
}

impl Render for SubagentGroup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut group = div().w_full().min_w_0().flex().flex_col().gap_2();
        for agent in &self.agents {
            if let Some(stream) = self.streams.get(&agent.agent_id) {
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
        if let Some(mut snapshot) = self
            .subagent_executions
            .get(&(id.into(), run.into()))
            .cloned()
            && snapshot.state == SubagentState::Running
        {
            snapshot.state = SubagentState::Cancelling;
            self.apply_subagent(snapshot, cx);
        }
    }

    pub(crate) fn refresh_subagent_card(&self, card: &Entity<ToolCard>, cx: &mut Context<Self>) {
        let snapshot = card
            .read(cx)
            .subagent()
            .and_then(|agent| {
                self.subagent_executions
                    .get(&(agent.agent_id.clone(), agent.message_id.clone()))
            })
            .cloned();
        if let Some(snapshot) = snapshot {
            card.update(cx, |card, cx| card.apply_subagent(snapshot, cx));
        }
    }

    pub fn apply_subagent(&mut self, snapshot: SubagentSnapshot, cx: &mut Context<Self>) {
        if snapshot.parent_thread_id != self.thread.id {
            return;
        }
        let key = (snapshot.agent_id.clone(), snapshot.message_id.clone());
        if self
            .subagent_executions
            .get(&key)
            .is_some_and(|current| current.state.is_terminal() && !snapshot.state.is_terminal())
        {
            return;
        }
        self.subagent_executions.insert(key, snapshot.clone());
        for card in self.tool_cards.values() {
            card.update(cx, |card, cx| card.apply_subagent(snapshot.clone(), cx));
        }
        let owner = snapshot.parent_message_id.clone();
        let group = if let Some(group) = self.subagent_groups.get(&owner) {
            group.clone()
        } else {
            let group = cx.new(|_| SubagentGroup {
                agents: Vec::new(),
                streams: HashMap::new(),
                stream_observers: HashMap::new(),
            });
            cx.observe(&group,|this,group,cx| {
                let index = this.entry_index_where(|entry| matches!(entry, StreamEntry::Subagents {group:owned} if owned == &group));
                this.invalidate_item(index); cx.notify();
            }).detach();
            self.subagent_groups.insert(owner.clone(), group.clone());
            group
        };
        group.update(cx, |group, cx| group.update(snapshot, cx));
        self.ensure_subagent_group_entry(&owner, &group);
        cx.notify();
    }
    fn ensure_subagent_group_entry(&mut self, owner: &str, group: &Entity<SubagentGroup>) {
        if self.entries.iter().any(
            |entry| matches!(entry, StreamEntry::Subagents { group: current } if current == group),
        ) {
            return;
        }
        self.ensure_entry_identities();
        let index = self
            .entry_identities
            .iter()
            .rposition(|identity| identity.message_id.as_deref() == Some(owner))
            .map_or(self.entries.len(), |index| index + 1);
        self.entries.insert(
            index,
            StreamEntry::Subagents {
                group: group.clone(),
            },
        );
        let identity = self.durable_entry_identity(owner, "subagents", None);
        self.entry_identities.insert(index, identity);
        self.grow_entry_indices(index, 1);
        self.list.splice(index..index, 1);
    }

    pub(crate) fn preserve_subagent_requests(&mut self, cx: &App) {
        self.subagent_groups
            .retain(|_, group| !group.read(cx).streams.is_empty());
    }

    pub(crate) fn restore_subagent_request_entries(&mut self) {
        let groups: Vec<_> = self
            .subagent_groups
            .iter()
            .map(|(owner, group)| (owner.clone(), group.clone()))
            .collect();
        for (owner, group) in groups {
            self.ensure_subagent_group_entry(&owner, &group);
        }
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
                .aria_label(vega_i18n::t("ui.back_to_parent"))
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
                .child(vega_i18n::t("ui.back_to_parent"))
                .into_any_element(),
        )
    }
}
