use super::{BRANCH_CHROME_HEIGHT, BRANCH_ROW_HEIGHT, BranchOperationId, SelectorStatus};
use crate::nsh::{OrderedCommands, ProjectionId};
use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::mpsc;
use vega_conversation::types::{BranchId, BranchItem};
use vega_theme::{Layout, ThemeColors, Typography};

#[derive(Clone, Default)]
pub(super) struct Draft(pub Arc<Mutex<DraftValue>>);

#[derive(Default)]
pub(super) struct DraftValue {
    pub text: String,
    pub composing: bool,
    pub revision: u64,
    pub page: Option<ProjectionId>,
}

impl Draft {
    pub fn interact<T>(
        &self,
        filter: Option<&str>,
        revision: Option<u64>,
        row: Option<&Weak<()>>,
        action: impl FnOnce() -> T,
    ) -> Option<T> {
        let lease = match row {
            Some(row) => Some(row.upgrade()?),
            None => None,
        };
        {
            let draft = self.0.lock().ok()?;
            if draft.composing
                || revision.is_some_and(|revision| revision != draft.revision)
                || filter.is_some_and(|filter| draft.text.trim().to_lowercase() != filter)
            {
                return None;
            }
        }
        let result = action();
        drop(lease);
        Some(result)
    }
}

#[derive(Clone)]
pub(super) struct RowGeometry(Arc<Mutex<GeometryValue>>);

struct GeometryValue {
    revision: ProjectionId,
    bounds: Option<([f32; 4], Weak<()>)>,
}

impl Default for RowGeometry {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(GeometryValue {
            revision: ProjectionId::new(),
            bounds: None,
        })))
    }
}

impl RowGeometry {
    pub fn revision(&self) -> Option<ProjectionId> {
        self.0.lock().ok().map(|geometry| geometry.revision.clone())
    }

    pub fn invalidate(&self) -> Option<ProjectionId> {
        let mut geometry = self.0.lock().ok()?;
        geometry.revision = ProjectionId::new();
        geometry.bounds = None;
        Some(geometry.revision.clone())
    }

    pub fn install(
        &self,
        revision: &ProjectionId,
        row: [f32; 4],
        viewport: [f32; 4],
        lease: Weak<()>,
    ) -> bool {
        let Ok(mut geometry) = self.0.lock() else {
            return false;
        };
        if !geometry.revision.matches(revision) {
            return false;
        }
        geometry.bounds = None;
        if lease.upgrade().is_none()
            || !row
                .iter()
                .chain(viewport.iter())
                .all(|value| value.is_finite())
            || row[2] <= 0.
            || row[3] <= 0.
            || viewport[2] <= 0.
            || viewport[3] <= 0.
        {
            return false;
        }
        let left = row[0].max(viewport[0]);
        let top = row[1].max(viewport[1]);
        let right = (row[0] + row[2]).min(viewport[0] + viewport[2]);
        let bottom = (row[1] + row[3]).min(viewport[1] + viewport[3]);
        if right <= left || bottom <= top {
            return false;
        }
        geometry.bounds = Some(([left, top, right - left, bottom - top], lease));
        true
    }

    pub fn bounds(&self, revision: &ProjectionId) -> Option<([f32; 4], Weak<()>)> {
        let geometry = self.0.lock().ok()?;
        if !geometry.revision.matches(revision) {
            return None;
        }
        geometry
            .bounds
            .as_ref()
            .filter(|(_, lease)| lease.upgrade().is_some())
            .cloned()
    }
}

#[derive(Clone)]
pub(super) struct Snapshot {
    pub page: ProjectionId,
    pub revision: ProjectionId,
    pub thread: String,
    pub project: String,
    pub open: bool,
    pub disabled: bool,
    pub pending: Option<(BranchOperationId, u64, BranchId)>,
    pub generation: Option<u64>,
    pub rows: Arc<[BranchItem]>,
    pub status: SelectorStatus,
    pub total: usize,
    pub focused: Option<BranchId>,
    pub visual: Option<BranchId>,
    pub reveal: Option<usize>,
    pub label: String,
    pub chip: bool,
    pub keyboard: bool,
    pub failure: bool,
    pub focus_input: bool,
    pub draft_revision: u64,
    pub draft: Draft,
    pub geometry: RowGeometry,
    pub colors: ThemeColors,
    pub appearance: vega_theme::Appearance,
    pub language: &'static str,
}

impl Snapshot {
    pub fn same_authority(&self, other: &Self) -> bool {
        self.page.matches(&other.page)
            && self.revision.matches(&other.revision)
            && self.thread == other.thread
            && self.project == other.project
            && self.open == other.open
            && self.disabled == other.disabled
            && self.pending == other.pending
            && self.generation == other.generation
            && self.status == other.status
            && self.focused == other.focused
            && self.visual == other.visual
            && self.failure == other.failure
            && self.label == other.label
            && self.chip == other.chip
            && self.appearance == other.appearance
            && self.language == other.language
    }

    pub fn same_render(&self, other: &Self) -> bool {
        self.same_authority(other)
            && self.keyboard == other.keyboard
            && self.focus_input == other.focus_input
            && self.draft_revision == other.draft_revision
            && Arc::ptr_eq(&self.rows, &other.rows)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Action {
    Toggle,
    Activate(BranchId),
    Previous,
    Next,
    Close,
    Reload,
}

pub(super) enum Request {
    Action(Snapshot, Action, Option<Weak<()>>, Option<u64>),
    Filter(ProjectionId, u64),
    Displayed(Snapshot),
    InputFocused(ProjectionId),
    GeometryChanged(ProjectionId),
    Measured(
        Snapshot,
        BranchId,
        [f32; 4],
        [f32; 4],
        Weak<()>,
        ProjectionId,
    ),
}

fn ink(value: gpui_kit::Rgba) -> Rgba {
    crate::nsh::color(value)
}

fn icon(markup: &'static str, color: impl IntoReactive<AdaptiveColor>) -> impl IntoElement {
    svg(ImageSrc::svg_markup(markup))
        .size(px(Layout::ICON_SIZE))
        .flex_shrink_0()
        .text_color(color)
}

const BRANCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="6" y1="7" x2="6" y2="17"/><circle cx="6" cy="5" r="2"/><circle cx="18" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><path d="M18 7v3a9 9 0 0 1-9 9H8"/></svg>"#;
const CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m20 6-11 11-5-5"/></svg>"#;
const PLUS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>"#;

fn recovery(state: Signal<Snapshot>, commands: OrderedCommands<Request>) -> AnyElement {
    view()
        .role(AccessibilityRole::Button)
        .label(move || state.with(|_| vega_i18n::t("settings.action.reload")))
        .h(px(BRANCH_ROW_HEIGHT))
        .px_2()
        .text_color(move || ink(state.get().colors.danger))
        .child(text(move || {
            state.with(|_| vega_i18n::t("settings.action.reload"))
        }))
        .on_press(move || {
            commands.send(Request::Action(state.get(), Action::Reload, None, None));
        })
        .into_element()
}

pub(super) fn trigger(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let retry = commands.clone();
    let displayed = commands.clone();
    effect(move || displayed.send(Request::Displayed(state.get())));
    view()
        .w_full()
        .h_full()
        .child(dynamic(move || {
            let observed = state.get();
            if observed.failure || commands.failed.get() {
                return recovery(state, retry.clone());
            }
            let send = commands.clone();
            let chip = observed.chip;
            view()
                .role(AccessibilityRole::Button)
                .label(observed.label.clone())
                .accessibility_state(AccessibilityState {
                    expanded: Some(observed.open),
                    busy: observed.pending.is_some(),
                    disabled: observed.disabled || observed.pending.is_some(),
                    ..Default::default()
                })
                .disabled(observed.disabled || observed.pending.is_some())
                .flex_row()
                .items_center()
                .gap(px(Layout::COMPOSER_UTILITY_CHIP_GAP))
                .h_full()
                .w_full()
                .px(px(if chip {
                    Layout::COMPOSER_UTILITY_CHIP_PADDING_X
                } else {
                    8.
                }))
                .rounded(px(if chip {
                    Layout::COMPOSER_UTILITY_CHIP_HEIGHT / 2.
                } else {
                    Layout::MENU_RADIUS
                }))
                .border(px(if chip && !observed.keyboard { 0. } else { 1. }))
                .border_color(if observed.keyboard {
                    ink(observed.colors.accent)
                } else {
                    ink(observed.colors.border_subtle)
                })
                .bg(if chip && observed.open {
                    ink(observed.colors.bg_utility_chip_overlay)
                } else {
                    ink(gpui_kit::Rgba {
                        a: 0.,
                        ..observed.colors.bg_base
                    })
                })
                .text_size(px(Typography::SIDEBAR))
                .text_color(if observed.disabled || observed.pending.is_some() {
                    ink(observed.colors.text_tertiary)
                } else if chip {
                    ink(observed.colors.text_primary)
                } else {
                    ink(observed.colors.text_secondary)
                })
                .hover(|style| style.bg(ink(observed.colors.bg_utility_chip_overlay)))
                .child(when(
                    move || chip,
                    move || icon(BRANCH, ink(observed.colors.text_secondary)),
                ))
                .child(
                    text(observed.label.clone())
                        .flex_1()
                        .min_w(px(0.))
                        .truncate(),
                )
                .on_press(move || {
                    send.send(Request::Action(
                        observed.clone(),
                        Action::Toggle,
                        None,
                        None,
                    ));
                })
                .into_element()
        }))
        .into_element()
}

pub(super) fn popup(state: Signal<Snapshot>, sender: mpsc::Sender<Request>) -> AnyElement {
    let commands = OrderedCommands::new(sender);
    let query = signal(String::new());
    let composing = signal(false);
    let search = element_ref();
    let list = element_ref();
    let viewport = signal(0_f32);
    let offset = signal(0_f32);
    let geometry_revision = signal_with(
        state.get_untracked().geometry.revision(),
        SignalEquals::Never,
    );
    let owner = signal_with(None::<ProjectionId>, SignalEquals::Never);
    let displayed = commands.clone();
    effect(move || displayed.send(Request::Displayed(state.get())));
    effect(move || {
        let current = state.get();
        if !owner
            .get()
            .is_some_and(|owner| owner.matches(&current.page))
        {
            owner.set(Some(current.page.clone()));
            if let Ok(draft) = current.draft.0.lock() {
                query.set(draft.text.clone());
            }
        } else if let Ok(draft) = current.draft.0.lock()
            && !draft.composing
            && !composing.get()
        {
            query.set(draft.text.clone());
        }
    });
    let filtered_owner = signal_with(None::<ProjectionId>, SignalEquals::Never);
    effect(move || {
        let current = state.get();
        if !filtered_owner
            .get()
            .is_some_and(|owner| owner.matches(&current.revision))
        {
            filtered_owner.set(Some(current.revision.clone()));
            if current.visual.is_none() {
                list.scroll_to_offset(px(0.), false);
            }
        }
        if let Some(index) = current.reveal {
            let top = index as f32 * BRANCH_ROW_HEIGHT;
            let height = viewport.get();
            let scroll = offset.get_untracked();
            if height > 0. && (top < scroll || top + BRANCH_ROW_HEIGHT > scroll + height) {
                list.scroll_to_offset(
                    px(if top < scroll {
                        top
                    } else {
                        top + BRANCH_ROW_HEIGHT - height
                    }),
                    false,
                );
            }
        }
    });
    effect(move || {
        if state.get().focus_input && search.node().is_some() {
            search.focus();
        }
    });
    let change = commands.clone();
    let composition = commands.clone();
    let input_focus = commands.clone();
    let retry = commands.clone();
    let rows = commands.clone();
    let scrolling = commands.clone();
    view()
        .w_full()
        .h_full()
        .accessibility_state(move || AccessibilityState {
            busy: state.get().pending.is_some() || state.get().status == SelectorStatus::Loading,
            ..Default::default()
        })
        .overflow_hidden()
        .rounded(px(Layout::MENU_RADIUS))
        .bg(move || ink(state.get().colors.bg_elevated))
        .text_color(move || ink(state.get().colors.text_primary))
        .text_size(px(Typography::SIDEBAR))
        .child(when(
            {
                let commands = commands.clone();
                move || state.get().failure || commands.failed.get()
            },
            move || recovery(state, retry.clone()),
        ))
        .child(when(
            move || !state.get().failure && !commands.failed.get(),
            move || {
                let change = change.clone();
                let composition = composition.clone();
                let input_focus = input_focus.clone();
                let rows = rows.clone();
                let scrolling = scrolling.clone();
                view()
                    .w_full()
                    .h_full()
                    .child(
                        view()
                            .flex_row()
                            .items_center()
                            .flex_shrink_0()
                            .h(px(crate::menu_list::MENU_ROW_HEIGHT))
                            .mb(px(4.))
                            .mx(px(4.))
                            .px(px(crate::menu_list::MENU_ROW_PADDING_X))
                            .gap_2()
                            .child(
                                svg(ImageSrc::svg_markup(
                                    &crate::icons::nsh_source(crate::icons::Icon::Search)
                                        .unwrap_or_default(),
                                ))
                                .size(px(Layout::ICON_SIZE))
                                .flex_shrink_0()
                                .accessible(false)
                                .text_color(move || ink(state.get().colors.text_tertiary)),
                            )
                            .child(
                                nsh_base::input()
                                    .element_ref(&search)
                                    .label(move || {
                                        state.with(|_| vega_i18n::t("ui.search_branches"))
                                    })
                                    .placeholder(move || {
                                        state
                                            .with(|_| vega_i18n::t("ui.search_branches").to_owned())
                                    })
                                    .value(move || query.get())
                                    .readonly(move || state.get().pending.is_some())
                                    .h(px(crate::menu_list::MENU_ROW_HEIGHT))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_color(move || ink(state.get().colors.text_primary))
                                    .on_layout(move |_: &Bounds<Points>| {
                                        if state.get_untracked().focus_input {
                                            search.focus();
                                        }
                                    })
                                    .on_focus(move |_: &()| {
                                        input_focus.send(Request::InputFocused(state.get().page));
                                    })
                                    .on_change(move |text: &str| {
                                        query.set(text.to_owned());
                                        let current = state.get();
                                        if let Ok(mut draft) = current.draft.0.lock() {
                                            if !draft
                                                .page
                                                .as_ref()
                                                .is_some_and(|page| page.matches(&current.page))
                                            {
                                                return;
                                            }
                                            draft.text = text.to_owned();
                                            draft.composing = composing.get();
                                            draft.revision = draft.revision.saturating_add(1);
                                            change.send(Request::Filter(
                                                current.page.clone(),
                                                draft.revision,
                                            ));
                                        }
                                    })
                                    .on_composition_change(move |event: &Composition| {
                                        composing.set(event.composing);
                                        let current = state.get();
                                        if let Ok(mut draft) = current.draft.0.lock() {
                                            if !draft
                                                .page
                                                .as_ref()
                                                .is_some_and(|page| page.matches(&current.page))
                                            {
                                                return;
                                            }
                                            draft.composing = event.composing;
                                            draft.revision = draft.revision.saturating_add(1);
                                            composition.send(Request::Filter(
                                                current.page.clone(),
                                                draft.revision,
                                            ));
                                        }
                                    }),
                            ),
                    )
                    .child(when(
                        move || {
                            state.get().total > 0
                                && matches!(state.get().status, SelectorStatus::Failed(_))
                        },
                        move || {
                            text(move || match state.get().status {
                                SelectorStatus::Failed(code) => super::branch_error_label(code),
                                _ => "",
                            })
                            .h(px(BRANCH_ROW_HEIGHT))
                            .px_2()
                            .text_color(move || ink(state.get().colors.danger))
                        },
                    ))
                    .child(
                        virtual_list(
                            move || state.get().rows.len(),
                            move |position| {
                                let rows = rows.clone();
                                dynamic(move || {
                                    let observed = state.get();
                                    let Some(row) = observed.rows.get(position.index()).cloned()
                                    else {
                                        return view().into_element();
                                    };
                                    let target = element_ref();
                                    let lease = Arc::new(());
                                    let send = rows.clone();
                                    let measure = rows.clone();
                                    let measured_snapshot = observed.clone();
                                    let measured_lease = lease.clone();
                                    let branch_id = row.id;
                                    let layout_revision = signal(0_u64);
                                    let selected = row.current;
                                    let visual = observed.visual == Some(row.id);
                                    let disabled =
                                        observed.disabled || observed.pending.is_some() || selected;
                                    effect(move || {
                                        let layout = layout_revision.get();
                                        let revision = geometry_revision.get();
                                        if layout == 0 || !visual || disabled {
                                            return;
                                        }
                                        let Some(revision) = revision else {
                                            return;
                                        };
                                        let measure = measure.clone();
                                        let observed = measured_snapshot.clone();
                                        let lease = Arc::downgrade(&measured_lease);
                                        spawn(async move {
                                            let Some(bounds) = target.measure().await else {
                                                return;
                                            };
                                            let Some(viewport) = list.measure().await else {
                                                return;
                                            };
                                            if target.node().is_some() {
                                                measure.send(Request::Measured(
                                                    observed,
                                                    branch_id,
                                                    [
                                                        bounds.page.x.0,
                                                        bounds.page.y.0,
                                                        bounds.frame.size.width.0,
                                                        bounds.frame.size.height.0,
                                                    ],
                                                    [
                                                        viewport.page.x.0,
                                                        viewport.page.y.0,
                                                        viewport.frame.size.width.0,
                                                        viewport.frame.size.height.0,
                                                    ],
                                                    lease,
                                                    revision,
                                                ));
                                            }
                                        });
                                    });
                                    view()
                                        .element_ref(&target)
                                        .role(AccessibilityRole::Menuitem)
                                        .label(row.label.clone())
                                        .accessibility_state(AccessibilityState {
                                            selected,
                                            disabled,
                                            ..Default::default()
                                        })
                                        .disabled(disabled)
                                        .on_layout(move |_: &Bounds<Points>| {
                                            layout_revision.set(
                                                layout_revision.get_untracked().saturating_add(1),
                                            );
                                        })
                                        .h(px(BRANCH_ROW_HEIGHT))
                                        .w_full()
                                        .flex_row()
                                        .items_center()
                                        .gap_2()
                                        .px(px(crate::menu_list::MENU_ROW_PADDING_X))
                                        .rounded(px(Layout::MENU_RADIUS))
                                        .bg(ink(if visual {
                                            observed.colors.bg_hover
                                        } else if selected {
                                            observed.colors.bg_active_alpha
                                        } else {
                                            observed.colors.bg_elevated
                                        }))
                                        .text_color(ink(if disabled {
                                            observed.colors.text_tertiary
                                        } else {
                                            observed.colors.text_primary
                                        }))
                                        .hover(|style| style.bg(ink(observed.colors.bg_hover)))
                                        .child(icon(BRANCH, ink(observed.colors.text_secondary)))
                                        .child(
                                            text(row.label.clone())
                                                .flex_1()
                                                .min_w(px(0.))
                                                .number_of_lines(1),
                                        )
                                        .child(when(
                                            move || selected,
                                            move || {
                                                icon(CHECK, ink(observed.colors.text_secondary))
                                            },
                                        ))
                                        .on_press(move || {
                                            if target.node().is_some() && !disabled {
                                                send.send(Request::Action(
                                                    observed.clone(),
                                                    Action::Activate(row.id),
                                                    Some(Arc::downgrade(&lease)),
                                                    Some(observed.draft_revision),
                                                ));
                                            }
                                        })
                                        .into_element()
                                })
                            },
                        )
                        .element_ref(&list)
                        .estimated_item_size(px(BRANCH_ROW_HEIGHT))
                        .flex_1()
                        .min_h(px(0.))
                        .w_full()
                        .on_layout(move |bounds: &Bounds<Points>| {
                            viewport.set(bounds.size.height.0)
                        })
                        .on_scroll(move |event: &ScrollEvent| {
                            let current = state.get_untracked();
                            geometry_revision.set(current.geometry.invalidate());
                            scrolling.send(Request::GeometryChanged(current.page));
                            offset.set(event.offset.y.0);
                            viewport.set(event.size.height.0);
                        })
                        .empty(
                            text(move || {
                                let current = state.get();
                                if current.total > 0 {
                                    return vega_i18n::t("ui.no_matching_branches");
                                }
                                match current.status {
                                    SelectorStatus::Loading => vega_i18n::t("ui.loading_branches"),
                                    SelectorStatus::Failed(code) => super::branch_error_label(code),
                                    _ => vega_i18n::t("ui.no_local_branches"),
                                }
                            })
                            .h(px(BRANCH_ROW_HEIGHT))
                            .text_color(move || ink(state.get().colors.text_tertiary)),
                        ),
                    )
                    .child(
                        view()
                            .h(px(1.))
                            .my(px(4.))
                            .mx(px(8.))
                            .flex_shrink_0()
                            .bg(move || ink(state.get().colors.border_subtle)),
                    )
                    .child(
                        view()
                            .role(AccessibilityRole::Menuitem)
                            .disabled(true)
                            .label(move || {
                                state.with(|_| vega_i18n::t("ui.create_and_switch_branch"))
                            })
                            .hint(move || {
                                state.with(|_| {
                                    vega_i18n::t(
                                        "ui.not_available_the_branch_service_only_lists_and",
                                    )
                                })
                            })
                            .tooltip(
                                text(move || {
                                    state.with(|_| {
                                        vega_i18n::t(
                                            "ui.not_available_the_branch_service_only_lists_and",
                                        )
                                    })
                                })
                                .into_element(),
                            )
                            .h(px(BRANCH_CHROME_HEIGHT
                                - crate::menu_list::MENU_ROW_HEIGHT
                                - 13.))
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px(px(crate::menu_list::MENU_ROW_PADDING_X))
                            .text_color(move || ink(state.get().colors.text_tertiary))
                            .child(icon(PLUS, move || ink(state.get().colors.text_tertiary)))
                            .child(text(move || {
                                state.with(|_| vega_i18n::t("ui.create_and_switch_branch"))
                            })),
                    )
            },
        ))
        .into_element()
}
