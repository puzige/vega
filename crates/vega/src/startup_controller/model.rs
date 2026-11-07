use gpui_kit::KeyDownEvent;
use vega_conversation::types::StartupError;
use vega_theme::ThemeColors;
use vega_ui::nsh::ProjectionId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Retry,
    Quit,
    Reload,
}

impl Action {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Retry => vega_i18n::t("ui.retry"),
            Self::Quit => vega_i18n::t("app.startup.quit"),
            Self::Reload => localized("Reload startup view", "重新加载启动界面"),
        }
    }
}

pub(super) fn localized(english: &'static str, chinese: &'static str) -> &'static str {
    if vega_i18n::language() == "zh-CN" {
        chinese
    } else {
        english
    }
}

#[derive(Clone)]
pub(super) struct Authority {
    pub(super) session: ProjectionId,
    pub(super) mount: ProjectionId,
    pub(super) projection: ProjectionId,
    pub(super) running: bool,
    pub(super) error: Option<StartupError>,
    pub(super) recovery: bool,
}

impl Authority {
    pub(super) fn controls(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        if !self.running && self.error.is_some() {
            actions.extend([Action::Retry, Action::Quit]);
        }
        if self.recovery {
            actions.push(Action::Reload);
        }
        actions
    }

    pub(super) fn accepts(&self, observed: &Self, action: Action) -> bool {
        self.session.matches(&observed.session)
            && self.mount.matches(&observed.mount)
            && self.projection.matches(&observed.projection)
            && self.running == observed.running
            && self.error == observed.error
            && self.recovery == observed.recovery
            && self.controls().contains(&action)
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub(super) frame: ProjectionId,
    pub(super) authority: Authority,
    pub(super) colors: ThemeColors,
    pub(super) selected: Option<Action>,
    pub(super) phase: Option<f32>,
    pub(super) language: &'static str,
    pub(super) appearance: vega_theme::Appearance,
}

#[derive(Clone)]
pub(super) struct Frame {
    id: ProjectionId,
    authority: Authority,
    pub(super) selected: Option<Action>,
    language: &'static str,
    appearance: vega_theme::Appearance,
}

impl Frame {
    fn matches_state(&self, snapshot: &Snapshot) -> bool {
        self.authority.session.matches(&snapshot.authority.session)
            && self.authority.mount.matches(&snapshot.authority.mount)
            && self
                .authority
                .projection
                .matches(&snapshot.authority.projection)
            && self.authority.running == snapshot.authority.running
            && self.authority.error == snapshot.authority.error
            && self.authority.recovery == snapshot.authority.recovery
            && self.selected == snapshot.selected
            && self.language == snapshot.language
            && self.appearance == snapshot.appearance
    }
}

impl Snapshot {
    pub(super) fn rendered_frame(&self) -> Frame {
        Frame {
            id: self.frame.clone(),
            authority: self.authority.clone(),
            selected: self.selected,
            language: self.language,
            appearance: self.appearance,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Input {
    Pointer,
    Keyboard,
    Accessibility,
}

#[derive(Default)]
pub(super) struct RenderFence {
    published: Option<Frame>,
    displayed: Option<Frame>,
    consumed: bool,
}

impl RenderFence {
    pub(super) fn publish(&mut self, snapshot: &mut Snapshot) {
        if let Some(previous) = &self.published
            && previous.matches_state(snapshot)
        {
            snapshot.frame = previous.id.clone();
            return;
        }
        snapshot.frame = ProjectionId::new();
        self.published = Some(snapshot.rendered_frame());
        self.consumed = false;
    }

    pub(super) fn acknowledge(&mut self, frame: Frame) -> bool {
        if !self
            .published
            .as_ref()
            .is_some_and(|published| published.id.matches(&frame.id))
            || self
                .displayed
                .as_ref()
                .is_some_and(|displayed| displayed.id.matches(&frame.id))
        {
            return false;
        }
        self.displayed = Some(frame);
        true
    }

    pub(super) fn claim(
        &mut self,
        observed: &Frame,
        current: &Snapshot,
        action: Action,
        input: Input,
        failed: bool,
    ) -> bool {
        let published_matches = self.published.as_ref().is_some_and(|published| {
            published.id.matches(&observed.id) && published.matches_state(current)
        });
        let displayed_matches = self
            .displayed
            .as_ref()
            .is_some_and(|displayed| displayed.id.matches(&observed.id));
        let failed_recovery_reload =
            failed && current.authority.recovery && action == Action::Reload;
        if self.consumed
            || !published_matches
            || !observed.matches_state(current)
            || !current.authority.accepts(&observed.authority, action)
            || (!matches!(input, Input::Pointer) && observed.selected != Some(action))
            || if failed {
                !failed_recovery_reload
            } else {
                !displayed_matches
            }
        {
            return false;
        }
        self.consumed = true;
        true
    }
}

pub(super) enum Request {
    Rendered(Frame),
    Activate { observed: Frame, action: Action },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Traverse { backwards: bool },
    Activate,
}

pub(super) fn key(event: &KeyDownEvent) -> Option<Key> {
    let modifiers = event.keystroke.modifiers;
    if event.is_held
        || modifiers.control
        || modifiers.alt
        || modifiers.platform
        || modifiers.function
    {
        return None;
    }
    match event.keystroke.key.as_str() {
        "tab" => Some(Key::Traverse {
            backwards: modifiers.shift,
        }),
        "enter" | "space" | " " if !modifiers.shift => Some(Key::Activate),
        _ => None,
    }
}

pub(super) fn traverse(
    current: Option<Action>,
    controls: &[Action],
    backwards: bool,
) -> Option<Action> {
    if controls.is_empty() {
        return None;
    }
    let position =
        current.and_then(|current| controls.iter().position(|action| *action == current));
    let next = match (position, backwards) {
        (Some(index), true) => (index + controls.len() - 1) % controls.len(),
        (Some(index), false) => (index + 1) % controls.len(),
        (None, true) => controls.len() - 1,
        (None, false) => 0,
    };
    Some(controls[next])
}
