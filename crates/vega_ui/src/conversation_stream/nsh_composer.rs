use super::*;
use crate::text_input::nsh::{ComposerKey, ComposerRoute, ComposerShortcut};

impl ConversationStream {
    pub fn render_nsh_composer_input(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::text_input::TextInput::render_nsh_composer(
            self.input.clone(),
            self.nsh_composer_route(cx),
            window,
            cx,
        )
    }

    pub(super) fn nsh_composer_route(&self, cx: &App) -> ComposerRoute {
        let input = self.input.read(cx);
        let current = input.text();
        ComposerRoute {
            policy: Default::default(),
            files: self.file_selector.is_open()
                || self.file_selector_wanted
                || self.file_index_loading,
            file_retry: self.file_selector_wanted && self.file_index_failure.is_some(),
            actions: self.composer_actions_visible(cx),
            file_generation: self.file_index_generation,
            file_highlight: self.file_selector.highlighted(),
            action_generation: self.actions.menu_generation,
            action_highlight: self.nsh_highlight(),
            has_history: !self.composer_history.is_empty(),
            recalling_history: self
                .history_cursor
                .and_then(|index| self.composer_history.get(index))
                .is_some_and(|draft| draft == current),
        }
    }

    pub(super) fn prepare_nsh_composer(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.nsh_composer_subscription.is_some() {
            return;
        }
        self.nsh_composer_subscription = Some(cx.subscribe_in(
            &self.input,
            window,
            |this, input, event: &ComposerShortcut, window, cx| {
                let route = this.nsh_composer_route(cx);
                if input != &this.input
                    || this.reading_preview
                    || !input.read(cx).composer_contains_focus(window, cx)
                    || !event.matches(input.read(cx), route)
                {
                    return;
                }
                match event.key {
                    ComposerKey::Send => this.activate_composer_primary(cx),
                    ComposerKey::History => this.recall_previous_draft(cx),
                    ComposerKey::Previous if route.files => {
                        this.on_selector_previous(&PreviousFile, window, cx)
                    }
                    ComposerKey::Next if route.files => {
                        this.on_selector_next(&NextFile, window, cx)
                    }
                    ComposerKey::Accept if route.file_retry => {
                        this.on_selector_retry_action(&RetryFile, window, cx)
                    }
                    ComposerKey::Accept if route.files => {
                        this.on_selector_accept(&AcceptFile, window, cx)
                    }
                    ComposerKey::Cancel if route.files => {
                        this.on_selector_cancel(&CancelFile, window, cx)
                    }
                    ComposerKey::Previous if route.actions => {
                        this.previous_composer_action(&PreviousComposerAction, window, cx)
                    }
                    ComposerKey::Next if route.actions => {
                        this.next_composer_action(&NextComposerAction, window, cx)
                    }
                    ComposerKey::Accept if route.actions => {
                        this.accept_composer_action(&AcceptComposerAction, window, cx)
                    }
                    ComposerKey::Cancel if route.actions => {
                        this.close_composer_actions(&CloseComposerActions, window, cx)
                    }
                    ComposerKey::FocusNext => {
                        this.next_composer_control(&NextComposerControl, window, cx)
                    }
                    ComposerKey::FocusPrevious => this.nsh_previous_composer_control(window, cx),
                    _ => {}
                }
            },
        ));
    }
}
