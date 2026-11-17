use super::*;
use crate::text_input::nsh::{ComposerKey, ComposerRoute, ComposerShortcut};

#[derive(Clone)]
pub(super) struct RenameBinding {
    identity: Entity<()>,
    input: Entity<TextInput>,
    thread: String,
    row: String,
    project_generation: u64,
    navigation_generation: u64,
    route: (Option<String>, Option<String>, bool),
}

impl RenameBinding {
    fn current(&self, owner: &ThreadsBlock, cx: &App) -> bool {
        owner.editing.as_ref().is_some_and(|session| {
            self.identity == session.binding.identity
                && self.input == session.input
                && self.thread == session.thread_id
                && self.row == session.row_key
        }) && self.project_generation == owner.project_generation
            && self.navigation_generation == owner.visit_navigation_generation
            && self.route == ThreadsBlock::current_visit_route(cx)
            && owner
                .threads
                .iter()
                .chain(owner.archived.iter())
                .any(|thread| thread.id == self.thread)
    }
}

impl ThreadsBlock {
    pub(super) fn bind_rename(
        &mut self,
        thread: String,
        row: String,
        input: Entity<TextInput>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.observe_visit_route(cx);
        let binding = RenameBinding {
            identity: cx.new(|_| ()),
            input: input.clone(),
            thread: thread.clone(),
            row: row.clone(),
            project_generation: self.project_generation,
            navigation_generation: self.visit_navigation_generation,
            route: Self::current_visit_route(cx),
        };
        let captured = binding.clone();
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |this, source, event: &ComposerShortcut, window, cx| {
                if source != &captured.input
                    || !captured.current(this, cx)
                    || !source.read(cx).composer_contains_focus(window, cx)
                    || !source.read(cx).composer_editor_ready()
                    || !event.matches(
                        source.read(cx),
                        ComposerRoute::single_line(captured.identity.entity_id()),
                    )
                {
                    return;
                }
                match event.key {
                    ComposerKey::Send => this.commit_rename(cx),
                    ComposerKey::Cancel => this.cancel_rename(cx),
                    ComposerKey::FocusNext => window.focus_next(cx),
                    ComposerKey::FocusPrevious => window.focus_prev(cx),
                    _ => {}
                }
            },
        );
        self.editing = Some(RenameSession {
            thread_id: thread,
            row_key: row,
            input: input.clone(),
            binding,
            _shortcut_subscription: subscription,
        });
        window.focus(&input.read(cx).focus_handle(cx), cx);
        cx.notify();
    }
}

#[derive(gpui_kit::IntoElement)]
struct RenameMount {
    owner: gpui_kit::WeakEntity<ThreadsBlock>,
    binding: RenameBinding,
    leading_inset: f32,
}

impl gpui_kit::RenderOnce for RenameMount {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(owner) = self.owner.upgrade() else {
            return div().into_any_element();
        };
        if !self.binding.current(owner.read(cx), cx) {
            return div().into_any_element();
        }
        let input = self.binding.input.clone();
        let identity = self.binding.identity.entity_id();
        let binding = self.binding;
        div()
            .flex_1()
            .min_w_0()
            .pl(px(self.leading_inset))
            .pr_1()
            .track_focus(&input.read(cx).focus_handle(cx))
            .on_mouse_down_out(move |_, _, cx| {
                owner.update(cx, |this, cx| {
                    if binding.current(this, cx) {
                        this.cancel_rename(cx);
                    }
                });
            })
            .child(TextInput::render_nsh_single_line(
                input, identity, window, cx,
            ))
            .into_any_element()
    }
}

pub(super) fn mount(
    session: &RenameSession,
    leading_inset: f32,
    cx: &mut Context<ThreadsBlock>,
) -> AnyElement {
    RenameMount {
        owner: cx.weak_entity(),
        binding: session.binding.clone(),
        leading_inset,
    }
    .into_any_element()
}
