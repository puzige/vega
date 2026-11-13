use std::{cell::RefCell, rc::Rc};

use gpui_kit::{Subscription, Window};
use vega_theme::Layout;

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Destination {
    Other(usize),
    ProviderList,
    Provider(String),
    ProviderForm(Option<String>),
    Model(String, String, usize),
}

impl Destination {
    fn has_title(&self) -> bool {
        matches!(self, Self::Provider(_) | Self::Model(..))
    }
}

pub(super) enum ReturnFocus {
    ProviderList {
        provider: Option<String>,
        index: usize,
    },
    Model {
        provider: String,
        model: String,
    },
}

#[derive(Default)]
struct Presentation {
    destination: Option<Destination>,
    narrow: Option<bool>,
    permit: Option<Destination>,
    epoch: u64,
    title_serial: u64,
    seed_pending: bool,
    title_active: bool,
}

impl Presentation {
    fn interrupt(&mut self) {
        self.destination = None;
        self.permit = None;
        self.epoch = self.epoch.wrapping_add(1);
        self.seed_pending = false;
        self.title_active = false;
    }
}

#[derive(Default)]
pub(super) struct SettingsMotion {
    presentation: Rc<RefCell<Presentation>>,
    subscriptions: Option<[Subscription; 2]>,
}

impl SettingsMotion {
    pub(super) fn interrupt(&mut self) {
        self.presentation.borrow_mut().interrupt();
    }

    pub(super) fn committed(&mut self, before: Destination, after: Destination) {
        if before != after {
            self.presentation.borrow_mut().permit = Some(after);
        }
    }
}

pub(super) fn narrow(window: &Window) -> bool {
    let content_width = (f32::from(window.viewport_size().width)
        - Layout::SETTINGS_NAV_WIDTH
        - Layout::SETTINGS_SECTION_GAP * 2.)
        .min(Layout::SETTINGS_CONTENT_MAX_WIDTH);
    content_width
        < Layout::SETTINGS_PROVIDER_LIST_WIDTH
            + Layout::SETTINGS_PROVIDER_COLUMN_GAP
            + Layout::SETTINGS_PROVIDER_DETAIL_MIN_WIDTH
}

impl SettingsView {
    pub(super) fn project_settings_motion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_motion.subscriptions.is_none() {
            let activation = cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.settings_motion.interrupt();
                }
                cx.notify();
            });
            let bounds = cx.observe_window_bounds(window, |this, window, cx| {
                if this.settings_motion.presentation.borrow().narrow != Some(narrow(window)) {
                    this.settings_motion.interrupt();
                    cx.notify();
                }
            });
            self.settings_motion.subscriptions = Some([activation, bounds]);
        }
        let destination = self.settings_motion_destination();
        let narrow = narrow(window);
        let eligible = window.is_window_active()
            && !cx.reduce_motion()
            && cx.try_global::<SettingsOpen>().is_none_or(|open| open.0);
        let mut state = self.settings_motion.presentation.borrow_mut();
        let changed = state.destination.as_ref() != Some(&destination);
        let same_mode = state.narrow == Some(narrow);
        let permitted = state.permit.take().as_ref() == Some(&destination)
            && state.destination.is_some()
            && changed
            && same_mode
            && eligible;
        if !eligible || !same_mode || (changed && !permitted) {
            state.interrupt();
        }
        if permitted {
            state.title_serial = state.title_serial.wrapping_add(1);
            state.seed_pending = narrow && destination.has_title();
            state.title_active = state.seed_pending;
        }
        state.destination = Some(destination);
        state.narrow = Some(narrow);
    }
}
