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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_ignores_repeat_modifiers_and_character_preference() {
        let mut event = KeyDownEvent {
            keystroke: gpui_kit::Keystroke {
                key: "enter".into(),
                ..Default::default()
            },
            is_held: false,
            prefer_character_input: false,
        };
        assert!(matches!(Key::from_event(&event), Key::Activate));
        event.is_held = true;
        assert!(matches!(Key::from_event(&event), Key::Ignore));
        event.is_held = false;
        event.keystroke.modifiers.platform = true;
        assert!(matches!(Key::from_event(&event), Key::Ignore));
        event.keystroke.modifiers.platform = false;
        event.prefer_character_input = true;
        assert!(matches!(Key::from_event(&event), Key::Ignore));
        event.prefer_character_input = false;
        event.keystroke.key = "tab".into();
        event.keystroke.modifiers.shift = true;
        assert!(matches!(Key::from_event(&event), Key::Next(true)));
    }

    #[test]
    fn reading_order_skips_unavailable_targets_and_exits_at_both_edges() {
        let session = ProjectionId::new();
        let first = Selection::reconcile(None, &session, &[1, 3]).unwrap();
        assert_eq!(first.action(&session, &[1, 3]), Some(1));
        assert!(matches!(
            Selection::advance(Some(&first), &session, &[1, 3], true),
            Step::Exit
        ));
        let Step::Selected(last) = Selection::advance(Some(&first), &session, &[1, 3], false)
        else {
            panic!("second eligible control must be selected");
        };
        assert_eq!(last.action(&session, &[1, 3]), Some(3));
        assert!(matches!(
            Selection::advance(Some(&last), &session, &[1, 3], false),
            Step::Exit
        ));
        assert_eq!(last.action(&session, &[1]), None);
    }

    #[test]
    fn replaced_session_cannot_activate_or_inherit_an_old_selection() {
        let session = ProjectionId::new();
        let Step::Selected(last) = Selection::advance(None, &session, &[1, 3], true) else {
            panic!("reverse entry selects the last control");
        };
        let replacement = ProjectionId::new();
        assert_eq!(last.action(&replacement, &[1, 3]), None);
        let reset = Selection::reconcile(Some(&last), &replacement, &[1, 3]).unwrap();
        assert_eq!(reset.action(&replacement, &[1, 3]), Some(1));
        assert!(Selection::reconcile(Some(&last), &replacement, &[]).is_none());
    }
}
