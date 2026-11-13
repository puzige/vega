use super::ProjectionId;
use gpui_kit::{App, FocusHandle, KeyDownEvent, Window};

#[derive(Clone)]
pub(crate) struct Selection<A> {
    session: ProjectionId,
    action: A,
}

pub(crate) enum Step<A> {
    Selected(Selection<A>),
    Exit,
}

pub(crate) enum Key {
    Next(bool),
    Activate,
    Ignore,
}

impl Key {
    pub(crate) fn from_event(event: &KeyDownEvent) -> Self {
        let modifiers = event.keystroke.modifiers;
        if event.prefer_character_input
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return Self::Ignore;
        }
        match event.keystroke.key.as_str() {
            "tab" => Self::Next(modifiers.shift),
            "enter" | "space" | " " if !event.is_held && !modifiers.shift => Self::Activate,
            _ => Self::Ignore,
        }
    }
}

impl<A: Clone + PartialEq> Selection<A> {
    pub(crate) fn reconcile(
        previous: Option<&Self>,
        session: &ProjectionId,
        enabled: &[A],
    ) -> Option<Self> {
        let action = previous
            .filter(|previous| previous.session.matches(session))
            .and_then(|previous| enabled.iter().find(|action| **action == previous.action))
            .or_else(|| enabled.first())?
            .clone();
        Some(Self {
            session: session.clone(),
            action,
        })
    }

    pub(crate) fn action(&self, session: &ProjectionId, enabled: &[A]) -> Option<A> {
        (self.session.matches(session) && enabled.contains(&self.action))
            .then(|| self.action.clone())
    }

    pub(crate) fn advance(
        previous: Option<&Self>,
        session: &ProjectionId,
        enabled: &[A],
        backwards: bool,
    ) -> Step<A> {
        let current = previous
            .filter(|previous| previous.session.matches(session))
            .and_then(|previous| enabled.iter().position(|action| *action == previous.action));
        let next = match (current, backwards) {
            (None, false) => enabled.first(),
            (None, true) => enabled.last(),
            (Some(index), false) => enabled.get(index + 1),
            (Some(index), true) => index.checked_sub(1).and_then(|index| enabled.get(index)),
        };
        match next {
            Some(action) => Step::Selected(Self {
                session: session.clone(),
                action: action.clone(),
            }),
            None => Step::Exit,
        }
    }
}

pub(crate) fn leave(host: &FocusHandle, backwards: bool, window: &mut Window, cx: &mut App) {
    window.focus(host, cx);
    for _ in 0..2 {
        if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if !host.contains_focused(window, cx) {
            return;
        }
    }
    window.focus(host, cx);
}
