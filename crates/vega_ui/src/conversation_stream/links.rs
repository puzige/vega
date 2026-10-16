use std::{ops::Range, time::Duration};

use gpui_kit::{
    App, AppContext, Bounds, CursorStyle, ElementId, Entity, GlobalElementId, Hitbox, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Subscription, TextAlign,
    TextLayout, Window, point, px, size,
};
use tokio_util::sync::CancellationToken;

use super::model::StreamSpan;

pub(crate) type LinkRange = (Range<usize>, String);

const MULTI_CLICK_DELAY: Duration = Duration::from_millis(500);
const CLICK_SLOP: f32 = 4.;

struct LinkInteraction {
    identity: (String, Vec<LinkRange>),
    press: Option<LinkPress>,
    pending: Option<CancellationToken>,
    hovered: Option<usize>,
    focus_out: Option<Subscription>,
    _activation: Subscription,
}

impl LinkInteraction {
    fn cancel(&mut self) {
        self.press = None;
        if let Some(pending) = self.pending.take() {
            pending.cancel();
        }
        self.focus_out = None;
    }
}

impl Drop for LinkInteraction {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct LinkPress {
    target: String,
    origin: Point<Pixels>,
    moved: bool,
}

#[derive(Clone)]
pub(super) struct LinkRegion {
    pub bounds: Bounds<Pixels>,
    pub index: usize,
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct PaintedLinkText {
    pub layout: TextLayout,
    pub links: Vec<LinkRange>,
    pub regions: Vec<LinkRegion>,
}

#[cfg(test)]
thread_local! {
    pub(super) static LINK_LAYOUTS: std::cell::RefCell<Option<Vec<PaintedLinkText>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) fn link_ranges(spans: &[StreamSpan]) -> Vec<LinkRange> {
    let mut offset = 0;
    spans
        .iter()
        .filter_map(|span| {
            let start = offset;
            offset += span.text.len();
            span.link
                .as_ref()
                .filter(|_| start < offset)
                .map(|target| (start..offset, target.clone()))
        })
        .collect()
}

pub(crate) fn fragment_link_ranges(links: &[LinkRange], fragment: &Range<usize>) -> Vec<LinkRange> {
    links
        .iter()
        .filter_map(|(range, target)| {
            let start = range.start.max(fragment.start);
            let end = range.end.min(fragment.end);
            (start < end).then(|| (start - fragment.start..end - fragment.start, target.clone()))
        })
        .collect()
}

pub(crate) fn link_element_id(prefix: &str, ordinal: usize) -> ElementId {
    ElementId::Name(format!("{prefix}-link-{ordinal}").into())
}

pub(crate) fn register_link_interactions(
    id: Option<&GlobalElementId>,
    hitbox: Hitbox,
    layout: TextLayout,
    links: Vec<LinkRange>,
    window: &mut Window,
    cx: &mut App,
) {
    if links.is_empty() {
        return;
    }
    let regions = link_regions(&layout, &links, window.text_style().text_align);
    #[cfg(test)]
    LINK_LAYOUTS.with_borrow_mut(|layouts| {
        if let Some(layouts) = layouts {
            layouts.push(PaintedLinkText {
                layout: layout.clone(),
                links: links.clone(),
                regions: regions.clone(),
            });
        }
    });
    let hovered = link_index(&regions, window.mouse_position())
        .filter(|index| allowed_link(&links[*index].1))
        .filter(|_| hitbox.is_hovered(window));
    if hovered.is_some() {
        window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
    }
    let Some(id) = id.filter(|_| links.iter().any(|(_, target)| allowed_link(target))) else {
        return;
    };
    let identity = (layout.text(), links.clone());
    let state = window.with_element_state::<Entity<LinkInteraction>, _>(id, |current, window| {
        let state = current.unwrap_or_else(|| {
            cx.new::<LinkInteraction>(|cx| LinkInteraction {
                identity: identity.clone(),
                press: None,
                pending: None,
                hovered: None,
                focus_out: None,
                _activation: cx.observe_window_activation(window, |state, window, _| {
                    if !window.is_window_active() {
                        state.cancel();
                    }
                }),
            })
        });
        state.update(cx, |state, _| {
            if state.identity != identity {
                state.cancel();
                state.identity = identity;
            }
            state.hovered = hovered;
        });
        (state.clone(), state)
    });
    let down_state = state.downgrade();
    let down_regions = regions.clone();
    let down_links = links.clone();
    let down_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        let _ = down_state.update(cx, |state, _| {
            if phase.capture() {
                state.cancel();
                return;
            }
            if event.button != MouseButton::Left
                || event.click_count != 1
                || event.modifiers.shift
                || event.modifiers.control
                || event.modifiers.alt
                || event.modifiers.platform
                || !down_hitbox.is_hovered_at(event.position, window)
            {
                return;
            }
            let Some(index) = link_index(&down_regions, event.position)
                .filter(|index| allowed_link(&down_links[*index].1))
            else {
                return;
            };
            state.press = Some(LinkPress {
                target: down_links[index].1.clone(),
                origin: event.position,
                moved: false,
            });
        });
    });

    let move_state = state.downgrade();
    let move_regions = regions.clone();
    let move_links = links.clone();
    let move_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if !phase.capture() {
            return;
        }
        let _ = move_state.update(cx, |state, _| {
            let hovered = link_index(&move_regions, event.position)
                .filter(|index| allowed_link(&move_links[*index].1))
                .filter(|_| move_hitbox.is_hovered_at(event.position, window));
            if state.hovered != hovered {
                state.hovered = hovered;
                window.refresh();
            }
            if let Some(pressed) = state.press.as_mut()
                && event.pressed_button == Some(MouseButton::Left)
            {
                let dx = f32::from(event.position.x - pressed.origin.x);
                let dy = f32::from(event.position.y - pressed.origin.y);
                if dx * dx + dy * dy > CLICK_SLOP * CLICK_SLOP
                    || hovered.is_none_or(|index| move_links[index].1 != pressed.target)
                {
                    pressed.moved = true;
                }
            }
        });
    });

    let up_state = state.downgrade();
    let up_hitbox = hitbox;
    window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
        if !phase.capture() || event.button != MouseButton::Left {
            return;
        }
        let selected_text = gpui_kit::base::TextSelection::selected_text(window, cx);
        let pending = up_state.update(cx, |state, _| {
            let pressed = state.press.take()?;
            let dx = f32::from(event.position.x - pressed.origin.x);
            let dy = f32::from(event.position.y - pressed.origin.y);
            if pressed.moved
                || !selected_text.is_empty()
                || dx * dx + dy * dy > CLICK_SLOP * CLICK_SLOP
                || event.click_count != 1
                || event.modifiers.shift
                || event.modifiers.control
                || event.modifiers.alt
                || event.modifiers.platform
                || !up_hitbox.is_hovered_at(event.position, window)
                || link_index(&regions, event.position)
                    .is_none_or(|index| links[index].1 != pressed.target)
            {
                return None;
            }
            let target = pressed.target;
            let cancellation = CancellationToken::new();
            state.pending = Some(cancellation.clone());
            Some((target, cancellation))
        });
        let Ok(Some((target, cancellation))) = pending else {
            return;
        };
        if let Some(focus) = window.focused(cx) {
            let blur_state = up_state.clone();
            let subscription = window.on_focus_out(&focus, cx, move |_, _, cx| {
                let _ = blur_state.update(cx, |state, _| state.cancel());
            });
            let _ = up_state.update(cx, |state, _| state.focus_out = Some(subscription));
        }
        let weak_state = up_state.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |cx| {
            tokio::select! {
                _ = cancellation.cancelled() => return,
                _ = cx.background_executor().timer(MULTI_CLICK_DELAY) => {}
            }
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = weak_state.update(cx, |state, cx| {
                    if !cancellation.is_cancelled() && window.is_window_active() {
                        state.pending = None;
                        state.focus_out = None;
                        cx.open_url(&target);
                    }
                });
            });
        })
        .detach();
    });
}

fn link_regions(layout: &TextLayout, links: &[LinkRange], align: TextAlign) -> Vec<LinkRegion> {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let mut regions = Vec::new();
    let mut offset = 0;
    let mut y = bounds.top();
    for line in layout.line_layouts() {
        let unwrapped = &line.unwrapped_layout;
        let mut start = 0;
        for end in line
            .wrap_boundaries
            .iter()
            .map(|boundary| unwrapped.runs[boundary.run_ix].glyphs[boundary.glyph_ix].index)
            .chain(std::iter::once(line.len()))
        {
            let start_x = unwrapped.x_for_index(start);
            let width = unwrapped.x_for_index(end) - start_x;
            let x = bounds.left()
                + match align {
                    TextAlign::Left => px(0.),
                    TextAlign::Center => (bounds.size.width - width) / 2.,
                    TextAlign::Right => bounds.size.width - width,
                };
            for (index, (range, _)) in links.iter().enumerate() {
                let from = range.start.max(offset + start);
                let to = range.end.min(offset + end);
                if from >= to {
                    continue;
                }
                let left = x + unwrapped.x_for_index(from - offset) - start_x;
                let right = x + unwrapped.x_for_index(to - offset) - start_x;
                if right > left {
                    regions.push(LinkRegion {
                        bounds: Bounds::new(point(left, y), size(right - left, line_height)),
                        index,
                    });
                }
            }
            start = end;
            y += line_height;
        }
        offset += line.len() + 1;
    }
    regions
}

fn link_index(regions: &[LinkRegion], position: Point<Pixels>) -> Option<usize> {
    regions
        .iter()
        .find(|region| {
            region.bounds.left() <= position.x
                && position.x < region.bounds.right()
                && region.bounds.top() <= position.y
                && position.y < region.bounds.bottom()
        })
        .map(|region| region.index)
}

pub(crate) fn allowed_link(target: &str) -> bool {
    if target.chars().any(char::is_whitespace) || target.chars().any(char::is_control) {
        return false;
    }
    let Some((scheme, remainder)) = target.split_once(':') else {
        return false;
    };
    if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
        remainder
            .strip_prefix("//")
            .and_then(|rest| rest.split(['/', '?', '#']).next())
            .is_some_and(|authority| !authority.is_empty())
    } else {
        scheme.eq_ignore_ascii_case("mailto") && !remainder.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::allowed_link;

    #[test]
    fn allows_only_absolute_http_https_and_mailto_targets() {
        for target in [
            "http://example.com/path",
            "https://example.com/path?q=1#section",
            "mailto:user@example.com",
            "mailto:?subject=Hello",
            "HTTPS://example.com",
            "http://[::1]:8080/path",
        ] {
            assert!(allowed_link(target));
        }
        for target in [
            "example.com",
            "/relative/path",
            "//example.com",
            "javascript:alert(1)",
            "data:text/plain,hello",
            "http:example.com",
            "http:///path",
            "mailto:",
            "mailto:user @example.com",
            " https://example.com",
            "https://example.com\n",
        ] {
            assert!(!allowed_link(target));
        }
    }
}
