use super::*;
use std::cell::RefCell;
use std::rc::Rc;

struct RowBounds {
    full: gpui_kit::Bounds<gpui_kit::Pixels>,
    visible: gpui_kit::Bounds<gpui_kit::Pixels>,
}

pub(super) struct ReorderState {
    bounds: Rc<RefCell<HashMap<ReorderItem, RowBounds>>>,
    targets: HashSet<String>,
    pub(super) focus: FocusHandle,
    pub(super) gesture: bool,
    pub(super) pointer_order: Option<(ReorderItem, Vec<String>)>,
    source: Option<ReorderItem>,
    previous_focus: Option<FocusHandle>,
}

impl ReorderState {
    pub(super) fn new(cx: &mut Context<ThreadsBlock>) -> Self {
        Self {
            bounds: Rc::default(),
            targets: HashSet::new(),
            focus: cx.focus_handle(),
            gesture: false,
            pointer_order: None,
            source: None,
            previous_focus: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(in crate::sidebar::threads_block) enum ReorderItem {
    Project(String),
    Thread(String),
}

impl ReorderItem {
    fn id(&self) -> &str {
        match self {
            Self::Project(id) | Self::Thread(id) => id,
        }
    }

    fn action(&self, before_id: Option<String>) -> SidebarOrganizationAction {
        match self {
            Self::Project(id) => SidebarOrganizationAction::MoveProject {
                project_id: id.clone(),
                before_id,
            },
            Self::Thread(id) => SidebarOrganizationAction::ReorderThread {
                thread_id: id.clone(),
                before_id,
            },
        }
    }
}

#[derive(Clone)]
struct ReorderDrag {
    label: String,
}

impl Render for ReorderDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(colors.bg_elevated)
            .border_1()
            .border_color(colors.border_subtle)
            .text_size(px(Typography::SIDEBAR))
            .text_color(colors.text_primary)
            .child(self.label.clone())
    }
}

impl ThreadsBlock {
    fn hold_reorder_projection(&mut self, item: &ReorderItem) {
        let Some(org) = self.organization.as_mut() else {
            return;
        };
        if org.reorder.gesture
            || org
                .reorder
                .pointer_order
                .as_ref()
                .is_some_and(|(source, _)| source == item)
        {
            return;
        }
        org.reorder.pointer_order = org.snapshot.as_ref().map(|snapshot| {
            (
                item.clone(),
                service::sorted_threads(&snapshot.threads, &snapshot.preferences)
                    .into_iter()
                    .map(|thread| thread.id)
                    .collect(),
            )
        });
    }

    pub(super) fn clear_reorder_projection(&mut self, cx: &mut Context<Self>) {
        if let Some(org) = self.organization.as_mut()
            && !org.reorder.gesture
            && org.reorder.pointer_order.take().is_some()
        {
            cx.notify();
        }
    }

    pub(super) fn prepare_reorder(&mut self) {
        let targets = self
            .organization
            .as_ref()
            .and_then(|org| org.reorder.source.as_ref())
            .map(|source| self.reorder_ids(source))
            .unwrap_or_default();
        if let Some(org) = self.organization.as_mut() {
            org.reorder.targets = targets.into_iter().collect();
            org.reorder.bounds.borrow_mut().clear();
        }
    }

    pub(super) fn reset_reorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(org) = self.organization.as_mut().filter(|org| org.reorder.gesture) {
            org.reorder.gesture = false;
            org.reorder.source = None;
            org.reorder.pointer_order = None;
            org.reorder.previous_focus = None;
            cx.stop_active_drag(window);
        }
    }

    fn reorder_ids(&self, item: &ReorderItem) -> Vec<String> {
        let Some(org) = &self.organization else {
            return Vec::new();
        };
        let Some(snapshot) = &org.snapshot else {
            return Vec::new();
        };
        match item {
            ReorderItem::Project(_) => snapshot
                .project_order
                .iter()
                .cloned()
                .chain(
                    snapshot
                        .projects
                        .iter()
                        .filter(|project| !snapshot.project_order.contains(&project.id))
                        .map(|project| project.id.clone()),
                )
                .collect(),
            ReorderItem::Thread(id) => {
                let threads = self.eligible_threads(true);
                let Some(source) = threads.iter().find(|thread| &thread.id == id) else {
                    return Vec::new();
                };
                service::sorted_threads(&threads, &snapshot.preferences)
                    .into_iter()
                    .filter(|thread| {
                        thread.pinned == source.pinned
                            && (source.pinned
                                || thread.project_binding() == source.project_binding())
                    })
                    .map(|thread| thread.id)
                    .collect()
            }
        }
    }

    pub(super) fn reorder_menu_actions(
        &self,
        item: &ReorderItem,
    ) -> Vec<(String, SidebarOrganizationAction)> {
        let ids = self.reorder_ids(item);
        let show_archived = self.organization.as_ref().is_some_and(|org| org.archive);
        let active: HashSet<_> = self
            .threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect();
        let visible: Vec<_> = ids
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                matches!(item, ReorderItem::Project(_))
                    || show_archived
                    || active.contains(id.as_str())
            })
            .map(|(index, _)| index)
            .collect();
        let Some(index) = visible.iter().position(|index| ids[*index] == item.id()) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if index > 0 {
            actions.push((
                vega_i18n::t("ui.move_up").into(),
                item.action(Some(ids[visible[index - 1]].clone())),
            ));
        }
        if let Some(next) = visible.get(index + 1) {
            let before = ids[*next + 1..].iter().find(|id| *id != item.id()).cloned();
            actions.push((vega_i18n::t("ui.move_down").into(), item.action(before)));
        }
        actions
    }

    fn reorder_drop_action(
        &self,
        source: &ReorderItem,
        target: &ReorderItem,
        after: bool,
    ) -> Option<SidebarOrganizationAction> {
        if source == target || std::mem::discriminant(source) != std::mem::discriminant(target) {
            return None;
        }
        let ids = self.reorder_ids(source);
        let index = ids.iter().position(|id| id == target.id())?;
        let before = if after {
            ids[index + 1..]
                .iter()
                .find(|id| *id != source.id())
                .cloned()
        } else {
            Some(target.id().to_owned())
        };
        Some(source.action(before))
    }

    pub(super) fn cancel_reorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(org) = self.organization.as_mut().filter(|org| org.reorder.gesture) else {
            return;
        };
        org.reorder.source = None;
        org.reorder.pointer_order = None;
        cx.stop_active_drag(window);
        if let Some(focus) = org.reorder.previous_focus.take() {
            window.focus(&focus, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn release_reorder(
        &mut self,
        event: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_reorder_projection(cx);
        let Some(org) = self.organization.as_mut().filter(|org| org.reorder.gesture) else {
            return;
        };
        org.reorder.gesture = false;
        org.reorder.pointer_order = None;
        let source = org.reorder.source.take();
        let target = org
            .reorder
            .bounds
            .borrow()
            .iter()
            .find(|(_, bounds)| {
                bounds.visible.size.width > px(0.)
                    && bounds.visible.size.height > px(0.)
                    && bounds.visible.contains(&event.position)
            })
            .map(|(item, bounds)| (item.clone(), event.position.y >= bounds.full.center().y));
        if let Some(focus) = org.reorder.previous_focus.take() {
            window.focus(&focus, cx);
        }
        let action = if cx.has_active_drag() && event.button == MouseButton::Left {
            source
                .as_ref()
                .zip(target.as_ref())
                .and_then(|(source, (target, after))| {
                    self.reorder_drop_action(source, target, *after)
                })
        } else {
            None
        };
        cx.stop_active_drag(window);
        cx.stop_propagation();
        if let Some(action) = action {
            self.submit_organization(action, cx);
        }
        cx.notify();
    }

    pub(in crate::sidebar::threads_block) fn render_reorder_row(
        &self,
        item: ReorderItem,
        label: String,
        row: crate::interaction::Interaction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(org) = &self.organization else {
            return row.into_any_element();
        };
        let colors = theme(cx).colors;
        let bounds_cache = org.reorder.bounds.clone();
        let bounds_item = item.clone();
        let valid_target = !org.pending
            && org.reorder.source.as_ref().is_some_and(|source| {
                source != &item
                    && std::mem::discriminant(source) == std::mem::discriminant(&item)
                    && org.reorder.targets.contains(item.id())
            });
        let drag_entity = cx.entity().downgrade();
        let drag_item = item.clone();
        let hover_item = item.clone();
        let pressed_item = item.clone();
        row.relative()
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button.is_none() {
                    if this.organization.as_ref().is_some_and(|org| org.pending) {
                        this.hold_reorder_projection(&hover_item);
                    } else if this.organization.as_ref().is_some_and(|org| {
                        org.reorder
                            .pointer_order
                            .as_ref()
                            .is_some_and(|(source, _)| source != &hover_item)
                    }) {
                        this.clear_reorder_projection(cx);
                    }
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, _| {
                    this.hold_reorder_projection(&pressed_item);
                }),
            )
            .on_drag(ReorderDrag { label }, move |drag, _, window, cx| {
                drag_entity
                    .update(cx, |this, cx| {
                        this.close_actions();
                        if let Some(org) = this.organization.as_mut() {
                            org.menu = None;
                            org.menu_anchor = None;
                            org.reorder.source = Some(drag_item.clone());
                            org.reorder.gesture = true;
                            org.reorder.previous_focus = window.focused(cx);
                            window.focus(&org.reorder.focus, cx);
                        }
                        cx.notify();
                    })
                    .ok();
                cx.new(|_| drag.clone())
            })
            .child(
                gpui_kit::canvas(
                    move |bounds, window, _| {
                        bounds_cache.borrow_mut().insert(
                            bounds_item,
                            RowBounds {
                                full: bounds,
                                visible: bounds.intersect(&window.content_mask().bounds),
                            },
                        );
                    },
                    move |bounds, _, window, cx| {
                        if valid_target
                            && cx.has_active_drag()
                            && bounds.contains(&window.mouse_position())
                        {
                            let height = px(Layout::SIDEBAR_REORDER_LINE_HEIGHT);
                            let top = if window.mouse_position().y < bounds.center().y {
                                bounds.top()
                            } else {
                                bounds.bottom() - height
                            };
                            window.paint_quad(gpui_kit::fill(
                                gpui_kit::Bounds::new(
                                    point(bounds.left(), top),
                                    gpui_kit::size(bounds.size.width, height),
                                ),
                                colors.accent,
                            ));
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .into_any_element()
    }
}
