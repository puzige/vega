use gpui_kit::{MouseButton, MouseMoveEvent, Pixels, Size};

pub(in crate::window) const MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH: f32 = 610.;
pub(in crate::window) const MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT: f32 = 480.;

#[derive(Default)]
pub(in crate::window) struct Geometry {
    width: Option<f32>,
    height: Option<f32>,
    window_size: Option<Size<Pixels>>,
    dragging: Option<bool>,
}

impl Geometry {
    pub(in crate::window) fn for_route(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }

    pub(in crate::window) fn window_resized(
        &mut self,
        size: Size<Pixels>,
        reveal_tabs: &mut [bool; 2],
    ) -> bool {
        if self.window_size == Some(size) {
            return false;
        }
        self.window_size = Some(size);
        *reveal_tabs = [true; 2];
        true
    }

    pub(in crate::window) fn begin_drag(&mut self, bottom: bool) {
        self.dragging = Some(bottom);
    }

    pub(in crate::window) fn end_drag(&mut self) {
        self.dragging = None;
    }

    pub(in crate::window) fn move_pointer(
        &mut self,
        event: &MouseMoveEvent,
        size: Size<Pixels>,
        reveal_tabs: &mut [bool; 2],
    ) -> bool {
        let Some(bottom) = self.dragging else {
            return false;
        };
        if !event
            .pressed_button
            .is_some_and(|button| button == MouseButton::Left)
        {
            self.end_drag();
            return false;
        }
        *reveal_tabs = [true; 2];
        if bottom {
            self.height = Some(f32::from(size.height - event.position.y));
        } else {
            self.width = Some(f32::from(size.width - event.position.x));
        }
        true
    }

    pub(in crate::window) fn dimensions(
        &self,
        available: f32,
        window_height: f32,
        default_height: f32,
        maximized: [bool; 2],
    ) -> (f32, f32) {
        let width = if maximized[0] {
            available - 300.
        } else {
            self.width.unwrap_or(available * 0.43)
        }
        .clamp(270., (available - 300.).max(270.));
        let height = if maximized[1] {
            window_height - 240.
        } else {
            self.height.unwrap_or(default_height)
        }
        .clamp(150., (window_height - 240.).max(150.));
        (width, height)
    }
}

pub(in crate::window) struct Panes {
    pub(in crate::window) selected: [bool; 2],
    pub(in crate::window) hidden: [bool; 2],
    pub(in crate::window) maximized: [bool; 2],
}

impl Panes {
    pub(in crate::window) fn dock_visible(&self, index: usize, extent: f32) -> bool {
        !self.hidden[index]
            && self.selected[index]
            && extent
                >= if index == 0 {
                    MIN_RIGHT_WORKSPACE_AVAILABLE_WIDTH
                } else {
                    MIN_BOTTOM_WORKSPACE_WINDOW_HEIGHT
                }
    }

    pub(in crate::window) fn fullscreen(&self) -> Option<usize> {
        (0..2).find(|index| self.maximized[*index] && !self.hidden[*index] && self.selected[*index])
    }

    pub(in crate::window) fn separator_visible(&self, index: usize, extent: f32) -> bool {
        self.dock_visible(index, extent) && self.fullscreen().is_none()
    }

    pub(in crate::window) fn visible(&self, index: usize, extent: f32) -> bool {
        self.fullscreen()
            .map_or_else(|| self.dock_visible(index, extent), |pane| pane == index)
    }
}
